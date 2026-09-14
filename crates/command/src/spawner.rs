use std::{
    ffi::{OsStr, OsString},
    fs::OpenOptions,
    io::{Error, ErrorKind, Write},
    sync::mpsc,
};

use once_cell::sync::OnceCell;

use crate::{PandoraChild, PandoraCommand, PandoraSandbox};

const LAUNCH_PROBE_ENV: &str = "BOOTOPTIM_LAUNCH_PROBE";
const LAUNCH_PROBE_SCHEMA: &str = "bootoptim.launch_probe.v1";

pub enum SpawnType {
    Normal,
    Elevated,
    Sandboxed(PandoraSandbox),
}

struct SpawnInfo {
    command: PandoraCommand,
    spawn_type: SpawnType,
    sender: tokio::sync::oneshot::Sender<std::io::Result<PandoraChild>>,
}

// We use a special thread for spawning commands, this is for a few reasons:
// 1. It prevents unix children from being terminated early due to PR_SET_PDEATHSIG if the parent thread is killed
// 2. It prevents race conditions such as inheriting handles on windows
// 3. Some functions e.g. ShellExecuteW will block the current thread until the user interacts with the UAC dialog

#[derive(Default)]
pub struct SpawnContext {
    #[cfg(windows)]
    pub job_handle: Option<std::os::windows::io::OwnedHandle>,
    #[cfg(windows)]
    pub null_device: Option<std::os::windows::io::OwnedHandle>,
    #[cfg(unix)]
    pub dev_null_fd: Option<libc::c_int>,
    #[cfg(target_os = "linux")]
    pub dbus_proxy: Option<crate::unix::linux::bwrap::DbusProxy>,
}

#[cfg(windows)]
fn illegal_filename_char(b: u8) -> bool {
    b < 0x1f || matches!(b, b'/' | b'?' | b'<' | b'>' | b'\\' | b':' | b'*' | b'|' | b'"')
}

pub fn spawn(command: PandoraCommand, spawn_type: SpawnType) -> tokio::sync::oneshot::Receiver<std::io::Result<PandoraChild>> {
    let (sender, receiver) = tokio::sync::oneshot::channel();

    if is_probe_minecraft_launch(&command) {
        probe_event("classpath_resolution", "inclusive_end", None);
        probe_event("native_extraction", "inclusive_end", None);
        probe_event("wrapper_arguments", "inclusive_end", None);
    }

    static SPAWNING_CHANNEL: OnceCell<mpsc::Sender<SpawnInfo>> = OnceCell::new();
    let channel = SPAWNING_CHANNEL.get_or_init(|| {
        let (send, recv) = mpsc::channel::<SpawnInfo>();

        std::thread::Builder::new()
            .name("Pandora Command Spawner".to_string())
            .stack_size(128 * 1024)
            .spawn(|| {
                let mut context = SpawnContext::default();

                // Initialize COM on this thread. In my testing this wasn't needed, but it shouldn't hurt
                #[cfg(windows)]
                unsafe {
                    _ = windows::Win32::System::Com::CoInitializeEx(
                        None,
                        windows::Win32::System::Com::COINIT_APARTMENTTHREADED |  windows::Win32::System::Com::COINIT_DISABLE_OLE1DDE,
                    );
                }

                for info in recv {
                    _ = info.sender.send(handle_spawn(info.command, info.spawn_type, &mut context))
                }
            })
            .unwrap();

        send
    });
    channel.send(SpawnInfo { command, spawn_type, sender }).unwrap();

    receiver
}

