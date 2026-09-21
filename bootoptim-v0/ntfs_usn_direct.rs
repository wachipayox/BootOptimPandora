//! Policy-free direct NTFS/USN capability shared by asset and AppCDS caches.
//!
//! This module never elevates, spawns a helper, infers cache policy, or hashes
//! content. Callers retain the protected file handle while evaluating their
//! own cache records.

use std::{
    ffi::{OsStr, c_void},
    fs::{File, OpenOptions},
    io,
    mem::zeroed,
    os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle},
    path::Path,
    ptr::{null, null_mut},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
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
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[repr(C)]
#[derive(Clone, Copy)]
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
    fn new(value: Handle) -> io::Result<Self> {
        if value.is_null() || value == INVALID_HANDLE_VALUE {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(value))
        }
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0); }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FileIdentity {
    pub(crate) volume_guid: String,
    pub(crate) volume_serial: u64,
    pub(crate) file_id: [u8; 16],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct JournalSnapshot {
    pub(crate) volume_serial: u64,
    pub(crate) journal_id: u64,
    pub(crate) first_usn: i64,
    pub(crate) lowest_valid_usn: i64,
    pub(crate) next_usn: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FileEvidence {
    pub(crate) journal: JournalSnapshot,
    pub(crate) file_id: [u8; 16],
    pub(crate) file_usn: i64,
}

pub(crate) struct ProtectedFile {
    file: File,
    identity: FileIdentity,
}

impl ProtectedFile {
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        let (identity, attributes) = identity_from_handle(file.as_raw_handle().cast())?;
        if attributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY) != 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "not a regular non-reparse NTFS file"));
        }
        Ok(Self { file, identity })
    }

    pub(crate) fn identity(&self) -> &FileIdentity {
        &self.identity
    }

    pub(crate) fn file_mut(&mut self) -> &mut File {
        &mut self.file
    }

    pub(crate) fn identity_unchanged(&self) -> bool {
        identity_from_handle(self.file.as_raw_handle().cast())
            .is_ok_and(|(identity, attributes)| {
                attributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY) == 0
                    && identity == self.identity
            })
    }
}

pub(crate) fn directory_identity(path: &Path) -> io::Result<FileIdentity> {
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
    let (identity, attributes) = identity_from_handle(handle.0)?;
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 || attributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not a non-reparse NTFS directory"));
    }
    Ok(identity)
}

pub(crate) struct Volume {
    handle: OwnedHandle,
    serial: u64,
}

impl Volume {
    pub(crate) fn open(identity: &FileIdentity) -> io::Result<Self> {
        let path = identity
            .volume_guid
            .strip_suffix('\\')
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid volume guid"))?;
        let wide = wide(OsStr::new(path));
        let handle = OwnedHandle::new(unsafe {
            CreateFileW(
                wide.as_ptr(),
                FILE_TRAVERSE,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                null(),
                OPEN_EXISTING,
                0,
                null_mut(),
            )
        })?;
        let serial = volume_serial(handle.0)?;
        if serial != identity.volume_serial {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "volume identity changed"));
        }
        Ok(Self { handle, serial })
    }

    pub(crate) fn query_journal(&self) -> io::Result<JournalSnapshot> {
        query_journal_handle(self.handle.0, self.serial)
    }

    pub(crate) fn query_file(
        &self,
        file: &ProtectedFile,
        expected_file_id: [u8; 16],
    ) -> io::Result<FileEvidence> {
        let before = self.query_journal()?;
        let (file_id, file_usn) = file_usn_from_handle(file.file.as_raw_handle().cast())?;
        let after = self.query_journal()?;
        if before.journal_id != after.journal_id
            || after.next_usn < before.next_usn
            || file_id != expected_file_id
            || after.volume_serial != file.identity.volume_serial
        {
            return Err(io::Error::new(io::ErrorKind::Other, "unstable NTFS/USN evidence"));
        }
        Ok(FileEvidence { journal: after, file_id, file_usn })
    }
}

fn valid_journal(reply: &JournalSnapshot) -> bool {
    reply.journal_id != 0
        && reply.first_usn >= 0
        && reply.lowest_valid_usn >= 0
        && reply.next_usn >= 0
        && reply.next_usn >= reply.first_usn
        && reply.next_usn >= reply.lowest_valid_usn
}

fn basic_file_information(handle: Handle) -> io::Result<ByHandleFileInformation> {
    let mut info: ByHandleFileInformation = unsafe { zeroed() };
    if unsafe { GetFileInformationByHandle(handle, &mut info) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(info)
    }
}

fn volume_serial(handle: Handle) -> io::Result<u64> {
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
        return Err(io::Error::last_os_error());
    }
    let len = fs_name.iter().position(|value| *value == 0).unwrap_or(fs_name.len());
    if String::from_utf16_lossy(&fs_name[..len]) != "NTFS" {
        return Err(io::Error::new(io::ErrorKind::Unsupported, "volume is not NTFS"));
    }
    Ok(serial as u64)
}

