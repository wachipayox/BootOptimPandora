include!("windows_direct.rs");

use super::{MissReason, ReuseDecision};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FastVerifyResult {
    VerifiedReuse,
    FastRebootstrap,
    IndividualRepairVerification,
}

impl WindowsSession {
    /// Fast-policy verifier used by the active BootOptim asset path.
    ///
    /// It never hashes existing object content. A complete trusted manifest may
    /// authorize `VerifiedReuse` from FileId/USN evidence. Missing, malformed or
    /// journal-era-invalid metadata is rebuilt from the protected existing file
    /// without claiming historical content integrity. A per-file FileId/USN
    /// change under an otherwise usable baseline requests repair of that object.
    pub(super) fn verify_existing_fast(
        &self,
        runtime: &AssetUsnCacheRuntime,
        path: &Path,
        expected_sha1: &str,
    ) -> FastVerifyResult {
        if !runtime.requested() {
            let candidate = protected_file(path).is_some();
            // Disabled capability must not authorize persistent USN state. Reuse
            // the protected regular candidate only for this launch and latch
            // publication off for the whole session.
            self.capability_failed.store(true, AtomicOrdering::Release);
            return if candidate {
                FastVerifyResult::FastRebootstrap
            } else {
                FastVerifyResult::IndividualRepairVerification
            };
        }

        let Some((file, before)) = protected_file(path) else {
            return FastVerifyResult::IndividualRepairVerification;
        };

        if before.volume_guid != self.volume_guid || before.volume_serial != self.volume_serial {
            return FastVerifyResult::IndividualRepairVerification;
        }

        let current = match self.query_file(&file, before.file_id) {
            Ok(current) => current,
            Err(_) => {
                // Capability uncertainty must never become a bulk SHA-1 audit.
                // The protected regular candidate can be used for this launch,
                // but no manifest will be published because query failure latches
                // `capability_failed` for the session.
                return FastVerifyResult::FastRebootstrap;
            },
        };

        if current.file_id != before.file_id
            || current.volume_serial != self.volume_serial
            || current.journal_id != self.initial_journal_id
            || !valid_journal_reply(&current)
            || current.file_usn < 0
        {
            return FastVerifyResult::FastRebootstrap;
        }

        if !identity_from_handle(file.as_raw_handle().cast()).is_some_and(|value| value == before) {
            return FastVerifyResult::IndividualRepairVerification;
        }

        if let Some(manifest) = &self.cached {
            if let Some(cached) = manifest.assets.iter().find(|asset| asset.expected_sha1 == expected_sha1) {
                let current_id = hex::encode(current.file_id);
                let evidence = HitEvidence {
                    feature_requested: runtime.requested(),
                    // `FullVerification` historically meant a bulk content pass for
                    // CLI/legacy/unknown launches. The fast policy deliberately does
                    // not expose that path: all launch authorities use the same USN
                    // evidence when the cache experiment is requested.
                    verification_mode: AssetVerificationMode::Normal,
                    capability: Ok(()),
                    ntfs: true,
                    reparse_point: false,
                    regular_file: true,
                    freeze_handle_held: true,
                    asset_index_sha1: &self.asset_index_sha1,
                    expected_asset_sha1: expected_sha1,
                    volume_guid: &before.volume_guid,
                    volume_serial: current.volume_serial,
                    journal_id: current.journal_id,
                    current_first_usn: current.first_usn,
                    current_lowest_valid_usn: current.lowest_valid_usn,
                    current_next_usn: current.next_usn,
                    current_file_id: &current_id,
                    current_file_usn: current.file_usn,
                    handle_identity_unchanged: true,
                };

                match evaluate_hit(manifest, cached, &evidence) {
                    ReuseDecision::VerifiedReuse => {
                        self.record_snapshot(expected_sha1, current.file_id, current.file_usn);
                        return FastVerifyResult::VerifiedReuse;
                    },
                    ReuseDecision::FullSha1(
                        MissReason::JournalIdMismatch
                        | MissReason::JournalRegression
                        | MissReason::JournalDiscontinuity
                        | MissReason::InvalidUsn,
                    ) => {
                        // Loss of continuity is equivalent to losing the manifest:
                        // rebuild a fresh USN baseline without reading every byte.
                        self.record_snapshot(expected_sha1, current.file_id, current.file_usn);
                        return FastVerifyResult::FastRebootstrap;
                    },
                    ReuseDecision::FullSha1(_) => {
                        // Under a complete same-era manifest, a changed FileId/USN
                        // is a per-object invalidation. The caller will download/
                        // repair this object and verify that downloaded body by SHA-1.
                        return FastVerifyResult::IndividualRepairVerification;
                    },
                }
            }
        }

        // Missing/malformed/identity-incompatible manifest: explicitly adopt the
        // current protected object as a fresh USN baseline without content SHA-1.
        // This intentionally cannot detect corruption that predates rebootstrap.
        self.record_snapshot(expected_sha1, current.file_id, current.file_usn);
        FastVerifyResult::FastRebootstrap
    }

