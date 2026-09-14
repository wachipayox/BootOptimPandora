//! Default-off NTFS/USN asset verification cache.
//!
//! A SHA-1 may be skipped only after the Windows implementation has produced a
//! `VerifiedReuse` decision while holding the protected file handle. Every
//! unavailable or ambiguous capability falls back to Pandora's stock SHA-1.

use std::{collections::HashSet, path::Path, sync::Arc};

use bridge::modal_action::AssetVerificationMode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[cfg(windows)]
mod windows;

pub(crate) const ASSET_USN_CACHE_ENV: &str = "BOOTOPTIM_ASSET_USN_CACHE";
pub(crate) const ASSET_USN_HELPER_ENV: &str = "BOOTOPTIM_ASSET_USN_HELPER";
pub(crate) const ASSET_USN_HELPER_SHA256_ENV: &str = "BOOTOPTIM_ASSET_USN_HELPER_SHA256";
const MANIFEST_SCHEMA: u32 = 1;
const MAX_MANIFEST_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone)]
pub(crate) struct AssetUsnCacheRuntime {
    requested: bool,
}

impl AssetUsnCacheRuntime {
    pub(crate) fn from_environment() -> Self {
        Self {
            requested: std::env::var_os(ASSET_USN_CACHE_ENV).is_some_and(|value| value == "1"),
        }
    }

    #[cfg(test)]
    fn requested_for_test(requested: bool) -> Self {
        Self { requested }
    }

    pub(crate) fn requested(&self) -> bool {
        self.requested
    }

    pub(crate) fn can_skip_sha1(
        &self,
        mode: AssetVerificationMode,
        decision: ReuseDecision,
    ) -> bool {
        self.requested
            && mode == AssetVerificationMode::Normal
            && decision == ReuseDecision::VerifiedReuse
    }
}

pub(crate) struct AssetUsnCacheSession {
    runtime: AssetUsnCacheRuntime,
    mode: AssetVerificationMode,
    #[cfg(windows)]
    inner: Option<Arc<windows::WindowsSession>>,
}

impl AssetUsnCacheSession {
    pub(crate) fn begin(
        mode: AssetVerificationMode,
        asset_index_sha1: &str,
        assets_objects_dir: Arc<Path>,
        expected_hashes: Vec<String>,
    ) -> Self {
        let runtime = AssetUsnCacheRuntime::from_environment();

        #[cfg(windows)]
        let inner = if runtime.requested() && mode == AssetVerificationMode::Normal {
            windows::WindowsSession::begin(asset_index_sha1, assets_objects_dir, expected_hashes)
                .ok()
                .map(Arc::new)
        } else {
            None
        };

        #[cfg(not(windows))]
        let _ = (asset_index_sha1, assets_objects_dir, expected_hashes);

        Self {
            runtime,
            mode,
            #[cfg(windows)]
            inner,
        }
    }

    pub(crate) fn verify_existing(
        &self,
        path: &Path,
        expected_sha1: &str,
        expected_hash: [u8; 20],
    ) -> bool {
        #[cfg(windows)]
        if let Some(inner) = &self.inner {
            return inner.verify_existing(
                &self.runtime,
                self.mode,
                path,
                expected_sha1,
                expected_hash,
            );
        }

        crate::fs::check_sha1_hash(path, expected_hash).unwrap_or(false)
    }