fn identity_from_handle(handle: Handle) -> io::Result<(FileIdentity, u32)> {
    let info = basic_file_information(handle)?;
    let serial = volume_serial(handle)?;
    if info.volume_serial_number as u64 != serial {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "volume serial mismatch"));
    }

    let mut final_path = [0u16; 32768];
    let written = unsafe {
        GetFinalPathNameByHandleW(handle, final_path.as_mut_ptr(), final_path.len() as u32, VOLUME_NAME_GUID)
    } as usize;
    if written == 0 || written >= final_path.len() {
        return Err(io::Error::last_os_error());
    }
    let path = String::from_utf16_lossy(&final_path[..written]);
    let end = path
        .find("}\\")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing volume guid"))?
        + 2;
    let volume_guid = path
        .get(..end)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid volume guid"))?
        .to_owned();
    if !volume_guid.starts_with(r"\\?\Volume{") || !volume_guid.ends_with("}\\") {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid volume guid"));
    }

    let raw_id = ((info.file_index_high as u64) << 32) | info.file_index_low as u64;
    let mut file_id = [0u8; 16];
    file_id[..8].copy_from_slice(&raw_id.to_le_bytes());
    Ok((
        FileIdentity { volume_guid, volume_serial: serial, file_id },
        info.file_attributes,
    ))
}

fn query_journal_handle(handle: Handle, volume_serial: u64) -> io::Result<JournalSnapshot> {
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
        return Err(io::Error::last_os_error());
    }
    let reply = JournalSnapshot {
        volume_serial,
        journal_id: u64::from_le_bytes(output[0..8].try_into().unwrap()),
        first_usn: i64::from_le_bytes(output[8..16].try_into().unwrap()),
        next_usn: i64::from_le_bytes(output[16..24].try_into().unwrap()),
        lowest_valid_usn: i64::from_le_bytes(output[24..32].try_into().unwrap()),
    };
    if !valid_journal(&reply) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid USN journal state"));
    }
    Ok(reply)
}

fn file_usn_from_handle(handle: Handle) -> io::Result<([u8; 16], i64)> {
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
        return Err(io::Error::last_os_error());
    }
    let mut file_id = [0u8; 16];
    file_id[..8].copy_from_slice(&output[8..16]);
    let file_usn = i64::from_le_bytes(output[24..32].try_into().unwrap());
    if file_usn < 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "negative file USN"));
    }
    Ok((file_id, file_usn))
}

fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

pub(crate) fn atomic_replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let count = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp = path.with_extension(format!("bootoptim-{}-{}-{}.tmp", std::process::id(), stamp, count));
    {
        use std::io::Write;
        let mut file = OpenOptions::new().write(true).create_new(true).open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }

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
        let error = io::Error::last_os_error();
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Seek, SeekFrom, Write};

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let count = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("bootoptim-ntfs-direct-{label}-{}-{count}", std::process::id()))
    }

    #[test]
    fn protected_handle_blocks_write_delete_and_rechecks_identity() {
        let dir = temp_dir("toctou");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("file");
        std::fs::write(&path, b"original").unwrap();
        let Ok(file) = ProtectedFile::open(&path) else {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        };
        assert!(OpenOptions::new().write(true).open(&path).is_err());
        assert!(std::fs::remove_file(&path).is_err());
        assert!(file.identity_unchanged());
        drop(file);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn direct_query_detects_same_size_restored_mtime_mutation() {
        let dir = temp_dir("mutation");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("file");
        std::fs::write(&path, b"abcdefgh").unwrap();
        let Ok(file) = ProtectedFile::open(&path) else {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        };
        let Ok(volume) = Volume::open(file.identity()) else {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        };
        let before = volume.query_file(&file, file.identity().file_id).unwrap();
        drop(file);
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        let mut writer = OpenOptions::new().write(true).open(&path).unwrap();
        writer.seek(SeekFrom::Start(0)).unwrap();
        writer.write_all(b"ABCDEFGH").unwrap();
        writer.sync_all().unwrap();
        writer.set_times(std::fs::FileTimes::new().set_modified(modified)).unwrap();
        drop(writer);
        let file = ProtectedFile::open(&path).unwrap();
        let after = volume.query_file(&file, file.identity().file_id).unwrap();
        assert_ne!(before.file_usn, after.file_usn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_recreate_changes_file_id_and_reparse_is_rejected() {
        let dir = temp_dir("replace");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("file");
        std::fs::write(&path, b"same").unwrap();
        let first = ProtectedFile::open(&path).unwrap().identity().clone();
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, b"same").unwrap();
        let second = ProtectedFile::open(&path).unwrap().identity().clone();
        assert_eq!(first.volume_guid, second.volume_guid);
        assert_ne!(first.file_id, second.file_id);

        let link = dir.join("link");
        if std::os::windows::fs::symlink_file(&path, &link).is_ok() {
            assert!(ProtectedFile::open(&link).is_err());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
