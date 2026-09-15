use super::{
    evaluate_hit, parse_manifest, validate_manifest, AssetUsnCacheRuntime, CacheManifest, CachedAsset,
    CapabilityFailure, HitEvidence, ReuseDecision, ASSET_USN_HELPER_ENV, ASSET_USN_HELPER_SHA256_ENV,
};
use crate::usn_protocol::{self, Handshake, Request, RequestKind, ResponseStatus};
use bridge::modal_action::AssetVerificationMode;
use rand::{rngs::OsRng, RngCore};
use sha1::{Digest, Sha1};
use sha2::Sha256;
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
    time::{Duration, Instant},
};

type Handle = *mut c_void;
const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
const FILE_SHARE_READ: u32 = 0x1;
const FILE_SHARE_WRITE: u32 = 0x2;
const FILE_SHARE_DELETE: u32 = 0x4;
const FILE_READ_ATTRIBUTES: u32 = 0x80;
const OPEN_EXISTING: u32 = 3;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
const PIPE_ACCESS_DUPLEX: u32 = 0x3;
const FILE_FLAG_FIRST_PIPE_INSTANCE: u32 = 0x0008_0000;
const PIPE_NOWAIT: u32 = 0x1;
const PIPE_REJECT_REMOTE_CLIENTS: u32 = 0x8;
const ERROR_PIPE_CONNECTED: i32 = 535;
const ERROR_PIPE_LISTENING: i32 = 536;
const ERROR_NO_DATA: i32 = 232;
const ERROR_CANCELLED: i32 = 1223;
const TOKEN_QUERY: u32 = 0x8;
const TOKEN_USER_CLASS: u32 = 1;
const SDDL_REVISION_1: u32 = 1;
const SEE_MASK_NOCLOSEPROCESS: u32 = 0x40;
const SW_HIDE: i32 = 0;
const STILL_ACTIVE: u32 = 259;
const VOLUME_NAME_GUID: u32 = 0x1;
const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(45);
const MESSAGE_TIMEOUT: Duration = Duration::from_secs(5);
const PINNED_HELPER_SHA256: Option<&str> = option_env!("BOOTOPTIM_ASSET_USN_HELPER_SHA256_PIN");

#[cfg(test)]
const FSCTL_READ_FILE_USN_DATA: u32 = 0x0009_00eb;

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

#[repr(C)]
struct SecurityAttributes {
    length: u32,
    security_descriptor: *mut c_void,
    inherit_handle: i32,
}

#[repr(C)]
struct SidAndAttributes {
    sid: *mut c_void,
    attributes: u32,
}

#[repr(C)]
struct TokenUser {
    user: SidAndAttributes,
}

