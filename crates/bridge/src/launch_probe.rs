use std::{
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    sync::OnceLock,
};

use parking_lot::Mutex;

const ENV_NAME: &str = "BOOTOPTIM_LAUNCH_PROBE";
const SCHEMA: &str = "bootoptim.launch_probe.v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TrackerKind {
    Parent,
    Assets,
    Libraries,
    JavaRuntime,
    Other,
}

#[derive(Clone, Copy, Debug)]
struct TrackerRecord {
    key: usize,
    kind: TrackerKind,
}

#[derive(Debug, Default)]
struct ProbeState {
    active: bool,
    modal_key: usize,
    config_done: bool,
    account_done: bool,
    prelaunch_done: bool,
    version_done: bool,
    assets_finished: bool,
    libraries_finished: bool,
    post_resolution_started: bool,
    trackers: Vec<TrackerRecord>,
}

static OUTPUT_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
static STATE: OnceLock<Mutex<ProbeState>> = OnceLock::new();

fn output_path() -> Option<&'static PathBuf> {
    OUTPUT_PATH
        .get_or_init(|| std::env::var_os(ENV_NAME).filter(|v| !v.is_empty()).map(PathBuf::from))
        .as_ref()
}

fn state() -> &'static Mutex<ProbeState> {
    STATE.get_or_init(|| Mutex::new(ProbeState::default()))
}

fn append_line(line: &str) {
    let Some(path) = output_path() else {
        return;
    };
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(line.as_bytes());
        let _ = file.write_all(b"\n");
    }
}

fn event_line(mono_ns: u64, phase: &str, event: &str, network: bool) -> String {
    format!(
        "{{\"schema\":\"{SCHEMA}\",\"mono_ns\":{mono_ns},\"phase\":\"{phase}\",\"event\":\"{event}\",\"network\":{network}}}"
    )
}

fn event(phase: &str, event_name: &str, network: bool) {
    append_line(&event_line(monotonic_ns(), phase, event_name, network));
}

fn outcome_event(phase: &str, outcome: &str) {
    append_line(&format!(
        "{{\"schema\":\"{SCHEMA}\",\"mono_ns\":{},\"phase\":\"{phase}\",\"event\":\"end\",\"network\":false,\"outcome\":\"{outcome}\"}}",
        monotonic_ns()
    ));
}

pub fn request(modal_key: usize) {
    let Some(path) = output_path() else {
        return;
    };

    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        let _ = std::fs::create_dir_all(parent);
    }
    let Ok(mut file) = OpenOptions::new().create(true).write(true).truncate(true).open(path) else {
        return;
    };

    let now = monotonic_ns();
    let _ = writeln!(file, "{}", event_line(now, "launch_request", "instant", false));
    let _ = writeln!(file, "{}", event_line(now, "instance_config", "begin", false));

    *state().lock() = ProbeState {
        active: true,
        modal_key,
        ..ProbeState::default()
    };
}

pub fn instance_config_loaded() {
    if output_path().is_none() {
        return;
    }
    let mut guard = state().lock();
    if !guard.active || guard.config_done {
        return;
    }
    guard.config_done = true;
    drop(guard);
    event("instance_config", "end", false);
    event("account_selection", "begin", false);
}

pub fn modal_clear(modal_key: usize) {
    if output_path().is_none() {
        return;
    }
    let mut guard = state().lock();
    if !guard.active || guard.modal_key != modal_key {
        return;
    }

    if !guard.account_done {
        guard.account_done = true;
        drop(guard);
        event("account_selection", "end", false);
        event("prelaunch", "begin", false);
        return;
    }

    if !guard.prelaunch_done {
        guard.prelaunch_done = true;
        drop(guard);
        event("prelaunch", "end", false);
        event("version_loader_resolution", "begin", false);
    }
}

pub fn tracker_created(modal_key: usize, tracker_key: usize, title: &str) {
    if output_path().is_none() {
        return;
    }
    let mut guard = state().lock();
    if !guard.active || guard.modal_key != modal_key {
        return;
    }

    let kind = tracker_kind(title);
    guard.trackers.push(TrackerRecord { key: tracker_key, kind });

    if kind == TrackerKind::Parent {
        return;
    }

    if matches!(kind, TrackerKind::Assets | TrackerKind::Libraries | TrackerKind::JavaRuntime) {
        if !guard.version_done {
            guard.version_done = true;
            drop(guard);
            event("version_loader_resolution", "end", false);
        } else {
            drop(guard);
        }
        event(phase_name(kind), "begin", title.starts_with("Downloading "));
    }
}

pub fn tracker_title_changed(modal_key: usize, tracker_key: usize, title: &str) {
    if output_path().is_none() {
        return;
    }
    let guard = state().lock();
    if !guard.active || guard.modal_key != modal_key {
        return;
    }
    let Some(record) = guard.trackers.iter().find(|v| v.key == tracker_key) else {
        return;
    };
    let kind = record.kind;
    drop(guard);

    if title.starts_with("Downloading ") && matches!(kind, TrackerKind::Assets | TrackerKind::Libraries | TrackerKind::JavaRuntime) {
        event(phase_name(kind), "network_download_begin", true);
    }
}

