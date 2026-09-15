#[cfg(not(windows))]
fn main() {
    eprintln!("windows only");
    std::process::exit(2);
}

#[cfg(windows)]
mod windows_probe {
    use std::{
        ffi::{c_void, OsStr},
        fs::{File, OpenOptions},
        io::{Seek, SeekFrom, Write},
        mem::zeroed,
        os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle},
        path::Path,
        ptr::{null, null_mut},
    };

    type Handle = *mut c_void;
    const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
    const FILE_SHARE_READ: u32 = 0x1;
    const FILE_SHARE_WRITE: u32 = 0x2;
    const FILE_SHARE_DELETE: u32 = 0x4;
    const FILE_TRAVERSE: u32 = 0x20;
    const OPEN_EXISTING: u32 = 3;
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const VOLUME_NAME_GUID: u32 = 0x1;
    const FSCTL_QUERY_USN_JOURNAL: u32 = 0x0009_00f4;
    const FSCTL_READ_FILE_USN_DATA: u32 = 0x0009_00eb;
    const TOKEN_DUPLICATE: u32 = 0x2;
    const TOKEN_QUERY: u32 = 0x8;
    const DISABLE_MAX_PRIVILEGE: u32 = 0x1;
    const WIN_BUILTIN_ADMINISTRATORS_SID: i32 = 26;
    const SECURITY_MAX_SID_SIZE: usize = 68;

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

    #[repr(C)]
    struct SidAndAttributes {
        sid: *mut c_void,
        attributes: u32,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct JournalState {
        id: u64,
        first: i64,
        next: i64,
        lowest_valid: i64,
    }

    struct OwnedHandle(Handle);
    impl OwnedHandle {
        fn new(handle: Handle) -> Option<Self> {
            (!handle.is_null() && handle != INVALID_HANDLE_VALUE).then_some(Self(handle))
        }
    }
    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }

    struct RestrictedImpersonation {
        _token: OwnedHandle,
    }
    impl Drop for RestrictedImpersonation {
        fn drop(&mut self) {
            unsafe {
                RevertToSelf();
            }
        }
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
        fn GetFileInformationByHandle(handle: Handle, info: *mut ByHandleFileInformation) -> i32;
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
    }

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn OpenProcessToken(process: Handle, access: u32, token: *mut Handle) -> i32;
        fn CreateRestrictedToken(
            existing_token: Handle,
            flags: u32,
            disable_sid_count: u32,
            sids_to_disable: *const SidAndAttributes,
            delete_privilege_count: u32,
            privileges_to_delete: *const c_void,
            restricted_sid_count: u32,
            sids_to_restrict: *const SidAndAttributes,
            new_token: *mut Handle,
        ) -> i32;
        fn CreateWellKnownSid(
            sid_type: i32,
            domain_sid: *const c_void,
            sid: *mut c_void,
            sid_size: *mut u32,
        ) -> i32;
        fn ImpersonateLoggedOnUser(token: Handle) -> i32;
        fn RevertToSelf() -> i32;
        fn CheckTokenMembership(token: Handle, sid: *const c_void, is_member: *mut i32) -> i32;
    }

    fn wide(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(Some(0)).collect()
    }

    fn administrators_sid() -> Result<[u8; SECURITY_MAX_SID_SIZE], String> {
        let mut sid = [0u8; SECURITY_MAX_SID_SIZE];
        let mut size = sid.len() as u32;
        if unsafe {
            CreateWellKnownSid(
                WIN_BUILTIN_ADMINISTRATORS_SID,
                null(),
                sid.as_mut_ptr().cast(),
                &mut size,
            )
        } == 0
        {
            return Err(format!("CreateWellKnownSid failed: {}", std::io::Error::last_os_error()));
        }
        Ok(sid)
    }

    fn effective_admin_member(admin_sid: &[u8; SECURITY_MAX_SID_SIZE]) -> Result<bool, String> {
        let mut member = 0i32;
        if unsafe { CheckTokenMembership(null_mut(), admin_sid.as_ptr().cast(), &mut member) } == 0 {
            return Err(format!("CheckTokenMembership failed: {}", std::io::Error::last_os_error()));
        }
        Ok(member != 0)
    }

    fn enter_restricted_token() -> Result<RestrictedImpersonation, String> {
        let admin_sid = administrators_sid()?;
        let mut process_token = null_mut();
        if unsafe {
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_DUPLICATE | TOKEN_QUERY,
                &mut process_token,
            )
        } == 0
        {
            return Err(format!("OpenProcessToken failed: {}", std::io::Error::last_os_error()));
        }
        let process_token = OwnedHandle::new(process_token).ok_or_else(|| "invalid process token".to_string())?;
        let disabled = SidAndAttributes {
            sid: admin_sid.as_ptr().cast_mut().cast(),
            attributes: 0,
        };
        let mut restricted = null_mut();
        if unsafe {
            CreateRestrictedToken(
                process_token.0,
                DISABLE_MAX_PRIVILEGE,
                1,
                &disabled,
                0,
                null(),
                0,
                null(),
                &mut restricted,
            )
        } == 0
        {
            return Err(format!("CreateRestrictedToken failed: {}", std::io::Error::last_os_error()));
        }
        let restricted = OwnedHandle::new(restricted).ok_or_else(|| "invalid restricted token".to_string())?;
        if unsafe { ImpersonateLoggedOnUser(restricted.0) } == 0 {
            return Err(format!("ImpersonateLoggedOnUser failed: {}", std::io::Error::last_os_error()));
        }
        if effective_admin_member(&admin_sid)? {
            unsafe {
                RevertToSelf();
            }
            return Err("restricted token still has effective Administrators membership".into());
        }
        Ok(RestrictedImpersonation { _token: restricted })
    }

    fn protected_file(path: &Path) -> Result<File, String> {
        OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
            .map_err(|error| format!("protected open failed: {error}"))
    }

    fn volume_guid_from_handle(handle: Handle) -> Result<String, String> {
        let mut info: ByHandleFileInformation = unsafe { zeroed() };
        if unsafe { GetFileInformationByHandle(handle, &mut info) } == 0 {
            return Err("GetFileInformationByHandle failed".into());
        }
        if info.file_attributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) != 0 {
            return Err("fixture is not an eligible regular file".into());
        }
        let mut buffer = [0u16; 32768];
        let written = unsafe {
            GetFinalPathNameByHandleW(handle, buffer.as_mut_ptr(), buffer.len() as u32, VOLUME_NAME_GUID)
        } as usize;
        if written == 0 || written >= buffer.len() {
            return Err("GetFinalPathNameByHandleW failed".into());
        }
        let full = String::from_utf16_lossy(&buffer[..written]);
        let end = full
            .find("}\\")
            .map(|index| index + 2)
            .ok_or_else(|| format!("unexpected volume GUID path: {full}"))?;
        Ok(full[..end].to_owned())
    }

    fn open_volume(volume_guid: &str) -> Result<OwnedHandle, String> {
        let path = volume_guid
            .strip_suffix('\\')
            .ok_or_else(|| "volume GUID missing trailing slash".to_string())?;
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
        .ok_or_else(|| format!("restricted volume open failed: {}", std::io::Error::last_os_error()))
    }

    fn parse_journal(bytes: &[u8], returned: u32) -> Option<JournalState> {
        if returned < 32 || bytes.len() < 32 {
            return None;
        }
        Some(JournalState {
            id: u64::from_le_bytes(bytes[0..8].try_into().ok()?),
            first: i64::from_le_bytes(bytes[8..16].try_into().ok()?),
            next: i64::from_le_bytes(bytes[16..24].try_into().ok()?),
            lowest_valid: i64::from_le_bytes(bytes[24..32].try_into().ok()?),
        })
    }

    fn query_journal(volume: Handle) -> Result<JournalState, String> {
        let mut output = [0u8; 128];
        let mut returned = 0u32;
        if unsafe {
            DeviceIoControl(
                volume,
                FSCTL_QUERY_USN_JOURNAL,
                null(),
                0,
                output.as_mut_ptr().cast(),
                output.len() as u32,
                &mut returned,
                null_mut(),
            )
        } == 0
        {
            return Err(format!("FSCTL_QUERY_USN_JOURNAL failed: {}", std::io::Error::last_os_error()));
        }
        let state = parse_journal(&output, returned).ok_or_else(|| "short journal response".to_string())?;
        if state.id == 0
            || state.first < 0
            || state.lowest_valid < 0
            || state.next < state.first
            || state.next < state.lowest_valid
        {
            return Err("invalid journal state".into());
        }
        Ok(state)
    }

    fn file_usn(file: &File) -> Result<i64, String> {
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
        {
            return Err(format!("FSCTL_READ_FILE_USN_DATA failed: {}", std::io::Error::last_os_error()));
        }
        if returned < 32 || u16::from_le_bytes([output[4], output[5]]) != 2 {
            return Err("unsupported/short file USN response".into());
        }
        Ok(i64::from_le_bytes(output[24..32].try_into().unwrap()))
    }

    fn snapshot(path: &Path) -> Result<(String, JournalState, i64), String> {
        let file = protected_file(path)?;
        let volume_guid = volume_guid_from_handle(file.as_raw_handle().cast())?;
        let volume = open_volume(&volume_guid)?;
        let before = query_journal(volume.0)?;
        let usn = file_usn(&file)?;
        let after = query_journal(volume.0)?;
        if before.id != after.id
            || after.first < before.first
            || after.lowest_valid < before.lowest_valid
            || after.next < before.next
        {
            return Err("journal changed incompatibly across file query".into());
        }
        Ok((volume_guid, after, usn))
    }

    pub fn run() -> Result<(), String> {
        let dir = std::env::temp_dir().join(format!("bootoptim-usn-unprivileged-{}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        let path = dir.join("asset");
        std::fs::write(&path, b"abcdefgh").map_err(|error| error.to_string())?;

        let admin_sid = administrators_sid()?;
        let was_admin = effective_admin_member(&admin_sid)?;
        let restricted = enter_restricted_token()?;
        if effective_admin_member(&admin_sid)? {
            return Err("restricted probe unexpectedly has effective Administrators membership".into());
        }

        let first = snapshot(&path)?;
        let mut file = OpenOptions::new().write(true).open(&path).map_err(|error| error.to_string())?;
        file.seek(SeekFrom::Start(0)).map_err(|error| error.to_string())?;
        file.write_all(b"ABCDEFGH").map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        drop(file);
        let second = snapshot(&path)?;
        drop(restricted);

        let _ = std::fs::remove_dir_all(&dir);
        if first.0 != second.0 || first.1.id != second.1.id || first.2 == second.2 {
            return Err("mutation did not produce stable-volume / changed-file-USN evidence".into());
        }

        println!(
            "ok original_admin_member={} restricted_admin_member=false volume={} journal_id={} first_file_usn={} second_file_usn={}",
            was_admin, second.0, second.1.id, first.2, second.2
        );
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn journal_layout_matches_usn_journal_data_v0_prefix() {
            let mut bytes = [0u8; 32];
            bytes[0..8].copy_from_slice(&7u64.to_le_bytes());
            bytes[8..16].copy_from_slice(&10i64.to_le_bytes());
            bytes[16..24].copy_from_slice(&20i64.to_le_bytes());
            bytes[24..32].copy_from_slice(&12i64.to_le_bytes());
            assert_eq!(
                parse_journal(&bytes, 32),
                Some(JournalState { id: 7, first: 10, next: 20, lowest_valid: 12 })
            );
            assert_eq!(parse_journal(&bytes, 31), None);
        }
    }
}

#[cfg(windows)]
fn main() {
    if let Err(error) = windows_probe::run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
