use std::{
    cell::Cell,
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    sync::OnceLock,
};

use parking_lot::Mutex;

const ENV_NAME: &str = "BOOTOPTIM_LAUNCH_PROBE";
const SCHEMA: &str = "bootoptim.launch_probe.v1";

thread_local! {
    static BACKEND_DISPATCH_MODAL: Cell<Option<usize>> = const { Cell::new(None) };
}

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
    top_level: bool,
}

#[derive(Debug, Default)]
struct ProbeState {
    active: bool,
    modal_key: usize,
    config_done: bool,
    account_done: bool,
    prelaunch_done: bool,
    version_done: bool,
    post_resolution_started: bool,
    trackers: Vec<TrackerRecord>,
}

static OUTPUT_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
static STATE: OnceLock<Mutex<ProbeState>> = OnceLock::new();

pub fn enabled() -> bool {
    output_path().is_some()
}

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
        let mut record = String::with_capacity(line.len() + 1);
        record.push_str(line);
        record.push('\n');
        let _ = file.write_all(record.as_bytes());
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

fn network_event(source: &str) {
    append_line(&format!(
        "{{\"schema\":\"{SCHEMA}\",\"mono_ns\":{},\"phase\":\"network_download\",\"event\":\"observed\",\"network\":true,\"source\":\"{source}\"}}",
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
    let mut initial = String::new();
    initial.push_str(&event_line(now, "launch_request", "instant", false));
    initial.push('\n');
    initial.push_str(&event_line(now, "instance_config", "begin", false));
    initial.push('\n');
    if file.write_all(initial.as_bytes()).is_err() {
        return;
    }

    *state().lock() = ProbeState {
        active: true,
        modal_key,
        ..ProbeState::default()
    };
}

pub fn backend_dispatch(modal_key: usize) {
    if !enabled() {
        return;
    }
    let guard = state().lock();
    if !guard.active || guard.modal_key != modal_key {
        return;
    }
    drop(guard);
    BACKEND_DISPATCH_MODAL.with(|slot| slot.set(Some(modal_key)));
}

pub fn instance_config_loaded() {
    if !enabled() {
        return;
    }
    let dispatched = BACKEND_DISPATCH_MODAL.with(Cell::get);
    let mut guard = state().lock();
    if !guard.active || guard.config_done || dispatched != Some(guard.modal_key) {
        return;
    }
    guard.config_done = true;
    drop(guard);
    BACKEND_DISPATCH_MODAL.with(|slot| slot.set(None));
    event("instance_config", "end", false);
    event("account_selection", "begin", false);
}

pub fn modal_clear(modal_key: usize) {
    if !enabled() {
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
    if !enabled() {
        return;
    }
    let mut guard = state().lock();
    if !guard.active || guard.modal_key != modal_key {
        return;
    }

    let kind = tracker_kind(title);
    let top_level = guard.version_done
        && matches!(kind, TrackerKind::Assets | TrackerKind::Libraries | TrackerKind::JavaRuntime);
    guard.trackers.push(TrackerRecord {
        key: tracker_key,
        kind,
        top_level,
    });
    drop(guard);

    if top_level {
        event(phase_name(kind), "begin", false);
    }
    if title.starts_with("Downloading ") {
        if let Some(source) = network_source(kind) {
            network_event(source);
        }
    }
}

pub fn tracker_title_changed(modal_key: usize, tracker_key: usize, title: &str) {
    if !enabled() || !title.starts_with("Downloading ") {
        return;
    }
    let guard = state().lock();
    if !guard.active || guard.modal_key != modal_key {
        return;
    }
    let Some(record) = guard.trackers.iter().find(|v| v.key == tracker_key) else {
        return;
    };
    let source = network_source(record.kind);
    drop(guard);

    if let Some(source) = source {
        network_event(source);
    }
}

pub fn tracker_finished(modal_key: usize, tracker_key: usize, error: bool) {
    if !enabled() {
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
        return;
    }
    drop(guard);

    if record.top_level {
        outcome_event(phase_name(record.kind), if error { "error" } else { "ok" });
    }
}

pub fn tracker_add_count(
    modal_key: usize,
    tracker_key: usize,
    current_count: usize,
    total_count: usize,
) {
    if !enabled() {
        return;
    }
    let mut guard = state().lock();
    if !guard.active || guard.modal_key != modal_key {
        return;
    }
    let Some(parent) = guard.trackers.iter().find(|v| v.key == tracker_key) else {
        return;
    };
    if parent.kind != TrackerKind::Parent {
        return;
    }

    if !guard.version_done && is_version_boundary(current_count, total_count) {
        guard.version_done = true;
        drop(guard);
        outcome_event("version_loader_resolution", "ok");
        return;
    }

    if guard.version_done
        && !guard.post_resolution_started
        && is_post_resolution_boundary(current_count, total_count)
    {
        guard.post_resolution_started = true;
        drop(guard);
        event("classpath_resolution", "inclusive_begin", false);
        event("native_extraction", "inclusive_begin", false);
        event("wrapper_arguments", "inclusive_begin", false);
    }
}

fn is_version_boundary(current_count: usize, total_count: usize) -> bool {
    total_count >= 5 && current_count == total_count - 5
}

fn is_post_resolution_boundary(current_count: usize, total_count: usize) -> bool {
    total_count >= 1 && current_count == total_count - 1
}

pub fn cancel(modal_key: usize) {
    if !enabled() {
        return;
    }
    let mut guard = state().lock();
    if !guard.active || guard.modal_key != modal_key {
        return;
    }
    guard.active = false;
    drop(guard);
    BACKEND_DISPATCH_MODAL.with(|slot| slot.set(None));
    outcome_event("launch", "cancelled");
}

pub fn error(modal_key: usize) {
    if !enabled() {
        return;
    }
    let mut guard = state().lock();
    if !guard.active || guard.modal_key != modal_key {
        return;
    }
    guard.active = false;
    drop(guard);
    BACKEND_DISPATCH_MODAL.with(|slot| slot.set(None));
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

fn network_source(kind: TrackerKind) -> Option<&'static str> {
    match kind {
        TrackerKind::Assets => Some("assets"),
        TrackerKind::Libraries => Some("libraries"),
        TrackerKind::JavaRuntime => Some("java_runtime"),
        TrackerKind::Parent | TrackerKind::Other => None,
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
        assert!(!line.to_ascii_lowercase().contains("account"));
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
                "classpath_resolution" | "native_extraction" | "wrapper_arguments" => 6,
                "java_spawn" => 7,
                "launcher_pre_java" => 8,
                "java_to_menu" => 9,
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
        assert_eq!(rank("classpath_resolution"), rank("wrapper_arguments"));
    }

    #[test]
    fn parent_progress_boundaries_cover_supported_loader_shapes() {
        assert!(is_version_boundary(2, 7));
        assert!(is_version_boundary(5, 10));
        assert!(is_version_boundary(8, 13));
        assert!(!is_version_boundary(7, 13));
        assert!(is_post_resolution_boundary(6, 7));
        assert!(is_post_resolution_boundary(9, 10));
        assert!(is_post_resolution_boundary(12, 13));
    }

    #[test]
    fn network_source_is_fixed_metadata_only() {
        assert_eq!(network_source(TrackerKind::Assets), Some("assets"));
        assert_eq!(network_source(TrackerKind::Libraries), Some("libraries"));
        assert_eq!(network_source(TrackerKind::JavaRuntime), Some("java_runtime"));
        assert_eq!(network_source(TrackerKind::Other), None);
    }

    #[test]
    fn monotonic_clock_does_not_go_backwards() {
        let a = monotonic_ns();
        let b = monotonic_ns();
        assert!(b >= a);
    }
}
