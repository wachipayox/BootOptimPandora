use super::{
    AssetUsnCacheRuntime, CacheManifest, CachedAsset, CapabilityFailure, HitEvidence, evaluate_hit, parse_manifest,
    validate_manifest,
};
use bridge::modal_action::AssetVerificationMode;
use sha1::{Digest, Sha1};
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering as AtomicOrdering},
    },
};

#[path = "../../../../bootoptim-v0/ntfs_usn_direct.rs"]
mod ntfs_usn_direct;

use ntfs_usn_direct::{FileEvidence, JournalSnapshot, ProtectedFile, Volume};

pub(super) struct WindowsSession {
    asset_index_sha1: String,
    assets_root: PathBuf,
    manifest_path: PathBuf,
    volume_guid: String,
    volume_serial: u64,
    initial_journal_id: u64,
    expected_hashes: HashSet<String>,
    cached: Option<CacheManifest>,
    snapshots: Mutex<HashMap<String, CachedAsset>>,
    volume: Mutex<Volume>,
    capability_failed: AtomicBool,
}

impl WindowsSession {
    pub(super) fn begin(
        asset_index_sha1: &str,
        assets_root: std::sync::Arc<Path>,
        expected_hashes: Vec<String>,
    ) -> Result<Self, CapabilityFailure> {
        if !super::is_lower_hex(asset_index_sha1, 40) {
            return Err(CapabilityFailure::Io);
        }
        let expected_hashes: HashSet<String> = expected_hashes.into_iter().collect();
        if expected_hashes.iter().any(|hash| !super::is_lower_hex(hash, 40)) {
            return Err(CapabilityFailure::Io);
        }

        let root_identity = ntfs_usn_direct::directory_identity(&assets_root).map_err(|_| CapabilityFailure::Io)?;
        let volume = Volume::open(&root_identity).map_err(|_| CapabilityFailure::Io)?;
        let initial = volume.query_journal().map_err(|_| CapabilityFailure::Io)?;
        if initial.journal_id == 0 || !valid_journal_reply(&initial) {
            return Err(CapabilityFailure::Io);
        }

        let manifest_path = assets_root.join(".bootoptim-usn-assets-v1.json");
        let cached =
            std::fs::read(&manifest_path)
                .ok()
                .and_then(|bytes| parse_manifest(&bytes).ok())
                .filter(|manifest| {
                    manifest.asset_index_sha1 == asset_index_sha1
                        && manifest.volume_guid == root_identity.volume_guid
                        && manifest.volume_serial == root_identity.volume_serial
                        && manifest.assets.len() == expected_hashes.len()
                        && manifest.assets.iter().all(|asset| expected_hashes.contains(&asset.expected_sha1))
                });

        Ok(Self {
            asset_index_sha1: asset_index_sha1.to_owned(),
            assets_root: assets_root.to_path_buf(),
            manifest_path,
            volume_guid: root_identity.volume_guid,
            volume_serial: root_identity.volume_serial,
            initial_journal_id: initial.journal_id,
            expected_hashes,
            cached,
            snapshots: Mutex::new(HashMap::new()),
            volume: Mutex::new(volume),
            capability_failed: AtomicBool::new(false),
        })
    }

    pub(super) fn verify_existing(
        &self,
        runtime: &AssetUsnCacheRuntime,
        mode: AssetVerificationMode,
        path: &Path,
        expected_sha1: &str,
        expected_hash: [u8; 20],
    ) -> bool {
        let Ok(mut file) = ProtectedFile::open(path) else {
            return crate::asset_probe_context::hash_path_if_active(path, expected_hash)
                .unwrap_or_else(|| crate::fs::check_sha1_hash(path, expected_hash).unwrap_or(false));
        };
        let before = file.identity().clone();

        if before.volume_guid != self.volume_guid || before.volume_serial != self.volume_serial {
            return hash_file(file.file_mut(), expected_hash).unwrap_or(false);
        }

        if let Some(manifest) = &self.cached
            && let Some(cached) = manifest.assets.iter().find(|asset| asset.expected_sha1 == expected_sha1)
            && let Ok(current) = self.query_file(&file)
            && current.file_id == before.file_id
            && current.journal.volume_serial == self.volume_serial
            && current.journal.journal_id == self.initial_journal_id
        {
            let current_id = hex::encode(current.file_id);
            let evidence = HitEvidence {
                feature_requested: runtime.requested(),
                verification_mode: mode,
                capability: Ok(()),
                ntfs: true,
                reparse_point: false,
                regular_file: true,
                freeze_handle_held: true,
                asset_index_sha1: &self.asset_index_sha1,
                expected_asset_sha1: expected_sha1,
                volume_guid: &before.volume_guid,
                volume_serial: current.journal.volume_serial,
                journal_id: current.journal.journal_id,
                current_first_usn: current.journal.first_usn,
                current_lowest_valid_usn: current.journal.lowest_valid_usn,
                current_next_usn: current.journal.next_usn,
                current_file_id: &current_id,
                current_file_usn: current.file_usn,
                handle_identity_unchanged: file.identity_unchanged(),
            };
            let decision = evaluate_hit(manifest, cached, &evidence);
            if runtime.can_skip_sha1(mode, decision) {
                self.record_snapshot(expected_sha1, current.file_id, current.file_usn);
                return true;
            }
        }

        let valid = hash_file(file.file_mut(), expected_hash).unwrap_or(false);
        if valid
            && let Ok(current) = self.query_file(&file)
            && current.file_id == before.file_id
            && current.journal.volume_serial == self.volume_serial
            && current.journal.journal_id == self.initial_journal_id
            && valid_journal_reply(&current.journal)
            && current.file_usn >= 0
            && file.identity_unchanged()
        {
            self.record_snapshot(expected_sha1, current.file_id, current.file_usn);
        }
        valid
    }

