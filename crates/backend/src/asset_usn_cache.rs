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
pub(crate) struct AssetUsnCacheRuntime { requested: bool }

impl AssetUsnCacheRuntime {
    pub(crate) fn from_environment() -> Self {
        Self { requested: std::env::var_os(ASSET_USN_CACHE_ENV).is_some_and(|value| value == "1") }
    }
    #[cfg(test)] fn requested_for_test(requested: bool) -> Self { Self { requested } }
    pub(crate) fn requested(&self) -> bool { self.requested }
    pub(crate) fn can_skip_sha1(&self, mode: AssetVerificationMode, decision: ReuseDecision) -> bool {
        self.requested && mode == AssetVerificationMode::Normal && decision == ReuseDecision::VerifiedReuse
    }
}

pub(crate) struct AssetUsnCacheSession {
    runtime: AssetUsnCacheRuntime,
    mode: AssetVerificationMode,
    #[cfg(windows)] inner: Option<Arc<windows::WindowsSession>>,
}

impl AssetUsnCacheSession {
    pub(crate) fn begin(mode: AssetVerificationMode, asset_index_sha1: &str, assets_objects_dir: Arc<Path>, expected_hashes: Vec<String>) -> Self {
        let runtime = AssetUsnCacheRuntime::from_environment();
        #[cfg(windows)]
        let inner = if runtime.requested() && mode == AssetVerificationMode::Normal {
            windows::WindowsSession::begin(asset_index_sha1, assets_objects_dir, expected_hashes).ok().map(Arc::new)
        } else { None };
        #[cfg(not(windows))] let _ = (asset_index_sha1, assets_objects_dir, expected_hashes);
        Self { runtime, mode, #[cfg(windows)] inner }
    }

    pub(crate) fn verify_existing(&self, path: &Path, expected_sha1: &str, expected_hash: [u8; 20]) -> bool {
        #[cfg(windows)] if let Some(inner) = &self.inner {
            return inner.verify_existing(&self.runtime, self.mode, path, expected_sha1, expected_hash);
        }
        crate::fs::check_sha1_hash(path, expected_hash).unwrap_or(false)
    }