    /// Publishes only metadata. Missing snapshots are acquired from the final
    /// object after the normal download/repair path has already verified that
    /// individual body against Mojang's SHA-1. No bulk content pass exists here.
    pub(super) fn finish_fast(&self) {
        if self.capability_failed.load(AtomicOrdering::Acquire) {
            return;
        }

        for expected in &self.expected_hashes {
            if self.snapshots.lock().ok().is_some_and(|map| map.contains_key(expected)) {
                continue;
            }
            let path = self.assets_root.join(&expected[..2]).join(expected);
            if !self.baseline_metadata_only(&path, expected) {
                return;
            }
        }

        let Ok(final_journal) = self.query_journal() else {
            return;
        };
        if final_journal.journal_id != self.initial_journal_id
            || final_journal.volume_serial != self.volume_serial
            || !valid_journal_reply(&final_journal)
        {
            return;
        }

        let Ok(snapshots) = self.snapshots.lock() else {
            return;
        };
        if snapshots.len() != self.expected_hashes.len()
            || self.expected_hashes.iter().any(|hash| !snapshots.contains_key(hash))
        {
            return;
        }

        let mut assets: Vec<CachedAsset> = snapshots.values().cloned().collect();
        assets.sort_by(|left, right| left.expected_sha1.cmp(&right.expected_sha1));
        let manifest = CacheManifest {
            schema: 1,
            asset_index_sha1: self.asset_index_sha1.clone(),
            volume_guid: self.volume_guid.clone(),
            volume_serial: self.volume_serial,
            journal_id: final_journal.journal_id,
            snapshot_first_usn: final_journal.first_usn,
            snapshot_lowest_valid_usn: final_journal.lowest_valid_usn,
            snapshot_next_usn: final_journal.next_usn,
            asset_count: assets.len() as u32,
            assets,
        };
        if validate_manifest(&manifest).is_err() {
            return;
        }
        let Ok(bytes) = serde_json::to_vec(&manifest) else {
            return;
        };
        let _ = atomic_publish(&self.manifest_path, &bytes);
    }

    fn baseline_metadata_only(&self, path: &Path, expected_sha1: &str) -> bool {
        if self.capability_failed.load(AtomicOrdering::Acquire) {
            return false;
        }
        let Some((file, before)) = protected_file(path) else {
            return false;
        };
        if before.volume_guid != self.volume_guid || before.volume_serial != self.volume_serial {
            return false;
        }
        let Ok(current) = self.query_file(&file, before.file_id) else {
            return false;
        };
        if current.file_id != before.file_id
            || current.volume_serial != self.volume_serial
            || current.journal_id != self.initial_journal_id
            || !valid_journal_reply(&current)
            || current.file_usn < 0
        {
            return false;
        }
        if !identity_from_handle(file.as_raw_handle().cast()).is_some_and(|value| value == before) {
            return false;
        }
        self.record_snapshot(expected_sha1, current.file_id, current.file_usn);
        true
    }
}

/// Last-resort fast policy when a USN session cannot be established at all.
/// This checks only that the exact candidate is a regular non-reparse object
/// while holding a write/delete-denying handle. It is deliberately *not* a
/// cryptographic or persistent reuse proof and therefore cannot publish cache
/// authority. It exists solely to avoid turning capability loss into a massive
/// SHA-1 audit.
pub(super) fn fast_untracked_candidate(path: &Path) -> bool {
    let Ok(file) = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
    else {
        return false;
    };
    let Some(info) = basic_file_information(file.as_raw_handle().cast()) else {
        return false;
    };
    info.file_attributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY) == 0
}

#[cfg(test)]
mod direct_integration_tests;
