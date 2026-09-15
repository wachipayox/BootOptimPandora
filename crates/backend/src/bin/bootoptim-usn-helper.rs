#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(not(windows))]
fn main() {
    std::process::exit(2);
}

#[cfg(windows)]
#[path = "../usn_protocol.rs"]
mod usn_protocol;

#[cfg(windows)]
mod windows_helper {
    use super::usn_protocol::{self, Handshake, Request, RequestKind, Response, ResponseStatus};
    use std::{
        ffi::{OsStr, c_void},
        mem::{size_of, zeroed},
        os::windows::ffi::OsStrExt,
        ptr::{null, null_mut},
        time::{Duration, Instant},
    };

    type Handle = *mut c_void;
    const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
    const GENERIC_READ: u32 = 0x8000_0000;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const FILE_READ_ATTRIBUTES: u32 = 0x80;
    const FILE_SHARE_READ: u32 = 0x1;
    const FILE_SHARE_WRITE: u32 = 0x2;
    const FILE_SHARE_DELETE: u32 = 0x4;
    const OPEN_EXISTING: u32 = 3;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const PIPE_NOWAIT: u32 = 0x1;
    const ERROR_NO_DATA: i32 = 232;
    const ERROR_PIPE_LISTENING: i32 = 536;
    const FSCTL_QUERY_USN_JOURNAL: u32 = 0x0009_00f4;
    const FSCTL_READ_FILE_USN_DATA: u32 = 0x0009_00eb;
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    const IO_TIMEOUT: Duration = Duration::from_secs(5);

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
    union FileIdUnion {
        file_id: i64,
        padding: [u8; 16],
    }

    #[repr(C)]
    struct FileIdDescriptor {
        size: u32,
        id_type: i32,
        id: FileIdUnion,
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
        fn ReadFile(handle: Handle, buffer: *mut c_void, len: u32, read: *mut u32, overlapped: *mut c_void) -> i32;
        fn WriteFile(
            handle: Handle,
            buffer: *const c_void,
            len: u32,
            written: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
        fn PeekNamedPipe(
            handle: Handle,
            buffer: *mut c_void,
            len: u32,
            read: *mut u32,
            available: *mut u32,
            left: *mut u32,
        ) -> i32;
        fn SetNamedPipeHandleState(
            handle: Handle,
            mode: *mut u32,
            max_collection_count: *mut u32,
            collect_timeout: *mut u32,
        ) -> i32;
        fn GetNamedPipeServerProcessId(handle: Handle, pid: *mut u32) -> i32;
        fn GetCurrentProcessId() -> u32;
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
        fn OpenFileById(
            volume: Handle,
            descriptor: *const FileIdDescriptor,
            access: u32,
            share: u32,
            security: *const c_void,
            flags: u32,
        ) -> Handle;
    }

    struct OwnedHandle(Handle);
    impl OwnedHandle {
        fn new(handle: Handle) -> Option<Self> {
            (!handle.is_null() && handle != INVALID_HANDLE_VALUE).then_some(Self(handle))
        }
    }
    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    fn wide(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(Some(0)).collect()
    }

    fn last_error() -> i32 {
        std::io::Error::last_os_error().raw_os_error().unwrap_or(-1)
    }