    pub(super) fn finish(&self) {
        if self.capability_failed.load(AtomicOrdering::Acquire) {
            return;
        }

        for expected in &self.expected_hashes {
            if self.snapshots.lock().ok().is_some_and(|map| map.contains_key(expected)) {
                continue;
            }
            let mut hash = [0u8; 20];
            if hex::decode_to_slice(expected, &mut hash).is_err() {
                return;
            }
            let path = self.assets_root.join(&expected[..2]).join(expected);
            if !self.baseline_one(&path, expected, hash) {
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
        let _ = ntfs_usn_direct::atomic_replace(&self.manifest_path, &bytes);
    }

    fn baseline_one(&self, path: &Path, expected_sha1: &str, expected_hash: [u8; 20]) -> bool {
        if self.capability_failed.load(AtomicOrdering::Acquire) {
            return false;
        }
        let Ok(mut file) = ProtectedFile::open(path) else {
            return false;
        };
        let before = file.identity().clone();
        if before.volume_guid != self.volume_guid || before.volume_serial != self.volume_serial {
            return false;
        }
        if hash_file(file.file_mut(), expected_hash) != Some(true) {
            return false;
        }
        let Ok(current) = self.query_file(&file) else {
            return false;
        };
        if current.file_id != before.file_id
            || current.journal.volume_serial != self.volume_serial
            || current.journal.journal_id != self.initial_journal_id
            || !valid_journal_reply(&current.journal)
            || current.file_usn < 0
            || !file.identity_unchanged()
        {
            return false;
        }
        self.record_snapshot(expected_sha1, current.file_id, current.file_usn);
        true
    }

    fn record_snapshot(&self, expected: &str, file_id: [u8; 16], file_usn: i64) {
        if file_usn < 0 {
            return;
        }
        if let Ok(mut map) = self.snapshots.lock() {
            map.insert(
                expected.to_owned(),
                CachedAsset {
                    expected_sha1: expected.to_owned(),
                    file_id: hex::encode(file_id),
                    last_usn: file_usn,
                },
            );
        }
    }

    fn query_file(&self, file: &ProtectedFile) -> Result<FileEvidence, CapabilityFailure> {
        self.with_capability(|volume| {
            let evidence = volume.query_file(file, file.identity().file_id).map_err(|_| CapabilityFailure::Io)?;
            if evidence.journal.journal_id != self.initial_journal_id {
                return Err(CapabilityFailure::Io);
            }
            Ok(evidence)
        })
    }

    fn query_journal(&self) -> Result<JournalSnapshot, CapabilityFailure> {
        self.with_capability(|volume| volume.query_journal().map_err(|_| CapabilityFailure::Io))
    }

    fn with_capability<T>(
        &self,
        query: impl FnOnce(&Volume) -> Result<T, CapabilityFailure>,
    ) -> Result<T, CapabilityFailure> {
        if self.capability_failed.load(AtomicOrdering::Acquire) {
            return Err(CapabilityFailure::Io);
        }
        let result = self.volume.lock().map_err(|_| CapabilityFailure::Io).and_then(|volume| query(&volume));
        if result.is_err() {
            self.capability_failed.store(true, AtomicOrdering::Release);
        }
        result
    }
}

fn valid_journal_reply(reply: &JournalSnapshot) -> bool {
    reply.journal_id != 0
        && reply.first_usn >= 0
        && reply.lowest_valid_usn >= 0
        && reply.next_usn >= 0
        && reply.next_usn >= reply.first_usn
        && reply.next_usn >= reply.lowest_valid_usn
}

fn hash_file(file: &mut File, expected: [u8; 20]) -> Option<bool> {
    if let Some(result) = crate::asset_probe_context::hash_open_file_if_active(file, expected) {
        return Some(result);
    }
    file.seek(SeekFrom::Start(0)).ok()?;
    let mut hasher = Sha1::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Some(expected == *hasher.finalize())
}
