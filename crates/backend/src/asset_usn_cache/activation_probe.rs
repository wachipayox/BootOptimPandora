//! Compact opt-in diagnostic for the USN asset-cache activation boundary.
//!
//! This probe never authorizes reuse. It records only aggregate state and writes
//! one create-new JSON sidecar after the asset phase. Paths, asset hashes, FileIds,
//! USNs, account data and command lines are deliberately excluded.

use super::{
    AssetVerificationMode, CapabilityFailure, ASSET_USN_CACHE_ENV, ASSET_USN_HELPER_ENV,
    ASSET_USN_HELPER_SHA256_ENV,
};
use serde::Serialize;
use std::{
    ffi::OsString,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
};

pub(crate) const ENV_NAME: &str = "BOOTOPTIM_ASSET_USN_PROBE";
const SCHEMA: &str = "bootoptim.asset_usn_cache_probe.v1";

pub(crate) struct ActivationProbe {
    output_path: PathBuf,
    requested: bool,
    verification_mode: &'static str,
    canonical_objects_layout: bool,
    helper_path_configured: bool,
    helper_sha256_configured: bool,
    manifest_present_before: bool,
    session_attempted: AtomicBool,
    session_state: Mutex<&'static str>,
    decision_reason: Mutex<&'static str>,
    verified_reuse_files: AtomicU64,
    stock_sha1_files: AtomicU64,
    finished: AtomicBool,
}

#[derive(Serialize)]
struct Snapshot {
    schema: &'static str,
    requested: bool,
    verification_mode: &'static str,
    canonical_objects_layout: bool,
    helper_path_configured: bool,
    helper_sha256_configured: bool,
    session_attempted: bool,
    session_state: &'static str,
    decision_reason: &'static str,
    manifest_scope: &'static str,
    manifest_present_before: bool,
    manifest_present_after: bool,
    publication_state: &'static str,
    verified_reuse_files: u64,
    stock_sha1_files: u64,
}

impl ActivationProbe {
    pub(crate) fn for_launch(
        mode: AssetVerificationMode,
        canonical_objects_layout: bool,
        manifest_path: &Path,
    ) -> Option<Arc<Self>> {
        Self::from_path(
            configured_path(std::env::var_os(ENV_NAME)),
            std::env::var_os(ASSET_USN_CACHE_ENV).is_some_and(|value| value == "1"),
            mode,
            canonical_objects_layout,
            std::env::var_os(ASSET_USN_HELPER_ENV).is_some(),
            std::env::var_os(ASSET_USN_HELPER_SHA256_ENV).is_some(),
            manifest_path.is_file(),
        )
    }

    fn from_path(
        output_path: Option<PathBuf>,
        requested: bool,
        mode: AssetVerificationMode,
        canonical_objects_layout: bool,
        helper_path_configured: bool,
        helper_sha256_configured: bool,
        manifest_present_before: bool,
    ) -> Option<Arc<Self>> {
        Some(Arc::new(Self {
            output_path: output_path?,
            requested,
            verification_mode: mode_name(mode),
            canonical_objects_layout,
            helper_path_configured,
            helper_sha256_configured,
            manifest_present_before,
            session_attempted: AtomicBool::new(false),
            session_state: Mutex::new("not_attempted"),
            decision_reason: Mutex::new("not_evaluated"),
            verified_reuse_files: AtomicU64::new(0),
            stock_sha1_files: AtomicU64::new(0),
            finished: AtomicBool::new(false),
        }))
    }

    pub(crate) fn precondition_blocked(&self, reason: &'static str) {
        set_mutex(&self.session_state, "fallback");
        set_mutex(&self.decision_reason, reason);
    }

    pub(crate) fn session_attempted(&self) {
        self.session_attempted.store(true, Ordering::Relaxed);
    }

    pub(crate) fn session_ready(&self) {
        set_mutex(&self.session_state, "active");
        set_mutex(&self.decision_reason, "capability_authenticated");
    }

    pub(crate) fn session_failed(&self, failure: CapabilityFailure) {
        set_mutex(&self.session_state, "fallback");
        set_mutex(&self.decision_reason, capability_reason(failure));
    }

