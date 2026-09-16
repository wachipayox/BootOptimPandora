use super::{
    evaluate_hit, parse_manifest, validate_manifest, AssetUsnCacheRuntime, CacheManifest, CachedAsset,
    CapabilityFailure, HitEvidence,
};
use bridge::modal_action::AssetVerificationMode;
use rand::{rngs::OsRng, RngCore};
use sha1::{Digest, Sha1};
use std::{
    collections::{HashMap, HashSet},
    ffi::{c_void, OsStr},
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    mem::zeroed,
    os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle},
    path::{Path, PathBuf},
    ptr::{null, null_mut},
    sync::{
        atomic::{AtomicBool, Ordering as AtomicOrdering},
        Mutex,
    },
};

type Handle = *mut c_void;
const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
const FILE_SHARE_READ: u32 = 0x1;
const FILE_SHARE_WRITE: u32 = 0x2;
const FILE_SHARE_DELETE: u32 = 0x4;
const FILE_TRAVERSE: u32 = 0x20;
const FILE_READ_ATTRIBUTES: u32 = 0x80;
const OPEN_EXISTING: u32 = 3;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
const VOLUME_NAME_GUID: u32 = 0x1;
const FSCTL_QUERY_USN_JOURNAL: u32 = 0x0009_00f4;
const FSCTL_READ_FILE_USN_DATA: u32 = 0x0009_00eb;
const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
const MOVEFILE_WRITE_THROUGH: u32 = 0x8;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ByHandleFileInformation {
    file_attributes: u32,
    creation_time_low: u32,
    creation_time_high: u32,
    last_access_time_low: u32,
    last_access_time_high: u32,
    last_write_time_low: u32,
    last_write_time_high: u32,
    volume_serial_number: u32,
    file_size_high: u32,
    file_size_low: u32,
    number_of_links: u32,
    file_index_high: u32,
    file_index_low: u32,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        security: *const c_void,
        creation: u32,
        flags: u32,
        template: Handle,
    ) -> Handle;
    fn CloseHandle(handle: Handle) -> i32;
    fn GetFileInformationByHandle(handle: Handle, info: *mut ByHandleFileInformation) -> i32;
    fn GetVolumeInformationByHandleW(
        handle: Handle,
        volume_name: *mut u16,
        volume_name_len: u32,
        serial: *mut u32,
        max_component: *mut u32,
        flags: *mut u32,
        fs_name: *mut u16,
        fs_name_len: u32,
    ) -> i32;
    fn GetFinalPathNameByHandleW(handle: Handle, path: *mut u16, len: u32, flags: u32) -> u32;
    fn DeviceIoControl(
        handle: Handle,
        code: u32,
        in_buffer: *const c_void,
        in_len: u32,
        out_buffer: *mut c_void,
        out_len: u32,
        returned: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
    fn MoveFileExW(existing: *const u16, new_name: *const u16, flags: u32) -> i32;
}

struct OwnedHandle(Handle);

unsafe impl Send for OwnedHandle {}

impl OwnedHandle {
    fn new(value: Handle) -> Option<Self> {
        (!value.is_null() && value != INVALID_HANDLE_VALUE).then_some(Self(value))
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FileIdentity {
    volume_guid: String,
    volume_serial: u64,
    file_id: [u8; 16],
    attributes: u32,
}

#[derive(Clone, Copy, Debug)]
struct JournalReply {
    volume_serial: u64,
    journal_id: u64,
    first_usn: i64,
    lowest_valid_usn: i64,
    next_usn: i64,
    file_id: [u8; 16],
    file_usn: i64,
}

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
    volume: Mutex<OwnedHandle>,
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

        let root_identity = directory_identity(&assets_root).ok_or(CapabilityFailure::Io)?;
        let volume = open_volume(&root_identity.volume_guid)?;
        if volume_serial(volume.0) != Some(root_identity.volume_serial) {
            return Err(CapabilityFailure::Io);
        }
        let initial = query_journal_handle(volume.0, root_identity.volume_serial)?;
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
        let Some((mut file, before)) = protected_file(path) else {
            return crate::asset_probe_context::hash_path_if_active(path, expected_hash)
                .unwrap_or_else(|| crate::fs::check_sha1_hash(path, expected_hash).unwrap_or(false));
        };

        if before.volume_guid != self.volume_guid || before.volume_serial != self.volume_serial {
            return hash_file(&mut file, expected_hash).unwrap_or(false);
        }

        if let Some(manifest) = &self.cached {
            if let Some(cached) = manifest.assets.iter().find(|asset| asset.expected_sha1 == expected_sha1) {
                if let Ok(current) = self.query_file(&file, before.file_id) {
                    if current.file_id == before.file_id
                        && current.volume_serial == self.volume_serial
                        && current.journal_id == self.initial_journal_id
                    {
                        let after = identity_from_handle(file.as_raw_handle().cast()).filter(|value| *value == before);
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
                            volume_serial: current.volume_serial,
                            journal_id: current.journal_id,
                            current_first_usn: current.first_usn,
                            current_lowest_valid_usn: current.lowest_valid_usn,
                            current_next_usn: current.next_usn,
                            current_file_id: &current_id,
                            current_file_usn: current.file_usn,
                            handle_identity_unchanged: after.is_some(),
                        };
                        let decision = evaluate_hit(manifest, cached, &evidence);
                        if runtime.can_skip_sha1(mode, decision) {
                            self.record_snapshot(expected_sha1, current.file_id, current.file_usn);
                            return true;
                        }
                    }
                }
            }
        }