    fn read_exact_timeout(handle: Handle, output: &mut [u8], deadline: Instant) -> bool {
        while Instant::now() < deadline {
            let mut available = 0u32;
            let ok = unsafe { PeekNamedPipe(handle, null_mut(), 0, null_mut(), &mut available, null_mut()) };
            if ok == 0 {
                let error = last_error();
                if error == ERROR_NO_DATA || error == ERROR_PIPE_LISTENING {
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
            let ok =
                unsafe { ReadFile(handle, output.as_mut_ptr().cast(), output.len() as u32, &mut read, null_mut()) };
            return ok != 0 && read as usize == output.len();
        }
        false
    }

    fn write_exact(handle: Handle, input: &[u8]) -> bool {
        let mut written = 0u32;
        let ok = unsafe { WriteFile(handle, input.as_ptr().cast(), input.len() as u32, &mut written, null_mut()) };
        ok != 0 && written as usize == input.len()
    }

    fn parse_nonce(value: &str) -> Option<[u8; usn_protocol::NONCE_LEN]> {
        if value.len() != usn_protocol::NONCE_LEN * 2
            || !value.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return None;
        }
        let mut out = [0u8; usn_protocol::NONCE_LEN];
        hex::decode_to_slice(value, &mut out).ok()?;
        Some(out)
    }

    fn file_id(info: &ByHandleFileInformation) -> [u8; 16] {
        let value = ((info.file_index_high as u64) << 32) | info.file_index_low as u64;
        let mut out = [0u8; 16];
        out[..8].copy_from_slice(&value.to_le_bytes());
        out
    }

    fn open_volume(volume_guid: &str) -> Option<(OwnedHandle, u64)> {
        let without_slash = volume_guid.strip_suffix('\\')?;
        let path = wide(OsStr::new(without_slash));
        let handle = OwnedHandle::new(unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                null(),
                OPEN_EXISTING,
                0,
                null_mut(),
            )
        })?;
        let mut serial = 0u32;
        let mut max_component = 0u32;
        let mut flags = 0u32;
        let mut fs_name = [0u16; 16];
        if unsafe {
            GetVolumeInformationByHandleW(
                handle.0,
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
        let fs_len = fs_name.iter().position(|c| *c == 0).unwrap_or(fs_name.len());
        if String::from_utf16_lossy(&fs_name[..fs_len]) != "NTFS" {
            return None;
        }
        Some((handle, serial as u64))
    }

    fn journal_state(volume: Handle) -> Option<usn_protocol::JournalState> {
        let mut out = [0u8; 128];
        let mut returned = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                volume,
                FSCTL_QUERY_USN_JOURNAL,
                null(),
                0,
                out.as_mut_ptr().cast(),
                out.len() as u32,
                &mut returned,
                null_mut(),
            )
        };
        if ok == 0 || returned < 32 {
            return None;
        }
        Some((
            u64::from_le_bytes(out[0..8].try_into().ok()?),
            i64::from_le_bytes(out[8..16].try_into().ok()?),
            i64::from_le_bytes(out[24..32].try_into().ok()?),
            i64::from_le_bytes(out[16..24].try_into().ok()?),
        ))
    }