    pub(crate) fn record_verified_reuse(&self) {
        self.verified_reuse_files.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_stock_sha1(&self) {
        self.stock_sha1_files.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn finish(&self, manifest_path: &Path) {
        if self.finished.swap(true, Ordering::Relaxed) {
            return;
        }

        let manifest_present_after = manifest_path.is_file();
        let session_state = get_mutex(&self.session_state, "poisoned");
        let decision_reason = get_mutex(&self.decision_reason, "poisoned");
        let publication_state = if session_state != "active" {
            "not_attempted"
        } else if !self.manifest_present_before && manifest_present_after {
            "created"
        } else if self.manifest_present_before && manifest_present_after {
            "present_after_finish"
        } else {
            "absent_after_finish"
        };

        let snapshot = Snapshot {
            schema: SCHEMA,
            requested: self.requested,
            verification_mode: self.verification_mode,
            canonical_objects_layout: self.canonical_objects_layout,
            helper_path_configured: self.helper_path_configured,
            helper_sha256_configured: self.helper_sha256_configured,
            session_attempted: self.session_attempted.load(Ordering::Relaxed),
            session_state,
            decision_reason,
            manifest_scope: "launcher_assets_objects",
            manifest_present_before: self.manifest_present_before,
            manifest_present_after,
            publication_state,
            verified_reuse_files: self.verified_reuse_files.load(Ordering::Relaxed),
            stock_sha1_files: self.stock_sha1_files.load(Ordering::Relaxed),
        };

        let Ok(mut bytes) = serde_json::to_vec(&snapshot) else {
            return;
        };
        bytes.push(b'\n');
        if let Some(parent) = self.output_path.parent().filter(|path| !path.as_os_str().is_empty()) {
            let _ = std::fs::create_dir_all(parent);
        }
        let Ok(mut output) = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.output_path)
        else {
            return;
        };
        let _ = output.write_all(&bytes);
    }
}

fn configured_path(value: Option<OsString>) -> Option<PathBuf> {
    value.filter(|value| !value.is_empty()).map(PathBuf::from)
}

fn mode_name(mode: AssetVerificationMode) -> &'static str {
    match mode {
        AssetVerificationMode::Normal => "normal",
        AssetVerificationMode::FullVerification => "full_verification",
    }
}

fn capability_reason(failure: CapabilityFailure) -> &'static str {
    match failure {
        CapabilityFailure::HelperMissing => "helper_missing",
        CapabilityFailure::HelperIdentityMismatch => "helper_identity_mismatch",
        CapabilityFailure::UacDenied => "uac_denied",
        CapabilityFailure::HelperCrash => "helper_crash",
        CapabilityFailure::Timeout => "helper_timeout",
        CapabilityFailure::MalformedProtocol => "helper_protocol",
        CapabilityFailure::InvalidPipeAcl => "invalid_pipe_acl",
        CapabilityFailure::InvalidPeerPid => "invalid_peer_pid",
        CapabilityFailure::Io => "capability_io",
    }
}

fn set_mutex(slot: &Mutex<&'static str>, value: &'static str) {
    if let Ok(mut guard) = slot.lock() {
        *guard = value;
    }
}

fn get_mutex(slot: &Mutex<&'static str>, fallback: &'static str) -> &'static str {
    slot.lock().map(|guard| *guard).unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "bootoptim-usn-activation-probe-{label}-{}.json",
            std::process::id()
        ))
    }

    #[test]
    fn asset_usn_cache_probe_is_default_off() {
        assert!(ActivationProbe::from_path(
            None,
            true,
            AssetVerificationMode::Normal,
            true,
            true,
            true,
            false,
        )
        .is_none());
    }

    #[test]
    fn asset_usn_cache_probe_reports_capability_cut_and_stock_fallback() {
        let output = temp_path("capability");
        let manifest = temp_path("manifest");
        let _ = std::fs::remove_file(&output);
        let _ = std::fs::remove_file(&manifest);

        let probe = ActivationProbe::from_path(
            Some(output.clone()),
            true,
            AssetVerificationMode::Normal,
            true,
            false,
            false,
            false,
        )
        .unwrap();
        probe.session_attempted();
        probe.session_failed(CapabilityFailure::HelperMissing);
        probe.record_stock_sha1();
        probe.finish(&manifest);

        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
        assert_eq!(value["schema"], SCHEMA);
        assert_eq!(value["session_attempted"], true);
        assert_eq!(value["session_state"], "fallback");
        assert_eq!(value["decision_reason"], "helper_missing");
        assert_eq!(value["helper_path_configured"], false);
        assert_eq!(value["publication_state"], "not_attempted");
        assert_eq!(value["verified_reuse_files"], 0);
        assert_eq!(value["stock_sha1_files"], 1);

        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn asset_usn_cache_probe_reports_seed_publication_and_reuse_counts() {
        let output = temp_path("publication");
        let manifest = temp_path("published-manifest");
        let _ = std::fs::remove_file(&output);
        let _ = std::fs::remove_file(&manifest);

        let probe = ActivationProbe::from_path(
            Some(output.clone()),
            true,
            AssetVerificationMode::Normal,
            true,
            true,
            true,
            false,
        )
        .unwrap();
        probe.session_attempted();
        probe.session_ready();
        probe.record_stock_sha1();
        probe.record_verified_reuse();
        std::fs::write(&manifest, b"manifest").unwrap();
        probe.finish(&manifest);

        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
        assert_eq!(value["session_state"], "active");
        assert_eq!(value["publication_state"], "created");
        assert_eq!(value["verified_reuse_files"], 1);
        assert_eq!(value["stock_sha1_files"], 1);

        let _ = std::fs::remove_file(output);
        let _ = std::fs::remove_file(manifest);
    }
}