        let valid = hash_file(&mut file, expected_hash).unwrap_or(false);
        if valid {
            if let Ok(current) = self.query_file(&file, before.file_id) {
                if current.file_id == before.file_id
                    && current.volume_serial == self.volume_serial
                    && current.journal_id == self.initial_journal_id
                    && valid_journal_reply(&current)
                    && current.file_usn >= 0
                    && identity_from_handle(file.as_raw_handle().cast()).is_some_and(|value| value == before)
                {
                    self.record_snapshot(expected_sha1, current.file_id, current.file_usn);
                }
            }
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
        let _ = atomic_publish(&self.manifest_path, &bytes);
    }

    fn baseline_one(&self, path: &Path, expected_sha1: &str, expected_hash: [u8; 20]) -> bool {
        if self.capability_failed.load(AtomicOrdering::Acquire) {
            return false;
        }
        let Some((mut file, before)) = protected_file(path) else {
            return false;
        };
        if before.volume_guid != self.volume_guid || before.volume_serial != self.volume_serial {
            return false;
        }
        if hash_file(&mut file, expected_hash) != Some(true) {
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

    fn query_file(&self, file: &File, expected_file_id: [u8; 16]) -> Result<JournalReply, CapabilityFailure> {
        self.with_capability(|volume| {
            let before = query_journal_handle(volume, self.volume_serial)?;
            let (file_id, file_usn) = file_usn_from_handle(file.as_raw_handle().cast())?;
            let after = query_journal_handle(volume, self.volume_serial)?;
            if before.journal_id != after.journal_id
                || before.journal_id != self.initial_journal_id
                || after.next_usn < before.next_usn
                || file_id != expected_file_id
            {
                return Err(CapabilityFailure::Io);
            }
            Ok(JournalReply {
                file_id,
                file_usn,
                ..after
            })
        })
    }

    fn query_journal(&self) -> Result<JournalReply, CapabilityFailure> {
        self.with_capability(|volume| query_journal_handle(volume, self.volume_serial))
    }

    fn with_capability<T>(
        &self,
        query: impl FnOnce(Handle) -> Result<T, CapabilityFailure>,
    ) -> Result<T, CapabilityFailure> {
        if self.capability_failed.load(AtomicOrdering::Acquire) {
            return Err(CapabilityFailure::Io);
        }
        let result = self.volume.lock().map_err(|_| CapabilityFailure::Io).and_then(|volume| query(volume.0));
        if result.is_err() {
            self.capability_failed.store(true, AtomicOrdering::Release);
        }
        result
    }
}

fn valid_journal_reply(reply: &JournalReply) -> bool {
    reply.first_usn >= 0
        && reply.lowest_valid_usn >= 0
        && reply.next_usn >= 0
        && reply.next_usn >= reply.first_usn
        && reply.next_usn >= reply.lowest_valid_usn
}

fn protected_file(path: &Path) -> Option<(File, FileIdentity)> {
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .ok()?;
    let identity = identity_from_handle(file.as_raw_handle().cast())?;
    if identity.attributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY) != 0 {
        return None;
    }
    Some((file, identity))
}

fn directory_identity(path: &Path) -> Option<FileIdentity> {
    let wide = wide(path.as_os_str());
    let handle = OwnedHandle::new(unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        )
    })?;
    let identity = identity_from_handle(handle.0)?;
    if identity.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 || identity.attributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
        return None;
    }
    Some(identity)
}