    pub(crate) fn finish(&self) {
        #[cfg(windows)] if let Some(inner) = &self.inner { inner.finish(); }
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
    pub file_id: String,
    pub last_usn: i64,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum ManifestError {
    #[error("manifest is too large")] TooLarge,
    #[error("manifest JSON is invalid")] Json,
    #[error("unknown manifest schema")] Schema,
    #[error("invalid asset-index SHA-1")] AssetIndexSha1,
    #[error("invalid local volume GUID")] VolumeGuid,
    #[error("invalid USN snapshot")] Usn,
    #[error("asset count does not match manifest")] AssetCount,
    #[error("invalid cached asset entry")] AssetEntry,
    #[error("duplicate cached asset SHA-1")] DuplicateAsset,
}

pub(crate) fn parse_manifest(bytes: &[u8]) -> Result<CacheManifest, ManifestError> {
    if bytes.len() > MAX_MANIFEST_BYTES { return Err(ManifestError::TooLarge); }
    let manifest: CacheManifest = serde_json::from_slice(bytes).map_err(|_| ManifestError::Json)?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

pub(crate) fn validate_manifest(manifest: &CacheManifest) -> Result<(), ManifestError> {
    if manifest.schema != MANIFEST_SCHEMA { return Err(ManifestError::Schema); }
    if !is_lower_hex(&manifest.asset_index_sha1, 40) { return Err(ManifestError::AssetIndexSha1); }
    if !is_volume_guid(&manifest.volume_guid) { return Err(ManifestError::VolumeGuid); }
    if manifest.snapshot_first_usn < 0 || manifest.snapshot_lowest_valid_usn < 0 || manifest.snapshot_next_usn < 0
        || manifest.snapshot_next_usn < manifest.snapshot_first_usn || manifest.snapshot_next_usn < manifest.snapshot_lowest_valid_usn {
        return Err(ManifestError::Usn);
    }
    if usize::try_from(manifest.asset_count).ok() != Some(manifest.assets.len()) { return Err(ManifestError::AssetCount); }
    let mut seen = HashSet::with_capacity(manifest.assets.len());
    for asset in &manifest.assets {
        if !is_lower_hex(&asset.expected_sha1, 40) || !is_lower_hex(&asset.file_id, 32)
            || asset.file_id.as_bytes()[16..].iter().any(|b| *b != b'0')
            || asset.last_usn < 0 || asset.last_usn > manifest.snapshot_next_usn {
            return Err(ManifestError::AssetEntry);
        }
        if !seen.insert(asset.expected_sha1.as_str()) { return Err(ManifestError::DuplicateAsset); }
    }
    Ok(())
}

pub(crate) fn is_lower_hex(value: &str, len: usize) -> bool {
    value.len() == len && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub(crate) fn is_volume_guid(value: &str) -> bool {
    let Some(inner) = value.strip_prefix(r"\\?\Volume{").and_then(|v| v.strip_suffix(r"}\")) else { return false; };
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
pub(crate) enum ReuseDecision { FullSha1(MissReason), VerifiedReuse }

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

pub(crate) fn evaluate_hit(manifest: &CacheManifest, cached: &CachedAsset, evidence: &HitEvidence<'_>) -> ReuseDecision {
    use MissReason::*;
    if !evidence.feature_requested { return ReuseDecision::FullSha1(FeatureDisabled); }
    if evidence.verification_mode != AssetVerificationMode::Normal { return ReuseDecision::FullSha1(FullVerification); }
    if let Err(reason) = evidence.capability { return ReuseDecision::FullSha1(Capability(reason)); }
    if !evidence.ntfs { return ReuseDecision::FullSha1(NonNtfs); }
    if evidence.reparse_point { return ReuseDecision::FullSha1(ReparsePoint); }
    if !evidence.regular_file { return ReuseDecision::FullSha1(NotRegularFile); }
    if !evidence.freeze_handle_held { return ReuseDecision::FullSha1(FreezeHandleUnavailable); }
    if evidence.asset_index_sha1 != manifest.asset_index_sha1 { return ReuseDecision::FullSha1(AssetIndexMismatch); }
    if evidence.expected_asset_sha1 != cached.expected_sha1 { return ReuseDecision::FullSha1(AssetSha1Mismatch); }
    if evidence.volume_guid != manifest.volume_guid || evidence.volume_serial != manifest.volume_serial { return ReuseDecision::FullSha1(VolumeMismatch); }
    if evidence.journal_id != manifest.journal_id { return ReuseDecision::FullSha1(JournalIdMismatch); }
    if evidence.current_first_usn < 0 || evidence.current_lowest_valid_usn < 0 || evidence.current_next_usn < 0 || evidence.current_file_usn < 0
        || evidence.current_next_usn < evidence.current_first_usn || evidence.current_next_usn < evidence.current_lowest_valid_usn {
        return ReuseDecision::FullSha1(InvalidUsn);
    }
    if evidence.current_next_usn < manifest.snapshot_next_usn { return ReuseDecision::FullSha1(JournalRegression); }
    if evidence.current_first_usn.max(evidence.current_lowest_valid_usn) > manifest.snapshot_next_usn { return ReuseDecision::FullSha1(JournalDiscontinuity); }
    if evidence.current_file_id != cached.file_id { return ReuseDecision::FullSha1(FileIdMismatch); }
    if evidence.current_file_usn != cached.last_usn { return ReuseDecision::FullSha1(FileUsnMismatch); }
    if !evidence.handle_identity_unchanged { return ReuseDecision::FullSha1(HandleIdentityChanged); }
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
    fn manifest() -> CacheManifest { CacheManifest { schema: 1, asset_index_sha1: INDEX_SHA.into(), volume_guid: VOLUME.into(), volume_serial: 7, journal_id: 11, snapshot_first_usn: 50, snapshot_lowest_valid_usn: 100, snapshot_next_usn: 1_000, asset_count: 1, assets: vec![CachedAsset { expected_sha1: ASSET_SHA.into(), file_id: FILE_ID.into(), last_usn: 900 }] } }
    fn evidence() -> HitEvidence<'static> { HitEvidence { feature_requested: true, verification_mode: AssetVerificationMode::Normal, capability: Ok(()), ntfs: true, reparse_point: false, regular_file: true, freeze_handle_held: true, asset_index_sha1: INDEX_SHA, expected_asset_sha1: ASSET_SHA, volume_guid: VOLUME, volume_serial: 7, journal_id: 11, current_first_usn: 50, current_lowest_valid_usn: 100, current_next_usn: 1_200, current_file_id: FILE_ID, current_file_usn: 900, handle_identity_unchanged: true } }
    fn miss(e: HitEvidence<'static>, reason: MissReason) { let m = manifest(); assert_eq!(evaluate_hit(&m, &m.assets[0], &e), ReuseDecision::FullSha1(reason)); }

    #[test] fn normal_complete_identity_is_the_only_pure_hit() { let m = manifest(); assert_eq!(evaluate_hit(&m, &m.assets[0], &evidence()), ReuseDecision::VerifiedReuse); let r = AssetUsnCacheRuntime::requested_for_test(true); assert!(r.can_skip_sha1(AssetVerificationMode::Normal, ReuseDecision::VerifiedReuse)); }
    #[test] fn full_verification_cli_legacy_authority_never_skips() { let mut e = evidence(); e.verification_mode = AssetVerificationMode::FullVerification; miss(e, MissReason::FullVerification); let r = AssetUsnCacheRuntime::requested_for_test(true); assert!(!r.can_skip_sha1(AssetVerificationMode::FullVerification, ReuseDecision::VerifiedReuse)); }
    #[test] fn opt_out_never_skips() { let mut e = evidence(); e.feature_requested = false; miss(e, MissReason::FeatureDisabled); let r = AssetUsnCacheRuntime::requested_for_test(false); assert!(!r.can_skip_sha1(AssetVerificationMode::Normal, ReuseDecision::VerifiedReuse)); }
    #[test] fn mutations_and_identity_changes_miss() { let mut e=evidence(); e.current_file_usn+=1; miss(e,MissReason::FileUsnMismatch); let mut e=evidence(); e.current_file_usn+=2; miss(e,MissReason::FileUsnMismatch); let mut e=evidence(); e.current_file_id=FILE_ID2; miss(e,MissReason::FileIdMismatch); let mut e=evidence(); e.asset_index_sha1=OTHER_SHA; miss(e,MissReason::AssetIndexMismatch); let mut e=evidence(); e.expected_asset_sha1=OTHER_SHA; miss(e,MissReason::AssetSha1Mismatch); let mut e=evidence(); e.volume_serial=8; miss(e,MissReason::VolumeMismatch); let mut e=evidence(); e.handle_identity_unchanged=false; miss(e,MissReason::HandleIdentityChanged); }
    #[test] fn journal_restamp_regression_and_discontinuity_miss() { let mut e=evidence(); e.journal_id+=1; miss(e,MissReason::JournalIdMismatch); let mut e=evidence(); e.current_next_usn=999; miss(e,MissReason::JournalRegression); let mut e=evidence(); e.current_lowest_valid_usn=1001; miss(e,MissReason::JournalDiscontinuity); let mut e=evidence(); e.current_first_usn=1001; miss(e,MissReason::JournalDiscontinuity); }
    #[test] fn helper_pipe_and_toctou_failures_miss() { for f in [CapabilityFailure::HelperMissing,CapabilityFailure::HelperIdentityMismatch,CapabilityFailure::UacDenied,CapabilityFailure::HelperCrash,CapabilityFailure::Timeout,CapabilityFailure::MalformedProtocol,CapabilityFailure::InvalidPipeAcl,CapabilityFailure::InvalidPeerPid,CapabilityFailure::Io] { let mut e=evidence(); e.capability=Err(f); miss(e,MissReason::Capability(f)); } let mut e=evidence(); e.ntfs=false; miss(e,MissReason::NonNtfs); let mut e=evidence(); e.reparse_point=true; miss(e,MissReason::ReparsePoint); let mut e=evidence(); e.regular_file=false; miss(e,MissReason::NotRegularFile); let mut e=evidence(); e.freeze_handle_held=false; miss(e,MissReason::FreezeHandleUnavailable); }
    #[test] fn corrupt_truncated_unknown_duplicate_and_partial_manifests_are_rejected() { assert_eq!(parse_manifest(b"{"),Err(ManifestError::Json)); let mut m=manifest();m.schema=2;assert_eq!(parse_manifest(&serde_json::to_vec(&m).unwrap()),Err(ManifestError::Schema)); let mut m=manifest();m.asset_count=2;assert_eq!(parse_manifest(&serde_json::to_vec(&m).unwrap()),Err(ManifestError::AssetCount)); let mut m=manifest();m.assets.push(m.assets[0].clone());m.asset_count=2;assert_eq!(parse_manifest(&serde_json::to_vec(&m).unwrap()),Err(ManifestError::DuplicateAsset)); }
}