fn handle_spawn(mut command: PandoraCommand, spawn_type: SpawnType, context: &mut SpawnContext) -> std::io::Result<PandoraChild> {
    let probe_minecraft = is_probe_minecraft_launch(&command);
    if probe_minecraft {
        probe_event("java_spawn", "begin", None);
    }

    let result = match spawn_type {
        SpawnType::Normal => {
            #[cfg(unix)]
            {
                crate::unix::unix_spawn::spawn(command, context)
            }
            #[cfg(windows)]
            {
                crate::windows::windows_spawn::spawn(command, context)
            }
        },
        SpawnType::Elevated => {
            command.stdin = crate::PandoraStdioWriteMode::Null;
            command.stdout = crate::PandoraStdioReadMode::Null;
            command.stderr = crate::PandoraStdioReadMode::Null;

            if command.inherit_env.is_some() || !command.env.is_empty() {
                Err(Error::new(ErrorKind::InvalidInput, "cannot set custom environment for elevated process"))
            } else {
                #[cfg(target_os = "linux")]
                {
                    crate::unix::linux::pkexec::spawn(command, context)
                }
                #[cfg(windows)]
                {
                    crate::windows::runas::spawn(command, context)
                }
                #[cfg(target_os = "macos")]
                {
                    crate::unix::macos::elevated::spawn(command)
                }
            }
        },
        SpawnType::Sandboxed(sandbox) => {
            #[cfg(target_os = "linux")]
            {
                crate::unix::linux::bwrap::spawn(command, sandbox, context)
            }

            #[cfg(windows)]
            {
                if sandbox.name.as_encoded_bytes().iter().any(|b| illegal_filename_char(*b)) {
                    Err(Error::new(ErrorKind::InvalidInput, "name contained illegal character"))
                } else {
                    crate::windows::appcontainer::spawn(command, sandbox, context)
                }
            }

            #[cfg(target_os = "macos")]
            {
                crate::unix::macos::sandbox::spawn(command, sandbox, context)
            }
        },
    };

    if probe_minecraft {
        probe_event("java_spawn", "end", Some(if result.is_ok() { "ok" } else { "error" }));
        if result.is_ok() {
            probe_event("launcher_pre_java", "end", Some("ok"));
            probe_java_to_menu_unobserved();
        }
    }
    result
}

fn probe_path() -> Option<&'static OsString> {
    static PATH: OnceCell<Option<OsString>> = OnceCell::new();
    PATH.get_or_init(|| std::env::var_os(LAUNCH_PROBE_ENV).filter(|v| !v.is_empty())).as_ref()
}

fn is_probe_minecraft_launch(command: &PandoraCommand) -> bool {
    probe_path().is_some()
        && command.args.iter().any(|arg| arg.0 == OsStr::new("com.moulberry.pandora.LaunchWrapper"))
}

fn probe_event(phase: &str, event: &str, outcome: Option<&str>) {
    let Some(path) = probe_path() else {
        return;
    };
    let now = monotonic_ns();
    let line = if let Some(outcome) = outcome {
        format!("{{\"schema\":\"{LAUNCH_PROBE_SCHEMA}\",\"mono_ns\":{now},\"phase\":\"{phase}\",\"event\":\"{event}\",\"network\":false,\"outcome\":\"{outcome}\"}}\n")
    } else {
        format!("{{\"schema\":\"{LAUNCH_PROBE_SCHEMA}\",\"mono_ns\":{now},\"phase\":\"{phase}\",\"event\":\"{event}\",\"network\":false}}\n")
    };
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(line.as_bytes());
    }
}

fn probe_java_to_menu_unobserved() {
    let Some(path) = probe_path() else {
        return;
    };
    let now = monotonic_ns();
    let line = format!(
        "{{\"schema\":\"{LAUNCH_PROBE_SCHEMA}\",\"mono_ns\":{now},\"phase\":\"java_to_menu\",\"event\":\"unobserved\",\"network\":false,\"observed\":false,\"duration_ns\":null}}\n"
    );
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(line.as_bytes());
    }
}

#[cfg(windows)]
fn monotonic_ns() -> u64 {
    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn QueryPerformanceCounter(value: *mut i64) -> i32;
        fn QueryPerformanceFrequency(value: *mut i64) -> i32;
    }
    let mut ticks = 0_i64;
    let mut frequency = 0_i64;
    unsafe {
        if QueryPerformanceCounter(&mut ticks) == 0 || QueryPerformanceFrequency(&mut frequency) == 0 || frequency <= 0 {
            return 0;
        }
    }
    ((ticks.max(0) as u128 * 1_000_000_000_u128) / frequency as u128).min(u64::MAX as u128) as u64
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn monotonic_ns() -> u64 {
    #[repr(C)]
    struct Timespec {
        tv_sec: i64,
        tv_nsec: i64,
    }
    unsafe extern "C" {
        fn clock_gettime(clock_id: i32, tp: *mut Timespec) -> i32;
    }
    #[cfg(target_os = "linux")]
    const CLOCK_MONOTONIC: i32 = 1;
    #[cfg(target_os = "macos")]
    const CLOCK_MONOTONIC: i32 = 6;

    let mut ts = Timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe {
        if clock_gettime(CLOCK_MONOTONIC, &mut ts) != 0 {
            return 0;
        }
    }
    (ts.tv_sec.max(0) as u128 * 1_000_000_000_u128 + ts.tv_nsec.max(0) as u128).min(u64::MAX as u128) as u64
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn monotonic_ns() -> u64 {
    use std::{sync::OnceLock, time::Instant};
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_nanos().min(u64::MAX as u128) as u64
}