    pub(crate) fn finish(&self) {
        #[cfg(windows)]
        if let Some(inner) = &self.inner {
            inner.finish();
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct CacheManifest {
    pub schema: u32,
    pub asset_index_sha1: String,
    pub volume_guid: String,
    pub volume_serial: u64,
    pub journal_id: u64,
    pub snapshot_first_usn: i64,
    pub snapshot_lowest_valid_usn: i64,
    pub snapshot_next_usn: i64,
    pub asset_count: u32,
    pub assets: Vec<CachedAsset>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct CachedAsset {
    pub expected_sha1: String,
    /// NTFS v1 uses the documented 64-bit file index. It is encoded in the
    /// lower half of a fixed 16-byte protocol field, with the upper half zero.
    pub file_id: String,
    pub last_usn: i64,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum ManifestError {
    #[error("manifest is too large")]
    TooLarge,
    #[error("manifest JSON is invalid")]
    Json,
    #[error("unknown manifest schema")]
    Schema,
    #[error("invalid asset-index SHA-1")]
    AssetIndexSha1,
    #[error("invalid local volume GUID")]
    VolumeGuid,
    #[error("invalid USN snapshot")]
    Usn,
    #[error("asset count does not match manifest")]
    AssetCount,
    #[error("invalid cached asset entry")]
    AssetEntry,
    #[error("duplicate cached asset SHA-1")]
    DuplicateAsset,
}

pub(crate) fn parse_manifest(bytes: &[u8]) -> Result<CacheManifest, ManifestError> {
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(ManifestError::TooLarge);
    }

    let manifest: CacheManifest = serde_json::from_slice(bytes).map_err(|_| ManifestError::Json)?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

pub(crate) fn validate_manifest(manifest: &CacheManifest) -> Result<(), ManifestError> {
    if manifest.schema != MANIFEST_SCHEMA {
        return Err(ManifestError::Schema);
    }
    if !is_lower_hex(&manifest.asset_index_sha1, 40) {
        return Err(ManifestError::AssetIndexSha1);
    }
    if !is_volume_guid(&manifest.volume_guid) {
        return Err(ManifestError::VolumeGuid);
    }
    if manifest.snapshot_first_usn < 0
        || manifest.snapshot_lowest_valid_usn < 0
        || manifest.snapshot_next_usn < 0
        || manifest.snapshot_next_usn < manifest.snapshot_first_usn
        || manifest.snapshot_next_usn < manifest.snapshot_lowest_valid_usn
    {
        return Err(ManifestError::Usn);
    }
    if usize::try_from(manifest.asset_count).ok() != Some(manifest.assets.len()) {
        return Err(ManifestError::AssetCount);
    }

    let mut seen = HashSet::with_capacity(manifest.assets.len());
    for asset in &manifest.assets {
        if !is_lower_hex(&asset.expected_sha1, 40)
            || !is_lower_hex(&asset.file_id, 32)
            || asset.file_id.as_bytes()[16..].iter().any(|byte| *byte != b'0')
            || asset.last_usn < 0
            || asset.last_usn > manifest.snapshot_next_usn
        {
            return Err(ManifestError::AssetEntry);
        }
        if !seen.insert(asset.expected_sha1.as_str()) {
            return Err(ManifestError::DuplicateAsset);
        }
    }
    Ok(())
}

pub(crate) fn is_lower_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(crate) fn is_volume_guid(value: &str) -> bool {
    let Some(inner) = value
        .strip_prefix(r"\\?\Volume{")
        .and_then(|value| value.strip_suffix(r"}\"))
    else {
        return false;
    };
    Uuid::parse_str(inner).is_ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CapabilityFailure {
    HelperMissing,
    HelperIdentityMismatch,
    UacDenied,
    HelperCrash,
    Timeout,
    MalformedProtocol,
    InvalidPipeAcl,
    InvalidPeerPid,
    Io,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MissReason {
    FeatureDisabled,
    FullVerification,
    Capability(CapabilityFailure),
    NonNtfs,
    ReparsePoint,
    NotRegularFile,
    FreezeHandleUnavailable,
    AssetIndexMismatch,
    AssetSha1Mismatch,
    VolumeMismatch,
    JournalIdMismatch,
    JournalRegression,
    JournalDiscontinuity,
    InvalidUsn,
    FileIdMismatch,
    FileUsnMismatch,
    HandleIdentityChanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReuseDecision {
    FullSha1(MissReason),
    VerifiedReuse,
}

#[derive(Debug, Clone)]
pub(crate) struct HitEvidence<'a> {
    pub feature_requested: bool,
    pub verification_mode: AssetVerificationMode,
    pub capability: Result<(), CapabilityFailure>,
    pub ntfs: bool,
    pub reparse_point: bool,
    pub regular_file: bool,
    pub freeze_handle_held: bool,
    pub asset_index_sha1: &'a str,
    pub expected_asset_sha1: &'a str,
    pub volume_guid: &'a str,
    pub volume_serial: u64,
    pub journal_id: u64,
    pub current_first_usn: i64,
    pub current_lowest_valid_usn: i64,
    pub current_next_usn: i64,
    pub current_file_id: &'a str,
    pub current_file_usn: i64,
    pub handle_identity_unchanged: bool,
}

pub(crate) fn evaluate_hit(
    manifest: &CacheManifest,
    cached: &CachedAsset,
    evidence: &HitEvidence<'_>,
) -> ReuseDecision {
    use MissReason::*;

    if !evidence.feature_requested {
        return ReuseDecision::FullSha1(FeatureDisabled);
    }
    if evidence.verification_mode != AssetVerificationMode::Normal {
        return ReuseDecision::FullSha1(FullVerification);
    }
    if let Err(reason) = evidence.capability {
        return ReuseDecision::FullSha1(Capability(reason));
    }
    if !evidence.ntfs {
        return ReuseDecision::FullSha1(NonNtfs);
    }
    if evidence.reparse_point {
        return ReuseDecision::FullSha1(ReparsePoint);
    }
    if !evidence.regular_file {
        return ReuseDecision::FullSha1(NotRegularFile);
    }
    if !evidence.freeze_handle_held {
        return ReuseDecision::FullSha1(FreezeHandleUnavailable);
    }
    if evidence.asset_index_sha1 != manifest.asset_index_sha1 {
        return ReuseDecision::FullSha1(AssetIndexMismatch);
    }
    if evidence.expected_asset_sha1 != cached.expected_sha1 {
        return ReuseDecision::FullSha1(AssetSha1Mismatch);
    }
    if evidence.volume_guid != manifest.volume_guid
        || evidence.volume_serial != manifest.volume_serial
    {
        return ReuseDecision::FullSha1(VolumeMismatch);
    }
    if evidence.journal_id != manifest.journal_id {
        return ReuseDecision::FullSha1(JournalIdMismatch);
    }
    if evidence.current_first_usn < 0
        || evidence.current_lowest_valid_usn < 0
        || evidence.current_next_usn < 0
        || evidence.current_file_usn < 0
        || evidence.current_next_usn < evidence.current_first_usn
        || evidence.current_next_usn < evidence.current_lowest_valid_usn
    {
        return ReuseDecision::FullSha1(InvalidUsn);
    }
    if evidence.current_next_usn < manifest.snapshot_next_usn {
        return ReuseDecision::FullSha1(JournalRegression);
    }
    if evidence
        .current_first_usn
        .max(evidence.current_lowest_valid_usn)
        > manifest.snapshot_next_usn
    {
        return ReuseDecision::FullSha1(JournalDiscontinuity);
    }
    if evidence.current_file_id != cached.file_id {
        return ReuseDecision::FullSha1(FileIdMismatch);
    }
    if evidence.current_file_usn != cached.last_usn {
        return ReuseDecision::FullSha1(FileUsnMismatch);
    }
    if !evidence.handle_identity_unchanged {
        return ReuseDecision::FullSha1(HandleIdentityChanged);
    }

    ReuseDecision::VerifiedReuse
}

#[cfg(test)]
mod tests {
    use super::*;

    const INDEX_SHA: &str = "0123456789abcdef0123456789abcdef01234567";
    const ASSET_SHA: &str = "1123456789abcdef0123456789abcdef01234567";
    const OTHER_SHA: &str = "2123456789abcdef0123456789abcdef01234567";
    const FILE_ID: &str = "00112233445566770000000000000000";
    const FILE_ID2: &str = "10112233445566770000000000000000";
    const VOLUME: &str = r"\\?\Volume{12345678-1234-5678-9abc-def012345678}\";

    fn manifest() -> CacheManifest {
        CacheManifest {
            schema: 1,
            asset_index_sha1: INDEX_SHA.into(),
            volume_guid: VOLUME.into(),
            volume_serial: 7,
            journal_id: 11,
            snapshot_first_usn: 50,
            snapshot_lowest_valid_usn: 100,
            snapshot_next_usn: 1_000,
            asset_count: 1,
            assets: vec![CachedAsset {
                expected_sha1: ASSET_SHA.into(),
                file_id: FILE_ID.into(),
                last_usn: 900,
            }],
        }
    }

    fn evidence() -> HitEvidence<'static> {
        HitEvidence {
            feature_requested: true,
            verification_mode: AssetVerificationMode::Normal,
            capability: Ok(()),
            ntfs: true,
            reparse_point: false,
            regular_file: true,
            freeze_handle_held: true,
            asset_index_sha1: INDEX_SHA,
            expected_asset_sha1: ASSET_SHA,
            volume_guid: VOLUME,
            volume_serial: 7,
            journal_id: 11,
            current_first_usn: 50,
            current_lowest_valid_usn: 100,
            current_next_usn: 1_200,
            current_file_id: FILE_ID,
            current_file_usn: 900,
            handle_identity_unchanged: true,
        }
    }

    fn miss(evidence: HitEvidence<'static>, reason: MissReason) {
        let manifest = manifest();
        assert_eq!(
            evaluate_hit(&manifest, &manifest.assets[0], &evidence),
            ReuseDecision::FullSha1(reason)
        );
    }

    #[test]
    fn normal_complete_identity_is_the_only_pure_hit() {
        let manifest = manifest();
        assert_eq!(
            evaluate_hit(&manifest, &manifest.assets[0], &evidence()),
            ReuseDecision::VerifiedReuse
        );

        let runtime = AssetUsnCacheRuntime::requested_for_test(true);
        assert!(runtime.can_skip_sha1(
            AssetVerificationMode::Normal,
            ReuseDecision::VerifiedReuse
        ));
    }

    #[test]
    fn full_verification_cli_and_legacy_authority_never_skip() {
        let mut evidence = evidence();
        evidence.verification_mode = AssetVerificationMode::FullVerification;
        miss(evidence, MissReason::FullVerification);

        let runtime = AssetUsnCacheRuntime::requested_for_test(true);
        assert!(!runtime.can_skip_sha1(
            AssetVerificationMode::FullVerification,
            ReuseDecision::VerifiedReuse
        ));
    }

    #[test]
    fn feature_is_default_fail_closed() {
        let mut evidence = evidence();
        evidence.feature_requested = false;
        miss(evidence, MissReason::FeatureDisabled);

        let runtime = AssetUsnCacheRuntime::requested_for_test(false);
        assert!(!runtime.can_skip_sha1(
            AssetVerificationMode::Normal,
            ReuseDecision::VerifiedReuse
        ));
    }

    #[test]
    fn same_size_and_restored_mtime_still_miss_when_usn_changes() {
        let mut evidence = evidence();
        evidence.current_file_usn += 1;
        miss(evidence, MissReason::FileUsnMismatch);
    }

    #[test]
    fn truncate_restore_still_misses_when_usn_changes() {
        let mut evidence = evidence();
        evidence.current_file_usn += 2;
        miss(evidence, MissReason::FileUsnMismatch);
    }

    #[test]
    fn delete_recreate_and_file_id_replacement_miss() {
        let mut evidence = evidence();
        evidence.current_file_id = FILE_ID2;
        miss(evidence, MissReason::FileIdMismatch);
    }

    #[test]
    fn journal_restamp_regression_and_both_lower_bounds_miss() {
        let mut evidence = evidence();
        evidence.journal_id += 1;
        miss(evidence, MissReason::JournalIdMismatch);

        let mut evidence = evidence();
        evidence.current_next_usn = 999;
        miss(evidence, MissReason::JournalRegression);

        let mut evidence = evidence();
        evidence.current_lowest_valid_usn = 1_001;
        miss(evidence, MissReason::JournalDiscontinuity);

        let mut evidence = evidence();
        evidence.current_first_usn = 1_001;
        miss(evidence, MissReason::JournalDiscontinuity);
    }

    #[test]
    fn asset_index_asset_hash_volume_and_identity_changes_miss() {
        let mut evidence = evidence();
        evidence.asset_index_sha1 = OTHER_SHA;
        miss(evidence, MissReason::AssetIndexMismatch);

        let mut evidence = evidence();
        evidence.expected_asset_sha1 = OTHER_SHA;
        miss(evidence, MissReason::AssetSha1Mismatch);

        let mut evidence = evidence();
        evidence.volume_serial = 8;
        miss(evidence, MissReason::VolumeMismatch);

        let mut evidence = evidence();
        evidence.handle_identity_unchanged = false;
        miss(evidence, MissReason::HandleIdentityChanged);
    }

    #[test]
    fn helper_uac_timeout_protocol_acl_and_pid_fail_closed() {
        for failure in [
            CapabilityFailure::HelperMissing,
            CapabilityFailure::HelperIdentityMismatch,
            CapabilityFailure::UacDenied,
            CapabilityFailure::HelperCrash,
            CapabilityFailure::Timeout,
            CapabilityFailure::MalformedProtocol,
            CapabilityFailure::InvalidPipeAcl,
            CapabilityFailure::InvalidPeerPid,
            CapabilityFailure::Io,
        ] {
            let mut evidence = evidence();
            evidence.capability = Err(failure);
            miss(evidence, MissReason::Capability(failure));
        }
    }

    #[test]
    fn non_ntfs_reparse_nonregular_and_toctou_protection_failure_miss() {
        let mut evidence = evidence();
        evidence.ntfs = false;
        miss(evidence, MissReason::NonNtfs);

        let mut evidence = evidence();
        evidence.reparse_point = true;
        miss(evidence, MissReason::ReparsePoint);

        let mut evidence = evidence();
        evidence.regular_file = false;
        miss(evidence, MissReason::NotRegularFile);

        let mut evidence = evidence();
        evidence.freeze_handle_held = false;
        miss(evidence, MissReason::FreezeHandleUnavailable);
    }

    #[test]
    fn corrupt_truncated_unknown_duplicate_partial_and_invalid_usn_manifests_are_rejected() {
        assert_eq!(parse_manifest(b"{"), Err(ManifestError::Json));

        let mut manifest = manifest();
        manifest.schema = 2;
        assert_eq!(
            parse_manifest(&serde_json::to_vec(&manifest).unwrap()),
            Err(ManifestError::Schema)
        );

        let mut manifest = manifest();
        manifest.snapshot_first_usn = 1_001;
        assert_eq!(
            parse_manifest(&serde_json::to_vec(&manifest).unwrap()),
            Err(ManifestError::Usn)
        );

        let mut manifest = manifest();
        manifest.asset_count = 2;
        assert_eq!(
            parse_manifest(&serde_json::to_vec(&manifest).unwrap()),
            Err(ManifestError::AssetCount)
        );

        let mut manifest = manifest();
        manifest.assets.push(manifest.assets[0].clone());
        manifest.asset_count = 2;
        assert_eq!(
            parse_manifest(&serde_json::to_vec(&manifest).unwrap()),
            Err(ManifestError::DuplicateAsset)
        );

        let mut manifest = manifest();
        manifest.assets[0].file_id = "00112233445566778899aabbccddeeff".into();
        assert_eq!(
            parse_manifest(&serde_json::to_vec(&manifest).unwrap()),
            Err(ManifestError::AssetEntry)
        );

        let duplicate_top_level = format!(
            r#"{{"schema":1,"schema":1,"asset_index_sha1":"{INDEX_SHA}","volume_guid":"{VOLUME}","volume_serial":7,"journal_id":11,"snapshot_first_usn":50,"snapshot_lowest_valid_usn":100,"snapshot_next_usn":1000,"asset_count":0,"assets":[]}}"#
        );
        assert_eq!(
            parse_manifest(duplicate_top_level.as_bytes()),
            Err(ManifestError::Json)
        );
    }
}