    fn file_usn(volume: Handle, requested_id: [u8; 16]) -> Option<i64> {
        if requested_id[8..].iter().any(|b| *b != 0) {
            return None;
        }
        let raw_id = i64::from_le_bytes(requested_id[..8].try_into().ok()?);
        let descriptor = FileIdDescriptor {
            size: size_of::<FileIdDescriptor>() as u32,
            id_type: 0,
            id: FileIdUnion { file_id: raw_id },
        };
        let file = OwnedHandle::new(unsafe {
            OpenFileById(
                volume,
                &descriptor,
                FILE_READ_ATTRIBUTES,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                null(),
                FILE_FLAG_OPEN_REPARSE_POINT,
            )
        })?;
        let mut info: ByHandleFileInformation = unsafe { zeroed() };
        if unsafe { GetFileInformationByHandle(file.0, &mut info) } == 0
            || info.file_attributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) != 0
            || file_id(&info) != requested_id
        {
            return None;
        }
        let mut out = [0u8; 256];
        let mut returned = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                file.0,
                FSCTL_READ_FILE_USN_DATA,
                null(),
                0,
                out.as_mut_ptr().cast(),
                out.len() as u32,
                &mut returned,
                null_mut(),
            )
        };
        if ok == 0 || returned < 32 || u16::from_le_bytes([out[4], out[5]]) != 2 {
            return None;
        }
        Some(i64::from_le_bytes(out[24..32].try_into().ok()?))
    }

    fn query(request: Request) -> Response {
        let mut response = Response {
            status: ResponseStatus::InvalidRequest,
            nonce: request.nonce,
            volume_serial: 0,
            journal_id: 0,
            first_usn: 0,
            lowest_valid_usn: 0,
            next_usn: 0,
            file_id: request.file_id,
            file_usn: 0,
        };
        let Some(volume_guid) = usn_protocol::volume_guid_str(&request.volume_guid) else {
            return response;
        };
        let Some((volume, serial)) = open_volume(volume_guid) else {
            response.status = ResponseStatus::NonNtfs;
            return response;
        };
        let Some(before) = journal_state(volume.0) else {
            response.status = ResponseStatus::IoError;
            return response;
        };
        if !usn_protocol::journal_state_is_valid(before) {
            response.status = ResponseStatus::IoError;
            return response;
        }

        let mut current = before;
        response.volume_serial = serial;
        if request.kind == RequestKind::File {
            let Some(usn) = file_usn(volume.0, request.file_id) else {
                response.status = ResponseStatus::NotFound;
                return response;
            };
            let Some(after) = journal_state(volume.0) else {
                response.status = ResponseStatus::IoError;
                return response;
            };
            if !usn_protocol::journal_transition_is_consistent(before, after) {
                response.status = ResponseStatus::IoError;
                return response;
            }
            current = after;
            response.file_usn = usn;
        }
        response.journal_id = current.0;
        response.first_usn = current.1;
        response.lowest_valid_usn = current.2;
        response.next_usn = current.3;
        response.status = ResponseStatus::Ok;
        response
    }

    pub fn run() -> i32 {
        let mut pipe_arg = None;
        let mut nonce_arg = None;
        let mut server_pid_arg = None;
        let mut args = std::env::args_os().skip(1);
        while let Some(arg) = args.next() {
            match arg.to_str() {
                Some("--pipe") if pipe_arg.is_none() => pipe_arg = args.next(),
                Some("--nonce") if nonce_arg.is_none() => nonce_arg = args.next(),
                Some("--server-pid") if server_pid_arg.is_none() => server_pid_arg = args.next(),
                _ => return 2,
            }
        }
        let Some(pipe) = pipe_arg.and_then(|v| v.into_string().ok()) else {
            return 2;
        };
        let Some(nonce_text) = nonce_arg.and_then(|v| v.into_string().ok()) else {
            return 2;
        };
        let Some(nonce) = parse_nonce(&nonce_text) else {
            return 2;
        };
        let expected_pipe = format!(r"\\.\pipe\BootOptimPandora-USN-v1-{nonce_text}");
        if pipe != expected_pipe {
            return 2;
        }
        let Some(server_pid) = server_pid_arg.and_then(|v| v.to_str().and_then(|s| s.parse::<u32>().ok())) else {
            return 2;
        };
        if server_pid == 0 {
            return 2;
        }

        let pipe_wide = wide(OsStr::new(&pipe));
        let handle = OwnedHandle::new(unsafe {
            CreateFileW(pipe_wide.as_ptr(), GENERIC_READ | GENERIC_WRITE, 0, null(), OPEN_EXISTING, 0, null_mut())
        });
        let Some(pipe_handle) = handle else {
            return 3;
        };
        let mut actual_server_pid = 0u32;
        if unsafe { GetNamedPipeServerProcessId(pipe_handle.0, &mut actual_server_pid) } == 0
            || actual_server_pid != server_pid
        {
            return 4;
        }
        let mut mode = PIPE_NOWAIT;
        if unsafe { SetNamedPipeHandleState(pipe_handle.0, &mut mode, null_mut(), null_mut()) } == 0 {
            return 4;
        }

        let deadline = Instant::now() + IO_TIMEOUT;
        let mut handshake_bytes = [0u8; usn_protocol::HANDSHAKE_LEN];
        if !read_exact_timeout(pipe_handle.0, &mut handshake_bytes, deadline) {
            return 5;
        }
        let Some(handshake) = usn_protocol::decode_handshake(&handshake_bytes) else {
            return 4;
        };
        if handshake.nonce != nonce || handshake.pid != server_pid {
            return 4;
        }
        let reply = usn_protocol::encode_handshake(Handshake {
            nonce,
            pid: unsafe { GetCurrentProcessId() },
        });
        if !write_exact(pipe_handle.0, &reply) {
            return 5;
        }

        let mut session_volume: Option<[u8; usn_protocol::VOLUME_GUID_LEN]> = None;
        loop {
            let deadline = Instant::now() + IO_TIMEOUT;
            let mut request_bytes = [0u8; usn_protocol::REQUEST_LEN];
            if !read_exact_timeout(pipe_handle.0, &mut request_bytes, deadline) {
                return 5;
            }
            let Some(request) = usn_protocol::decode_request(&request_bytes) else {
                return 4;
            };
            if request.nonce != nonce {
                return 4;
            }
            if request.kind == RequestKind::Shutdown {
                return 0;
            }
            if usn_protocol::volume_guid_str(&request.volume_guid).is_none() {
                return 4;
            }
            if request.kind == RequestKind::Journal && request.file_id != [0; 16] {
                return 4;
            }
            match session_volume {
                Some(volume) if volume != request.volume_guid => return 4,
                None => session_volume = Some(request.volume_guid),
                _ => {},
            }
            let response = query(request);
            if !write_exact(pipe_handle.0, &usn_protocol::encode_response(response)) {
                return 5;
            }
        }
    }
}

#[cfg(windows)]
fn main() {
    std::process::exit(windows_helper::run());
}