fn basic_file_information(handle: Handle) -> Option<ByHandleFileInformation> {
    let mut info: ByHandleFileInformation = unsafe { zeroed() };
    if unsafe { GetFileInformationByHandle(handle, &mut info) } == 0 {
        None
    } else {
        Some(info)
    }
}

fn volume_serial(handle: Handle) -> Option<u64> {
    let mut serial = 0u32;
    let mut max_component = 0u32;
    let mut flags = 0u32;
    let mut fs_name = [0u16; 16];
    if unsafe {
        GetVolumeInformationByHandleW(
            handle,
            null_mut(),
            0,
            &mut serial,
            &mut max_component,
            &mut flags,
            fs_name.as_mut_ptr(),
            fs_name.len() as u32,
        )
    } == 0
    {
        return None;
    }
    let len = fs_name.iter().position(|value| *value == 0).unwrap_or(fs_name.len());
    (String::from_utf16_lossy(&fs_name[..len]) == "NTFS").then_some(serial as u64)
}

fn identity_from_handle(handle: Handle) -> Option<FileIdentity> {
    let info = basic_file_information(handle)?;
    let serial = volume_serial(handle)?;
    if info.volume_serial_number as u64 != serial {
        return None;
    }

    let mut final_path = [0u16; 32768];
    let written = unsafe {
        GetFinalPathNameByHandleW(handle, final_path.as_mut_ptr(), final_path.len() as u32, VOLUME_NAME_GUID)
    } as usize;
    if written == 0 || written >= final_path.len() {
        return None;
    }
    let path = String::from_utf16_lossy(&final_path[..written]);
    let end = path.find("}\\")? + 2;
    let volume_guid = path.get(..end)?.to_owned();
    if !super::is_volume_guid(&volume_guid) {
        return None;
    }

    let raw_id = ((info.file_index_high as u64) << 32) | info.file_index_low as u64;
    let mut file_id = [0u8; 16];
    file_id[..8].copy_from_slice(&raw_id.to_le_bytes());
    Some(FileIdentity {
        volume_guid,
        volume_serial: serial,
        file_id,
        attributes: info.file_attributes,
    })
}

fn open_volume(volume_guid: &str) -> Result<OwnedHandle, CapabilityFailure> {
    let path = volume_guid.strip_suffix('\\').ok_or(CapabilityFailure::Io)?;
    let wide = wide(OsStr::new(path));
    OwnedHandle::new(unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_TRAVERSE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null(),
            OPEN_EXISTING,
            0,
            null_mut(),
        )
    })
    .ok_or(CapabilityFailure::Io)
}

fn query_journal_handle(handle: Handle, volume_serial: u64) -> Result<JournalReply, CapabilityFailure> {
    let mut output = [0u8; 128];
    let mut returned = 0u32;
    if unsafe {
        DeviceIoControl(
            handle,
            FSCTL_QUERY_USN_JOURNAL,
            null(),
            0,
            output.as_mut_ptr().cast(),
            output.len() as u32,
            &mut returned,
            null_mut(),
        )
    } == 0
        || returned < 32
    {
        return Err(CapabilityFailure::Io);
    }
    let reply = JournalReply {
        volume_serial,
        journal_id: u64::from_le_bytes(output[0..8].try_into().map_err(|_| CapabilityFailure::Io)?),
        first_usn: i64::from_le_bytes(output[8..16].try_into().map_err(|_| CapabilityFailure::Io)?),
        next_usn: i64::from_le_bytes(output[16..24].try_into().map_err(|_| CapabilityFailure::Io)?),
        lowest_valid_usn: i64::from_le_bytes(output[24..32].try_into().map_err(|_| CapabilityFailure::Io)?),
        file_id: [0; 16],
        file_usn: -1,
    };
    if reply.journal_id == 0 || !valid_journal_reply(&reply) {
        return Err(CapabilityFailure::Io);
    }
    Ok(reply)
}