#[repr(C)]
struct ShellExecuteInfoW {
    size: u32,
    mask: u32,
    hwnd: Handle,
    verb: *const u16,
    file: *const u16,
    parameters: *const u16,
    directory: *const u16,
    show: i32,
    instance: Handle,
    id_list: *mut c_void,
    class: *const u16,
    class_key: Handle,
    hot_key: u32,
    icon_or_monitor: Handle,
    process: Handle,
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
    fn GetCurrentProcess() -> Handle;
    fn GetCurrentProcessId() -> u32;
    fn GetProcessId(process: Handle) -> u32;
    fn GetExitCodeProcess(process: Handle, exit_code: *mut u32) -> i32;
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
    fn CreateNamedPipeW(
        name: *const u16,
        open_mode: u32,
        pipe_mode: u32,
        max_instances: u32,
        out_size: u32,
        in_size: u32,
        timeout: u32,
        security: *const SecurityAttributes,
    ) -> Handle;
    fn ConnectNamedPipe(pipe: Handle, overlapped: *mut c_void) -> i32;
    fn GetNamedPipeClientProcessId(pipe: Handle, pid: *mut u32) -> i32;
    fn PeekNamedPipe(
        pipe: Handle,
        buffer: *mut c_void,
        len: u32,
        read: *mut u32,
        available: *mut u32,
        left: *mut u32,
    ) -> i32;
    fn ReadFile(
        handle: Handle,
        buffer: *mut c_void,
        len: u32,
        read: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
    fn WriteFile(
        handle: Handle,
        buffer: *const c_void,
        len: u32,
        written: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
    fn LocalFree(memory: *mut c_void) -> *mut c_void;
    fn MoveFileExW(existing: *const u16, new_name: *const u16, flags: u32) -> i32;
    #[cfg(test)]
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
}

#[link(name = "advapi32")]
unsafe extern "system" {
    fn OpenProcessToken(process: Handle, access: u32, token: *mut Handle) -> i32;
    fn GetTokenInformation(
        token: Handle,
        class: u32,
        info: *mut c_void,
        info_len: u32,
        returned: *mut u32,
    ) -> i32;
    fn ConvertSidToStringSidW(sid: *mut c_void, string_sid: *mut *mut u16) -> i32;
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
        sddl: *const u16,
        revision: u32,
        descriptor: *mut *mut c_void,
        size: *mut u32,
    ) -> i32;
}

#[link(name = "shell32")]
unsafe extern "system" {
    fn ShellExecuteExW(info: *mut ShellExecuteInfoW) -> i32;
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
    /// NTFS v1 uses the documented 64-bit file index. The fixed-size protocol
    /// carries 16 bytes so the representation cannot become a path channel;
    /// the high 64 bits are required to remain zero for this NTFS-only v1.
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

struct HelperPipe {
    pipe: OwnedHandle,
    process: OwnedHandle,
    nonce: [u8; usn_protocol::NONCE_LEN],
    volume_guid: [u8; usn_protocol::VOLUME_GUID_LEN],
}

impl HelperPipe {
    fn query(
        &mut self,
        kind: RequestKind,
        file_id: [u8; 16],
    ) -> Result<JournalReply, CapabilityFailure> {
        let mut exit = 0u32;
        if unsafe { GetExitCodeProcess(self.process.0, &mut exit) } == 0 || exit != STILL_ACTIVE {
            return Err(CapabilityFailure::HelperCrash);
        }

        let request = usn_protocol::encode_request(Request {
            kind,
            nonce: self.nonce,
            volume_guid: self.volume_guid,
            file_id,
        });
        if !write_exact(self.pipe.0, &request) {
            return Err(CapabilityFailure::HelperCrash);
        }

        let mut bytes = [0u8; usn_protocol::RESPONSE_LEN];
        if !read_exact_timeout(self.pipe.0, &mut bytes, Instant::now() + MESSAGE_TIMEOUT) {
            return Err(CapabilityFailure::Timeout);
        }
        let response = usn_protocol::decode_response(&bytes)
            .ok_or(CapabilityFailure::MalformedProtocol)?;
        if response.nonce != self.nonce {
            return Err(CapabilityFailure::MalformedProtocol);
        }

        match response.status {
            ResponseStatus::Ok => Ok(JournalReply {
                volume_serial: response.volume_serial,
                journal_id: response.journal_id,
                first_usn: response.first_usn,
                lowest_valid_usn: response.lowest_valid_usn,
                next_usn: response.next_usn,
                file_id: response.file_id,
                file_usn: response.file_usn,
            }),
            _ => Err(CapabilityFailure::Io),
        }
    }
}

impl Drop for HelperPipe {
    fn drop(&mut self) {
        let shutdown = usn_protocol::encode_request(Request {
            kind: RequestKind::Shutdown,
            nonce: self.nonce,
            volume_guid: self.volume_guid,
            file_id: [0; 16],
        });
        // Best effort only. Closing the server handle wakes or breaks the client;
        // waiting for FlushFileBuffers here could block stock fallback on a hung helper.
        let _ = write_exact(self.pipe.0, &shutdown);
    }
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
    helper: Mutex<HelperPipe>,
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
        let mut helper = launch_helper(&root_identity.volume_guid)?;
        let initial = helper.query(RequestKind::Journal, [0; 16])?;
        if initial.volume_serial != root_identity.volume_serial
            || initial.journal_id == 0
            || !valid_journal_reply(&initial)
        {
            return Err(CapabilityFailure::Io);
        }

        let manifest_path = assets_root.join(".bootoptim-usn-assets-v1.json");
        let cached = std::fs::read(&manifest_path)
            .ok()
            .and_then(|bytes| parse_manifest(&bytes).ok())
            .filter(|manifest| {
                manifest.asset_index_sha1 == asset_index_sha1
                    && manifest.volume_guid == root_identity.volume_guid
                    && manifest.volume_serial == root_identity.volume_serial
                    && manifest.assets.len() == expected_hashes.len()
                    && manifest
                        .assets
                        .iter()
                        .all(|asset| expected_hashes.contains(&asset.expected_sha1))
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
            helper: Mutex::new(helper),
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

        if let Some(manifest) = &self.cached
            && let Some(cached) = manifest
                .assets
                .iter()
                .find(|asset| asset.expected_sha1 == expected_sha1)
            && let Ok(current) = self.query_file(before.file_id)
            && current.file_id == before.file_id
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

        let valid = hash_file(&mut file, expected_hash).unwrap_or(false);
        if valid
            && let Ok(current) = self.query_file(before.file_id)
            && current.file_id == before.file_id
            && current.volume_serial == self.volume_serial
            && current.journal_id == self.initial_journal_id
            && valid_journal_reply(&current)
            && current.file_usn >= 0
            && identity_from_handle(file.as_raw_handle().cast()).is_some_and(|value| value == before)
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
            if self
                .snapshots
                .lock()
                .ok()
                .is_some_and(|map| map.contains_key(expected))
            {
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
            || self
                .expected_hashes
                .iter()
                .any(|hash| !snapshots.contains_key(hash))
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
        let Ok(current) = self.query_file(before.file_id) else {
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

    fn query_file(&self, file_id: [u8; 16]) -> Result<JournalReply, CapabilityFailure> {
        self.query(RequestKind::File, file_id)
    }

    fn query_journal(&self) -> Result<JournalReply, CapabilityFailure> {
        self.query(RequestKind::Journal, [0; 16])
    }

    fn query(
        &self,
        kind: RequestKind,
        file_id: [u8; 16],
    ) -> Result<JournalReply, CapabilityFailure> {
        if self.capability_failed.load(AtomicOrdering::Acquire) {
            return Err(CapabilityFailure::Io);
        }
        let result = self
            .helper
            .lock()
            .map_err(|_| CapabilityFailure::Io)?
            .query(kind, file_id);
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
    if identity.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || identity.attributes & FILE_ATTRIBUTE_DIRECTORY == 0
    {
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

fn identity_from_handle(handle: Handle) -> Option<FileIdentity> {
    let info = basic_file_information(handle)?;
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
    if info.volume_serial_number != serial {
        return None;
    }
    let len = fs_name.iter().position(|value| *value == 0).unwrap_or(fs_name.len());
    if String::from_utf16_lossy(&fs_name[..len]) != "NTFS" {
        return None;
    }

    let mut final_path = [0u16; 32768];
    let written = unsafe {
        GetFinalPathNameByHandleW(
            handle,
            final_path.as_mut_ptr(),
            final_path.len() as u32,
            VOLUME_NAME_GUID,
        )
    } as usize;
    if written == 0 || written >= final_path.len() {
        return None;
    }
    let path = String::from_utf16_lossy(&final_path[..written]);
    let volume_guid = path.get(..usn_protocol::VOLUME_GUID_LEN)?.to_owned();
    usn_protocol::volume_guid_bytes(&volume_guid)?;

    let raw_id = ((info.file_index_high as u64) << 32) | info.file_index_low as u64;
    let mut file_id = [0u8; 16];
    file_id[..8].copy_from_slice(&raw_id.to_le_bytes());
    Some(FileIdentity {
        volume_guid,
        volume_serial: serial as u64,
        file_id,
        attributes: info.file_attributes,
    })
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

fn valid_helper_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn launch_helper(volume_guid: &str) -> Result<HelperPipe, CapabilityFailure> {
    let helper_path = std::env::var_os(ASSET_USN_HELPER_ENV)
        .map(PathBuf::from)
        .ok_or(CapabilityFailure::HelperMissing)?
        .canonicalize()
        .map_err(|_| CapabilityFailure::HelperMissing)?;
    let expected_digest = std::env::var(ASSET_USN_HELPER_SHA256_ENV)
        .map_err(|_| CapabilityFailure::HelperIdentityMismatch)?;
    let pinned_digest = PINNED_HELPER_SHA256
        .filter(|value| valid_helper_digest(value))
        .ok_or(CapabilityFailure::HelperIdentityMismatch)?;
    if !valid_helper_digest(&expected_digest) || expected_digest != pinned_digest {
        return Err(CapabilityFailure::HelperIdentityMismatch);
    }

    // Keep a read handle open with write/delete sharing denied from hashing until
    // after the elevated process is authenticated. Any pathname substitution must
    // still match the SHA-256 embedded in this Pandora build.
    let mut guard = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(&helper_path)
        .map_err(|_| CapabilityFailure::HelperMissing)?;
    let info = basic_file_information(guard.as_raw_handle().cast())
        .ok_or(CapabilityFailure::HelperIdentityMismatch)?;
    if info.file_attributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY) != 0 {
        return Err(CapabilityFailure::HelperIdentityMismatch);
    }

    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = guard
            .read(&mut buffer)
            .map_err(|_| CapabilityFailure::HelperIdentityMismatch)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    if hex::encode(hasher.finalize()) != pinned_digest {
        return Err(CapabilityFailure::HelperIdentityMismatch);
    }

    let mut nonce = [0u8; usn_protocol::NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);
    let nonce_hex = hex::encode(nonce);
    let pipe_name = format!(r"\\.\pipe\BootOptimPandora-USN-v1-{nonce_hex}");
    let descriptor = restrictive_security_descriptor()?;
    let pipe_wide = wide(OsStr::new(&pipe_name));
    let mut security = SecurityAttributes {
        length: std::mem::size_of::<SecurityAttributes>() as u32,
        security_descriptor: descriptor,
        inherit_handle: 0,
    };
    let pipe = OwnedHandle::new(unsafe {
        CreateNamedPipeW(
            pipe_wide.as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            4096,
            4096,
            MESSAGE_TIMEOUT.as_millis() as u32,
            &mut security,
        )
    });
    unsafe {
        LocalFree(descriptor);
    }
    let pipe = pipe.ok_or(CapabilityFailure::InvalidPipeAcl)?;

    let verb = wide(OsStr::new("runas"));
    let executable = wide(helper_path.as_os_str());
    let parameters = wide(OsStr::new(&format!(
        "--pipe {pipe_name} --nonce {nonce_hex} --server-pid {}",
        unsafe { GetCurrentProcessId() }
    )));
    let mut shell = ShellExecuteInfoW {
        size: std::mem::size_of::<ShellExecuteInfoW>() as u32,
        mask: SEE_MASK_NOCLOSEPROCESS,
        hwnd: null_mut(),
        verb: verb.as_ptr(),
        file: executable.as_ptr(),
        parameters: parameters.as_ptr(),
        directory: null(),
        show: SW_HIDE,
        instance: null_mut(),
        id_list: null_mut(),
        class: null(),
        class_key: null_mut(),
        hot_key: 0,
        icon_or_monitor: null_mut(),
        process: null_mut(),
    };
    if unsafe { ShellExecuteExW(&mut shell) } == 0 {
        return Err(if last_error() == ERROR_CANCELLED {
            CapabilityFailure::UacDenied
        } else {
            CapabilityFailure::HelperMissing
        });
    }

    let process = OwnedHandle::new(shell.process).ok_or(CapabilityFailure::HelperCrash)?;
    let helper_pid = unsafe { GetProcessId(process.0) };
    if helper_pid == 0 {
        return Err(CapabilityFailure::HelperCrash);
    }

    connect_pipe(pipe.0, Instant::now() + CONNECT_TIMEOUT)?;
    let mut client_pid = 0u32;
    if unsafe { GetNamedPipeClientProcessId(pipe.0, &mut client_pid) } == 0
        || client_pid != helper_pid
    {
        return Err(CapabilityFailure::InvalidPeerPid);
    }

    let hello = usn_protocol::encode_handshake(Handshake {
        nonce,
        pid: unsafe { GetCurrentProcessId() },
    });
    if !write_exact(pipe.0, &hello) {
        return Err(CapabilityFailure::HelperCrash);
    }
    let mut acknowledgement = [0u8; usn_protocol::HANDSHAKE_LEN];
    if !read_exact_timeout(
        pipe.0,
        &mut acknowledgement,
        Instant::now() + MESSAGE_TIMEOUT,
    ) {
        return Err(CapabilityFailure::Timeout);
    }
    let acknowledgement = usn_protocol::decode_handshake(&acknowledgement)
        .ok_or(CapabilityFailure::MalformedProtocol)?;
    if acknowledgement.nonce != nonce || acknowledgement.pid != helper_pid {
        return Err(CapabilityFailure::InvalidPeerPid);
    }

    drop(guard);
    Ok(HelperPipe {
        pipe,
        process,
        nonce,
        volume_guid: usn_protocol::volume_guid_bytes(volume_guid)
            .ok_or(CapabilityFailure::MalformedProtocol)?,
    })
}

fn restrictive_security_descriptor() -> Result<*mut c_void, CapabilityFailure> {
    let mut token = null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(CapabilityFailure::InvalidPipeAcl);
    }
    let token = OwnedHandle::new(token).ok_or(CapabilityFailure::InvalidPipeAcl)?;

    let mut needed = 0u32;
    unsafe {
        GetTokenInformation(token.0, TOKEN_USER_CLASS, null_mut(), 0, &mut needed);
    }
    if needed == 0 {
        return Err(CapabilityFailure::InvalidPipeAcl);
    }
    let words =
        (needed as usize + std::mem::size_of::<usize>() - 1) / std::mem::size_of::<usize>();
    let mut buffer = vec![0usize; words];
    if unsafe {
        GetTokenInformation(
            token.0,
            TOKEN_USER_CLASS,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(CapabilityFailure::InvalidPipeAcl);
    }

    let user = unsafe { &*(buffer.as_ptr().cast::<TokenUser>()) };
    let mut sid_string: *mut u16 = null_mut();
    if unsafe { ConvertSidToStringSidW(user.user.sid, &mut sid_string) } == 0
        || sid_string.is_null()
    {
        return Err(CapabilityFailure::InvalidPipeAcl);
    }
    let mut len = 0usize;
    unsafe {
        while *sid_string.add(len) != 0 {
            len += 1;
        }
    }
    let sid = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(sid_string, len) });
    unsafe {
        LocalFree(sid_string.cast());
    }

    let sddl = wide(OsStr::new(&format!(
        "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;{sid})"
    )));
    let mut descriptor = null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            null_mut(),
        )
    } == 0
        || descriptor.is_null()
    {
        return Err(CapabilityFailure::InvalidPipeAcl);
    }
    Ok(descriptor)
}

fn connect_pipe(pipe: Handle, deadline: Instant) -> Result<(), CapabilityFailure> {
    while Instant::now() < deadline {
        if unsafe { ConnectNamedPipe(pipe, null_mut()) } != 0 {
            return Ok(());
        }
        match last_error() {
            ERROR_PIPE_CONNECTED => return Ok(()),
            ERROR_PIPE_LISTENING | ERROR_NO_DATA => std::thread::sleep(Duration::from_millis(10)),
            _ => return Err(CapabilityFailure::MalformedProtocol),
        }
    }
    Err(CapabilityFailure::Timeout)
}

fn read_exact_timeout(handle: Handle, output: &mut [u8], deadline: Instant) -> bool {
    while Instant::now() < deadline {
        let mut available = 0u32;
        let ok = unsafe {
            PeekNamedPipe(
                handle,
                null_mut(),
                0,
                null_mut(),
                &mut available,
                null_mut(),
            )
        };
        if ok == 0 {
            if matches!(last_error(), ERROR_NO_DATA | ERROR_PIPE_LISTENING) {
                std::thread::sleep(Duration::from_millis(5));
                continue;
            }
            return false;
        }
        if available < output.len() as u32 {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }
        let mut read = 0u32;
        return unsafe {
            ReadFile(
                handle,
                output.as_mut_ptr().cast(),
                output.len() as u32,
                &mut read,
                null_mut(),
            )
        } != 0
            && read as usize == output.len();
    }
    false
}

fn write_exact(handle: Handle, input: &[u8]) -> bool {
    let mut written = 0u32;
    let ok = unsafe {
        WriteFile(
            handle,
            input.as_ptr().cast(),
            input.len() as u32,
            &mut written,
            null_mut(),
        )
    };
    ok != 0 && written as usize == input.len()
}

fn last_error() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(-1)
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
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        let error = std::io::Error::last_os_error();
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
fn local_file_usn(path: &Path) -> Option<i64> {
    let (file, _) = protected_file(path)?;
    let mut output = [0u8; 256];
    let mut returned = 0u32;
    if unsafe {
        DeviceIoControl(
            file.as_raw_handle().cast(),
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
        return None;
    }
    Some(i64::from_le_bytes(output[24..32].try_into().ok()?))
}

#[cfg(test)]
mod windows_tests {
    use super::*;

    fn temp_path(label: &str) -> PathBuf {
        let mut random = [0u8; 8];
        OsRng.fill_bytes(&mut random);
        std::env::temp_dir().join(format!(
            "bootoptim-usn-{label}-{}-{}",
            std::process::id(),
            hex::encode(random)
        ))
    }

    #[test]
    fn helper_digest_pin_is_strict_lower_hex() {
        let valid = "0123456789abcdef".repeat(4);
        assert!(valid_helper_digest(&valid));
        assert!(!valid_helper_digest(&valid[..63]));
        assert!(!valid_helper_digest(&valid.to_uppercase()));
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
    fn same_size_mutation_with_restored_mtime_changes_real_ntfs_usn() {
        let dir = temp_path("same-size");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("asset");
        std::fs::write(&path, b"abcdefgh").unwrap();
        let Some(before) = local_file_usn(&path) else {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        };
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();

        let mut file = OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        file.write_all(b"ABCDEFGH").unwrap();
        file.sync_all().unwrap();
        file.set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
        drop(file);

        let Some(after) = local_file_usn(&path) else {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        };
        assert_ne!(before, after);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn truncate_restore_with_restored_mtime_changes_real_ntfs_usn() {
        let dir = temp_path("truncate");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("asset");
        std::fs::write(&path, b"0123456789").unwrap();
        let Some(before) = local_file_usn(&path) else {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        };
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();

        let mut file = OpenOptions::new().read(true).write(true).open(&path).unwrap();
        file.set_len(3).unwrap();
        file.set_len(10).unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        file.write_all(b"0123456789").unwrap();
        file.sync_all().unwrap();
        file.set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
        drop(file);

        let Some(after) = local_file_usn(&path) else {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        };
        assert_ne!(before, after);
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