pub fn tracker_finished(modal_key: usize, tracker_key: usize, error: bool) {
    if output_path().is_none() {
        return;
    }
    let mut guard = state().lock();
    if !guard.active || guard.modal_key != modal_key {
        return;
    }
    let Some(record) = guard.trackers.iter().find(|v| v.key == tracker_key).copied() else {
        return;
    };

    if record.kind == TrackerKind::Parent {
        guard.active = false;
        drop(guard);
        append_line(&format!(
            "{{\"schema\":\"{SCHEMA}\",\"mono_ns\":{},\"phase\":\"java_to_menu\",\"event\":\"unobserved\",\"network\":false,\"observed\":false,\"duration_ns\":null}}",
            monotonic_ns()
        ));
        if error {
            outcome_event("launch", "error");
        }
        return;
    }

    match record.kind {
        TrackerKind::Assets => guard.assets_finished = true,
        TrackerKind::Libraries => guard.libraries_finished = true,
        _ => {}
    }
    drop(guard);

    if matches!(record.kind, TrackerKind::Assets | TrackerKind::Libraries | TrackerKind::JavaRuntime) {
        outcome_event(phase_name(record.kind), if error { "error" } else { "ok" });
    }
}

pub fn tracker_add_count(modal_key: usize, tracker_key: usize) {
    if output_path().is_none() {
        return;
    }
    let mut guard = state().lock();
    if !guard.active || guard.modal_key != modal_key || guard.post_resolution_started {
        return;
    }
    let Some(parent) = guard.trackers.iter().find(|v| v.key == tracker_key) else {
        return;
    };
    if parent.kind != TrackerKind::Parent {
        return;
    }

    if guard.assets_finished && guard.libraries_finished {
        guard.post_resolution_started = true;
        drop(guard);
        event("classpath_resolution", "inclusive_begin", false);
        event("native_extraction", "inclusive_begin", false);
    }
}

pub fn cancel(modal_key: usize) {
    if output_path().is_none() {
        return;
    }
    let mut guard = state().lock();
    if !guard.active || guard.modal_key != modal_key {
        return;
    }
    guard.active = false;
    drop(guard);
    outcome_event("launch", "cancelled");
}

pub fn error(modal_key: usize) {
    if output_path().is_none() {
        return;
    }
    let guard = state().lock();
    if !guard.active || guard.modal_key != modal_key {
        return;
    }
    drop(guard);
    outcome_event("launch", "error");
}

fn tracker_kind(title: &str) -> TrackerKind {
    match title {
        "Launching" => TrackerKind::Parent,
        "Verifying integrity of game assets" | "Downloading game assets" => TrackerKind::Assets,
        "Verifying integrity of game libraries" | "Downloading game libraries" => TrackerKind::Libraries,
        "Verifying integrity of Java Runtime" | "Downloading Java Runtime" => TrackerKind::JavaRuntime,
        _ => TrackerKind::Other,
    }
}

fn phase_name(kind: TrackerKind) -> &'static str {
    match kind {
        TrackerKind::Assets => "assets_verify_download",
        TrackerKind::Libraries => "libraries_classpath_inputs",
        TrackerKind::JavaRuntime => "java_runtime",
        TrackerKind::Parent => "launch",
        TrackerKind::Other => "other",
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
    use std::time::Instant;
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_nanos().min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_format_is_structured_and_path_free() {
        let line = event_line(123, "assets_verify_download", "begin", false);
        assert_eq!(line, "{\"schema\":\"bootoptim.launch_probe.v1\",\"mono_ns\":123,\"phase\":\"assets_verify_download\",\"event\":\"begin\",\"network\":false}");
        assert!(!line.contains('/'));
        assert!(!line.contains('\\'));
        assert!(!line.to_ascii_lowercase().contains("token"));
        assert!(!line.to_ascii_lowercase().contains("username"));
    }

    #[test]
    fn required_phase_order_contract_keeps_parallel_group_together() {
        fn rank(phase: &str) -> u8 {
            match phase {
                "launch_request" => 0,
                "instance_config" => 1,
                "account_selection" => 2,
                "prelaunch" => 3,
                "version_loader_resolution" => 4,
                "java_runtime" | "assets_verify_download" | "libraries_classpath_inputs" => 5,
                "classpath_resolution" | "native_extraction" => 6,
                "wrapper_arguments" => 7,
                "java_spawn" => 8,
                "launcher_pre_java" => 9,
                "java_to_menu" => 10,
                _ => 255,
            }
        }
        let serial = [
            "launch_request", "instance_config", "account_selection", "prelaunch",
            "version_loader_resolution", "assets_verify_download", "native_extraction",
            "wrapper_arguments", "java_spawn", "launcher_pre_java", "java_to_menu",
        ];
        assert!(serial.windows(2).all(|w| rank(w[0]) <= rank(w[1])));
        assert_eq!(rank("assets_verify_download"), rank("libraries_classpath_inputs"));
        assert_eq!(rank("assets_verify_download"), rank("java_runtime"));
        assert_eq!(rank("classpath_resolution"), rank("native_extraction"));
    }

    #[test]
    fn monotonic_clock_does_not_go_backwards() {
        let a = monotonic_ns();
        let b = monotonic_ns();
        assert!(b >= a);
    }
}