fn file_usn_from_handle(handle: Handle) -> Result<([u8; 16], i64), CapabilityFailure> {
    let mut output = [0u8; 256];
    let mut returned = 0u32;
    if unsafe {
        DeviceIoControl(
            handle,
            FSCTL_READ_FILE_USN_DATA,
            null(),
            0,
            output.as_mut_ptr().cast(),
            output.len() as u32,
            &mut returned,
            null_mut(),
        )
    } == 0
        || returned < 32
        || u16::from_le_bytes([output[4], output[5]]) != 2
    {
        return Err(CapabilityFailure::Io);
    }
    let mut file_id = [0u8; 16];
    file_id[..8].copy_from_slice(&output[8..16]);
    let file_usn = i64::from_le_bytes(output[24..32].try_into().map_err(|_| CapabilityFailure::Io)?);
    if file_usn < 0 {
        return Err(CapabilityFailure::Io);
    }
    Ok((file_id, file_usn))
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

fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

fn atomic_publish(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut random = [0u8; 16];
    OsRng.fill_bytes(&mut random);
    let temp = path.with_extension(format!("{}.tmp", hex::encode(random)));
    let mut file = OpenOptions::new().write(true).create_new(true).open(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);

    let source = wide(temp.as_os_str());
    let destination = wide(path.as_os_str());
    if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH) }
        == 0
    {
        let error = std::io::Error::last_os_error();
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod windows_tests {
    use super::*;

    fn temp_path(label: &str) -> PathBuf {
        let mut random = [0u8; 8];
        OsRng.fill_bytes(&mut random);
        std::env::temp_dir().join(format!(
            "bootoptim-usn-direct-{label}-{}-{}",
            std::process::id(),
            hex::encode(random)
        ))
    }

    #[test]
    fn protected_handle_blocks_conflicting_writer_and_delete() {
        let dir = temp_path("toctou");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("asset");
        std::fs::write(&path, b"original").unwrap();
        let Some((file, identity)) = protected_file(&path) else {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        };

        assert!(OpenOptions::new().write(true).open(&path).is_err());
        assert!(std::fs::remove_file(&path).is_err());
        assert!(identity_from_handle(file.as_raw_handle().cast()).is_some_and(|value| value == identity));

        drop(file);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn direct_volume_and_file_queries_need_no_helper_process() {
        let dir = temp_path("direct");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("asset");
        std::fs::write(&path, b"abcdefgh").unwrap();
        let Some((file, identity)) = protected_file(&path) else {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        };
        let Ok(volume) = open_volume(&identity.volume_guid) else {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        };
        assert_eq!(volume_serial(volume.0), Some(identity.volume_serial));
        let journal = query_journal_handle(volume.0, identity.volume_serial).unwrap();
        let (file_id, file_usn) = file_usn_from_handle(file.as_raw_handle().cast()).unwrap();
        assert_eq!(file_id, identity.file_id);
        assert!(journal.journal_id != 0);
        assert!(file_usn >= 0);
        drop(file);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn same_size_mutation_with_restored_mtime_changes_real_ntfs_usn() {
        let dir = temp_path("same-size");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("asset");
        std::fs::write(&path, b"abcdefgh").unwrap();
        let Some((file, _)) = protected_file(&path) else {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        };
        let Ok((_, before)) = file_usn_from_handle(file.as_raw_handle().cast()) else {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        };
        drop(file);
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();

        let mut file = OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        file.write_all(b"ABCDEFGH").unwrap();
        file.sync_all().unwrap();
        file.set_times(std::fs::FileTimes::new().set_modified(modified)).unwrap();
        drop(file);

        let Some((file, _)) = protected_file(&path) else {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        };
        let Ok((_, after)) = file_usn_from_handle(file.as_raw_handle().cast()) else {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        };
        assert_ne!(before, after);
        drop(file);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_recreate_changes_real_ntfs_file_id() {
        let dir = temp_path("recreate");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("asset");
        std::fs::write(&path, b"same bytes").unwrap();
        let Some((file, first)) = protected_file(&path) else {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        };
        drop(file);
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, b"same bytes").unwrap();
        let Some((file, second)) = protected_file(&path) else {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        };
        drop(file);

        assert_eq!(first.volume_guid, second.volume_guid);
        assert_eq!(first.volume_serial, second.volume_serial);
        assert_ne!(first.file_id, second.file_id);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reparse_point_is_never_candidate_eligible() {
        let dir = temp_path("reparse");
        let _ = std::fs::create_dir_all(&dir);
        let target = dir.join("target");
        let link = dir.join("link");
        std::fs::write(&target, b"target").unwrap();
        if std::os::windows::fs::symlink_file(&target, &link).is_err() {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        assert!(protected_file(&link).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
