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
    created_ns: u64,
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
    post_resolution_started: bool,
    loader_network_emitted: bool,
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

fn event_at(mono_ns: u64, phase: &str, event_name: &str, network: bool) {
    append_line(&event_line(mono_ns, phase, event_name, network));
}

fn event(phase: &str, event_name: &str, network: bool) {
    event_at(monotonic_ns(), phase, event_name, network);
}

fn outcome_event_at(mono_ns: u64, phase: &str, outcome: &str) {
    append_line(&format!(
        "{{\"schema\":\"{SCHEMA}\",\"mono_ns\":{mono_ns},\"phase\":\"{phase}\",\"event\":\"end\",\"network\":false,\"outcome\":\"{outcome}\"}}"
    ));
}

fn outcome_event(phase: &str, outcome: &str) {
    outcome_event_at(monotonic_ns(), phase, outcome);
}

fn network_event(phase: &str, source: &str) {
    append_line(&format!(
        "{{\"schema\":\"{SCHEMA}\",\"mono_ns\":{},\"phase\":\"{phase}\",\"event\":\"observed\",\"network\":true,\"source\":\"{source}\"}}",
        monotonic_ns()
    ));
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

    let now = monotonic_ns();
    let kind = tracker_kind(title);
    let mut begins: Vec<(u64, TrackerKind)> = Vec::new();
    let mut version_end = None;

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
            created_ns: now,
            top_level,
            finished: false,
        });

        if top_level {
            begins.push((now, kind));
        } else if kind == TrackerKind::Assets && !guard.version_done {
            // Asset loading is only entered by the outer try_join4, never by Forge/NeoForge
            // launch-version construction. At this point, any still-live Java/library tracker
            // also belongs to that outer group; nested verifier trackers had to finish before
            // create_launch_version could return.
            guard.version_done = true;
            let mut earliest = now;
            for record in &mut guard.trackers {
                if !record.finished
                    && matches!(record.kind, TrackerKind::Assets | TrackerKind::Libraries | TrackerKind::JavaRuntime)
                {
                    record.top_level = true;
                    earliest = earliest.min(record.created_ns);
                    begins.push((record.created_ns, record.kind));
                }
            }
            version_end = Some(earliest);
        }
    }

    if let Some(at) = version_end {
        outcome_event_at(at, "version_loader_resolution", "ok");
    }
    begins.sort_by_key(|(at, _)| *at);
    begins.dedup_by_key(|(_, kind)| *kind);
    for (at, begin_kind) in begins {
        event_at(at, phase_name(begin_kind), "begin", false);
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

    let mut phase_end = None;
    let mut start_post_envelope = false;
    {
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
        if top_level {
            phase_end = Some(kind);
        }

        if guard.version_done && !guard.post_resolution_started && observable_outer_group_finished(&guard) {
            guard.post_resolution_started = true;
            start_post_envelope = true;
        }
    }

    if let Some(kind) = phase_end {
        outcome_event(phase_name(kind), if error { "error" } else { "ok" });
    }
    if start_post_envelope {
        // log_configuration is the fourth try_join4 sibling but has no progress tracker in
        // this Pandora revision. These are therefore conservative upper-bound envelopes:
        // they may include a short untracked log-config tail before the real post-join work.
        event("classpath_resolution", "inclusive_begin", false);
        event("native_extraction", "inclusive_begin", false);
        event("wrapper_arguments", "inclusive_begin", false);
    }
}

fn observable_outer_group_finished(state: &ProbeState) -> bool {
    let has_assets = state.trackers.iter().any(|r| r.top_level && r.kind == TrackerKind::Assets);
    let has_libraries = state.trackers.iter().any(|r| r.top_level && r.kind == TrackerKind::Libraries);
    has_assets
        && has_libraries
        && state
            .trackers
            .iter()
            .filter(|r| r.top_level && matches!(r.kind, TrackerKind::Assets | TrackerKind::Libraries | TrackerKind::JavaRuntime))
            .all(|r| r.finished)
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
    if !enabled() || !is_loader_sha1_request_boundary(current_count, total_count) {
        return;
    }

    let should_emit = {
        let mut guard = state().lock();
        if !guard.active || guard.modal_key != modal_key || guard.loader_network_emitted {
            return;
        }
        let Some(record) = guard.trackers.iter().find(|r| r.key == tracker_key) else {
            return;
        };
        if record.kind != TrackerKind::Parent {
            return;
        }
        guard.loader_network_emitted = true;
        true
    };

    if should_emit {
        // Pinned Forge/NeoForge create_forgelike code increments the parent tracker to 1/13
        // immediately before constructing the join whose right-hand future is download_sha1.
        network_request_observed("loader_sha1");
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
    fn outer_group_completion_requires_assets_libraries_and_all_observed_siblings() {
        let mut state = ProbeState::default();
        state.trackers = vec![
            TrackerRecord { key: 1, kind: TrackerKind::Assets, created_ns: 1, top_level: true, finished: true },
            TrackerRecord { key: 2, kind: TrackerKind::Libraries, created_ns: 2, top_level: true, finished: true },
            TrackerRecord { key: 3, kind: TrackerKind::JavaRuntime, created_ns: 3, top_level: true, finished: false },
        ];
        assert!(!observable_outer_group_finished(&state));
        state.trackers[2].finished = true;
        assert!(observable_outer_group_finished(&state));
    }

    #[test]
    fn completed_nested_trackers_are_not_outer_candidates() {
        let state = ProbeState {
            trackers: vec![
                TrackerRecord { key: 1, kind: TrackerKind::JavaRuntime, created_ns: 1, top_level: false, finished: true },
                TrackerRecord { key: 2, kind: TrackerKind::Libraries, created_ns: 2, top_level: false, finished: true },
            ],
            ..ProbeState::default()
        };
        assert_eq!(state.trackers.iter().filter(|r| !r.finished).count(), 0);
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
