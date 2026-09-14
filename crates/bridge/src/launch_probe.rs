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
    top_level: bool,
    finished: bool,
}

#[derive(Debug, Default)]
struct ProbeState {
    active: bool,
    modal_key: usize,
    config_dispatch_ready: bool,
    config_done: bool,
    account_done: bool,
    prelaunch_done: bool,
    version_done: bool,
    loader_network_emitted: bool,
    trackers: Vec<TrackerRecord>,
}

static OUTPUT_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
static STATE: OnceLock<Mutex<ProbeState>> = OnceLock::new();
static WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

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

fn write_lock() -> &'static Mutex<()> {
    WRITE_LOCK.get_or_init(|| Mutex::new(()))
}

fn append_record(build: impl FnOnce(u64) -> String) {
    let Some(path) = output_path() else {
        return;
    };
    let _write_guard = write_lock().lock();
    let now = monotonic_ns();
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let mut record = build(now);
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
    append_record(|now| event_line(now, phase, event_name, network));
}

fn outcome_event(phase: &str, outcome: &str) {
    append_record(|now| {
        format!(
            "{{\"schema\":\"{SCHEMA}\",\"mono_ns\":{now},\"phase\":\"{phase}\",\"event\":\"end\",\"network\":false,\"outcome\":\"{outcome}\"}}"
        )
    });
}

fn network_event(phase: &str, source: &str) {
    append_record(|now| {
        format!(
            "{{\"schema\":\"{SCHEMA}\",\"mono_ns\":{now},\"phase\":\"{phase}\",\"event\":\"observed\",\"network\":true,\"source\":\"{source}\"}}"
        )
    });
}

pub fn network_download_observed(source: &'static str) {
    if !enabled() || !network_download_source_allowed(source) {
        return;
    }
    let guard = state().lock();
    if !guard.active {
        return;
    }
    drop(guard);
    network_event("network_download", source);
}

pub fn network_request_observed(source: &'static str) {
    if !enabled() || !network_request_source_allowed(source) {
        return;
    }
    let guard = state().lock();
    if !guard.active {
        return;
    }
    drop(guard);
    network_event("network_request", source);
}

fn network_download_source_allowed(source: &str) -> bool {
    matches!(source, "assets" | "libraries" | "java_runtime" | "tracked_other")
}

fn network_request_source_allowed(source: &str) -> bool {
    matches!(source, "account_login" | "loader_sha1")
}

pub fn request(modal_key: usize) {
    let Some(path) = output_path() else {
        return;
    };

    {
        let guard = state().lock();
        if guard.active {
            return;
        }
    }

    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        let _ = std::fs::create_dir_all(parent);
    }

    let _write_guard = write_lock().lock();
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
    let mut guard = state().lock();
    if !guard.active || guard.modal_key != modal_key {
        return;
    }
    guard.config_dispatch_ready = true;
}

pub fn instance_config_loaded() {
    if !enabled() {
        return;
    }
    let mut guard = state().lock();
    if !guard.active || guard.config_done || !guard.config_dispatch_ready {
        return;
    }
    guard.config_dispatch_ready = false;
    guard.config_done = true;
    drop(guard);
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

    let kind = tracker_kind(title);
    let mut begin_kinds = Vec::new();
    let mut end_version = false;

    {
        let mut guard = state().lock();
        if !guard.active || guard.modal_key != modal_key {
            return;
        }

        let top_level = guard.version_done
            && matches!(kind, TrackerKind::Assets | TrackerKind::Libraries | TrackerKind::JavaRuntime);
        guard.trackers.push(TrackerRecord {
            key: tracker_key,
            kind,
            top_level,
            finished: false,
        });

        if top_level {
            begin_kinds.push(kind);
        } else if kind == TrackerKind::Assets && !guard.version_done {
            // Safety fallback for an unexpected progress shape. Asset loading exists only
            // in the outer launch group. Do not backdate promoted sibling starts: emitting
            // them now keeps JSONL timestamps nondecreasing instead of fabricating history.
            guard.version_done = true;
            end_version = true;
            for record in &mut guard.trackers {
                if !record.finished
                    && matches!(record.kind, TrackerKind::Assets | TrackerKind::Libraries | TrackerKind::JavaRuntime)
                {
                    record.top_level = true;
                    begin_kinds.push(record.kind);
                }
            }
        }
    }

    if end_version {
        outcome_event("version_loader_resolution", "ok");
    }
    begin_kinds.sort_by_key(|kind| match kind {
        TrackerKind::JavaRuntime => 0,
        TrackerKind::Assets => 1,
        TrackerKind::Libraries => 2,
        TrackerKind::Parent | TrackerKind::Other => 3,
    });
    begin_kinds.dedup();
    for begin_kind in begin_kinds {
        event(phase_name(begin_kind), "begin", false);
    }

    if title == "Logging in" {
        network_request_observed("account_login");
    }
    if title.starts_with("Downloading ") {
        network_download_observed(network_source(kind).unwrap_or("tracked_other"));
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
    let source = network_source(record.kind).unwrap_or("tracked_other");
    drop(guard);
    network_download_observed(source);
}

pub fn tracker_finished(modal_key: usize, tracker_key: usize, error: bool) {
    if !enabled() {
        return;
    }

    let phase_end = {
        let mut guard = state().lock();
        if !guard.active || guard.modal_key != modal_key {
            return;
        }
        let Some(index) = guard.trackers.iter().position(|v| v.key == tracker_key) else {
            return;
        };

        let kind = guard.trackers[index].kind;
        let top_level = guard.trackers[index].top_level;
        guard.trackers[index].finished = true;

        if kind == TrackerKind::Parent {
            guard.active = false;
            return;
        }
        top_level.then_some(kind)
    };

    if let Some(kind) = phase_end {
        outcome_event(phase_name(kind), if error { "error" } else { "ok" });
    }
}

fn is_version_resolution_boundary(current_count: usize, total_count: usize) -> bool {
    total_count >= 5 && current_count == total_count - 5
}

fn is_loader_sha1_request_boundary(current_count: usize, total_count: usize) -> bool {
    current_count == 1 && total_count == 13
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

    let mut emit_loader_network = false;
    let mut end_version = false;

    {
        let mut guard = state().lock();
        if !guard.active || guard.modal_key != modal_key {
            return;
        }
        let Some(record) = guard.trackers.iter().find(|r| r.key == tracker_key) else {
            return;
        };
        if record.kind != TrackerKind::Parent {
            return;
        }

        if is_loader_sha1_request_boundary(current_count, total_count) && !guard.loader_network_emitted {
            guard.loader_network_emitted = true;
            emit_loader_network = true;
        }

        if !guard.version_done && is_version_resolution_boundary(current_count, total_count) {
            guard.version_done = true;
            end_version = true;
        }
    }

    if emit_loader_network {
        // Pinned Forge/NeoForge create_forgelike increments the parent tracker to 1/13
        // immediately before constructing the join containing the installer SHA-1 request.
        network_request_observed("loader_sha1");
    }
    if end_version {
        outcome_event("version_loader_resolution", "ok");
    }
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
    guard.config_dispatch_ready = false;
    drop(guard);
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
    guard.config_dispatch_ready = false;
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
    use std::collections::HashSet;

    use super::*;

    #[derive(Clone, Copy, Debug)]
    struct TraceEvent {
        mono_ns: u64,
        phase: &'static str,
        event: &'static str,
    }

    fn is_span_phase(phase: &str) -> bool {
        matches!(
            phase,
            "instance_config"
                | "account_selection"
                | "prelaunch"
                | "version_loader_resolution"
                | "java_runtime"
                | "assets_verify_download"
                | "libraries_classpath_inputs"
                | "appcds_preflight"
                | "java_spawn"
        )
    }

    fn validate_complete_trace(events: &[TraceEvent]) -> Result<(), &'static str> {
        let mut open = HashSet::new();
        let mut previous = None;
        let mut launcher_pre_java_ended = false;

        for item in events {
            if let Some(previous) = previous {
                if item.mono_ns < previous {
                    return Err("timestamp moved backwards");
                }
            }
            previous = Some(item.mono_ns);

            if launcher_pre_java_ended && matches!(item.event, "begin" | "inclusive_begin") {
                return Err("begin after launcher_pre_java.end");
            }

            if is_span_phase(item.phase) {
                match item.event {
                    "begin" | "inclusive_begin" => {
                        if !open.insert(item.phase) {
                            return Err("duplicate begin");
                        }
                    }
                    "end" | "inclusive_end" => {
                        if !open.remove(item.phase) {
                            return Err("span end without prior begin");
                        }
                    }
                    _ => {}
                }
            }

            if item.phase == "launcher_pre_java" && item.event == "end" {
                launcher_pre_java_ended = true;
            }
        }

        if !open.is_empty() {
            return Err("span left open");
        }
        Ok(())
    }

    fn complete_trace() -> Vec<TraceEvent> {
        vec![
            TraceEvent { mono_ns: 1, phase: "launch_request", event: "instant" },
            TraceEvent { mono_ns: 2, phase: "instance_config", event: "begin" },
            TraceEvent { mono_ns: 3, phase: "instance_config", event: "end" },
            TraceEvent { mono_ns: 4, phase: "account_selection", event: "begin" },
            TraceEvent { mono_ns: 5, phase: "network_request", event: "observed" },
            TraceEvent { mono_ns: 6, phase: "account_selection", event: "end" },
            TraceEvent { mono_ns: 7, phase: "prelaunch", event: "begin" },
            TraceEvent { mono_ns: 8, phase: "prelaunch", event: "end" },
            TraceEvent { mono_ns: 9, phase: "version_loader_resolution", event: "begin" },
            TraceEvent { mono_ns: 10, phase: "network_request", event: "observed" },
            TraceEvent { mono_ns: 11, phase: "version_loader_resolution", event: "end" },
            TraceEvent { mono_ns: 12, phase: "java_runtime", event: "begin" },
            TraceEvent { mono_ns: 13, phase: "assets_verify_download", event: "begin" },
            TraceEvent { mono_ns: 14, phase: "libraries_classpath_inputs", event: "begin" },
            TraceEvent { mono_ns: 15, phase: "network_download", event: "observed" },
            TraceEvent { mono_ns: 20, phase: "libraries_classpath_inputs", event: "end" },
            TraceEvent { mono_ns: 30, phase: "assets_verify_download", event: "end" },
            TraceEvent { mono_ns: 31, phase: "java_runtime", event: "end" },
            TraceEvent { mono_ns: 32, phase: "classpath_resolution", event: "unobserved" },
            TraceEvent { mono_ns: 33, phase: "native_extraction", event: "unobserved" },
            TraceEvent { mono_ns: 34, phase: "wrapper_arguments", event: "unobserved" },
            TraceEvent { mono_ns: 35, phase: "appcds_preflight", event: "begin" },
            TraceEvent { mono_ns: 36, phase: "appcds_preflight", event: "end" },
            TraceEvent { mono_ns: 37, phase: "java_spawn", event: "begin" },
            TraceEvent { mono_ns: 38, phase: "java_spawn", event: "end" },
            TraceEvent { mono_ns: 39, phase: "launcher_pre_java", event: "end" },
            TraceEvent { mono_ns: 40, phase: "java_to_menu", event: "unobserved" },
        ]
    }

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
                "appcds_preflight" => 7,
                "java_spawn" => 8,
                "launcher_pre_java" => 9,
                "java_to_menu" => 10,
                _ => 255,
            }
        }
        let serial = [
            "launch_request",
            "instance_config",
            "account_selection",
            "prelaunch",
            "version_loader_resolution",
            "assets_verify_download",
            "classpath_resolution",
            "appcds_preflight",
            "java_spawn",
            "launcher_pre_java",
            "java_to_menu",
        ];
        assert!(serial.windows(2).all(|w| rank(w[0]) <= rank(w[1])));
        assert_eq!(rank("assets_verify_download"), rank("libraries_classpath_inputs"));
        assert_eq!(rank("assets_verify_download"), rank("java_runtime"));
    }

    #[test]
    fn version_resolution_boundaries_cover_pinned_loader_shapes() {
        for (version_done, total) in [(2, 7), (5, 10), (8, 13)] {
            assert!(is_version_resolution_boundary(version_done, total));
            assert!(!is_version_resolution_boundary(version_done.saturating_sub(1), total));
        }
    }

    #[test]
    fn complete_trace_has_ordered_spans_and_monotonic_timestamps() {
        assert_eq!(validate_complete_trace(&complete_trace()), Ok(()));
    }

    #[test]
    fn complete_trace_rejects_end_without_begin() {
        let mut trace = complete_trace();
        trace.retain(|item| !(item.phase == "assets_verify_download" && item.event == "begin"));
        assert_eq!(validate_complete_trace(&trace), Err("span end without prior begin"));
    }

    #[test]
    fn complete_trace_rejects_begin_after_launcher_pre_java_end() {
        let mut trace = complete_trace();
        trace.push(TraceEvent { mono_ns: 41, phase: "java_spawn", event: "begin" });
        assert_eq!(validate_complete_trace(&trace), Err("begin after launcher_pre_java.end"));
    }

    #[test]
    fn complete_trace_rejects_timestamp_regression() {
        let mut trace = complete_trace();
        trace[12].mono_ns = 5;
        assert_eq!(validate_complete_trace(&trace), Err("timestamp moved backwards"));
    }

    #[test]
    fn loader_sha1_boundary_matches_pinned_forgelike_shape_only() {
        assert!(is_loader_sha1_request_boundary(1, 13));
        assert!(!is_loader_sha1_request_boundary(0, 13));
        assert!(!is_loader_sha1_request_boundary(1, 10));
        assert!(!is_loader_sha1_request_boundary(2, 13));
    }

    #[test]
    fn network_sources_are_fixed_and_non_sensitive() {
        for source in ["assets", "libraries", "java_runtime", "tracked_other"] {
            assert!(network_download_source_allowed(source));
        }
        for source in ["account_login", "loader_sha1"] {
            assert!(network_request_source_allowed(source));
        }
        assert!(!network_download_source_allowed("https://example.invalid/private"));
        assert!(!network_request_source_allowed("account-name"));
    }

    #[test]
    fn monotonic_clock_does_not_go_backwards() {
        let a = monotonic_ns();
        let b = monotonic_ns();
        assert!(b >= a);
    }
}
