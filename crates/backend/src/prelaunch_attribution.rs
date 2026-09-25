use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use serde::Serialize;

pub(crate) const ENV_NAME: &str = "BOOTOPTIM_PRELAUNCH_ATTRIBUTION";
const SCHEMA: &str = "bootoptim.prelaunch_attribution.v1";
const LAUNCH_SCHEMA: &str = "bootoptim.launch_probe.v1";

static RUN_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Default, Serialize)]
pub(crate) struct SpanCounters {
    #[serde(flatten)]
    values: BTreeMap<&'static str, u64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    unobserved: Vec<&'static str>,
}

impl SpanCounters {
    pub(crate) fn insert(&mut self, key: &'static str, value: u64) {
        self.values.insert(key, value);
    }

    pub(crate) fn unobserved(&mut self, key: &'static str) {
        if !self.unobserved.contains(&key) {
            self.unobserved.push(key);
        }
    }
}

#[derive(Debug)]
pub(crate) struct SpanTimer {
    phase: &'static str,
    parent: Option<&'static str>,
    inclusive: bool,
    start_mono_ns: u64,
    start_cpu_ns: Option<u64>,
}

#[derive(Debug, Serialize)]
struct Correlation {
    launch_probe_schema: &'static str,
    launcher_pre_java_begin_mono_ns: Option<u64>,
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct SpanRecord {
    phase: &'static str,
    parent: Option<&'static str>,
    inclusive: bool,
    observation: &'static str,
    start_mono_ns: Option<u64>,
    end_mono_ns: Option<u64>,
    wall_ns: Option<u64>,
    cpu_ns: Option<u64>,
    cpu_observation: &'static str,
    counters: SpanCounters,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
}

#[derive(Debug, Serialize)]
struct AttributionReport {
    schema: &'static str,
    run_id: String,
    origin: &'static str,
    endpoint: &'static str,
    correlation: Correlation,
    spans: Vec<SpanRecord>,
    endpoint_mono_ns: Option<u64>,
    flush_context: Option<&'static str>,
    limitations: [&'static str; 5],
}

pub(crate) struct PrelaunchAttribution {
    output_path: Option<PathBuf>,
    report: Option<Mutex<AttributionReport>>,
}

impl PrelaunchAttribution {
    pub(crate) fn from_environment(modal_key: usize) -> Self {
        let output_path = configured_path(std::env::var_os(ENV_NAME));
        if output_path.is_none() {
            return Self::new(None, None);
        }

        let launch_root_mono_ns = bridge::launch_probe::active_root_begin_mono_ns(modal_key);
        Self::new(output_path, launch_root_mono_ns)
    }

    fn new(output_path: Option<PathBuf>, launch_root_mono_ns: Option<u64>) -> Self {
        if output_path.is_none() {
            return Self {
                output_path: None,
                report: None,
            };
        }

        let run_start = monotonic_ns();
        let sequence = RUN_COUNTER.fetch_add(1, Ordering::Relaxed);
        let run_id = format!("{:08x}-{:016x}-{:016x}", std::process::id(), run_start, sequence);

        Self {
            output_path,
            report: Some(Mutex::new(AttributionReport {
                schema: SCHEMA,
                run_id,
                origin: "BackendState::prelaunch.enter",
                endpoint: "BackendState::prelaunch.return",
                correlation: Correlation {
                    launch_probe_schema: LAUNCH_SCHEMA,
                    launcher_pre_java_begin_mono_ns: launch_root_mono_ns,
                    status: if launch_root_mono_ns.is_some() {
                        "observed"
                    } else {
                        "unobserved"
                    },
                },
                spans: Vec::new(),
                endpoint_mono_ns: None,
                flush_context: None,
                limitations: [
                    "Nested spans with inclusive=true are contained by their parent and must not be added to the parent wall time.",
                    "Bytes are emitted only when already exposed by the production operation; telemetry performs no extra filesystem walk or stat pass.",
                    "restore_prior records the legacy original_mods recovery check; on the persistent game-directory path it is a no-op.",
                    "Connector restore/merge after game stop is outside the Start-to-Java/prelaunch endpoint and is not converted into prelaunch time.",
                    "This sidecar does not observe Java-to-menu/TTMM and does not read Minecraft output.",
                ],
            })),
        }
    }

    pub(crate) fn begin(
        &self,
        phase: &'static str,
        parent: Option<&'static str>,
        inclusive: bool,
    ) -> Option<SpanTimer> {
        self.report.as_ref()?;
        Some(SpanTimer {
            phase,
            parent,
            inclusive,
            start_mono_ns: monotonic_ns(),
            start_cpu_ns: process_cpu_ns(),
        })
    }

    pub(crate) fn finish(&self, timer: Option<SpanTimer>, counters: SpanCounters) {
        let (Some(timer), Some(report)) = (timer, &self.report) else {
            return;
        };

        let end_mono_ns = monotonic_ns();
        let end_cpu_ns = process_cpu_ns();
        let cpu_ns = match (timer.start_cpu_ns, end_cpu_ns) {
            (Some(start), Some(end)) if end >= start => Some(end - start),
            _ => None,
        };

        report.lock().expect("prelaunch attribution mutex poisoned").spans.push(SpanRecord {
            phase: timer.phase,
            parent: timer.parent,
            inclusive: timer.inclusive,
            observation: "observed",
            start_mono_ns: Some(timer.start_mono_ns),
            end_mono_ns: Some(end_mono_ns),
            wall_ns: Some(end_mono_ns.saturating_sub(timer.start_mono_ns)),
            cpu_ns,
            cpu_observation: if cpu_ns.is_some() { "observed" } else { "unobserved" },
            counters,
            reason: None,
        });
    }

    pub(crate) fn record_unobserved(
        &self,
        phase: &'static str,
        parent: Option<&'static str>,
        inclusive: bool,
        reason: &'static str,
    ) {
        let Some(report) = &self.report else {
            return;
        };
        let mut report = report.lock().expect("prelaunch attribution mutex poisoned");
        if report.spans.iter().any(|span| span.phase == phase) {
            return;
        }
        report.spans.push(SpanRecord {
            phase,
            parent,
            inclusive,
            observation: "unobserved",
            start_mono_ns: None,
            end_mono_ns: None,
            wall_ns: None,
            cpu_ns: None,
            cpu_observation: "unobserved",
            counters: SpanCounters::default(),
            reason: Some(reason),
        });
    }

    pub(crate) fn complete_required_observations(&self) {
        for (phase, parent, inclusive, reason) in [
            (
                "restore_prior",
                Some("prelaunch"),
                false,
                "persistent game-directory setup exited before legacy original_mods recovery",
            ),
            (
                "load_content",
                Some("prelaunch"),
                false,
                "private launcher keeps .minecraft live and does not load Mods metadata during Start",
            ),
            (
                "mods_scan",
                Some("prelaunch"),
                false,
                "private launcher keeps .minecraft live and does not scan Mods entries during Start",
            ),
            (
                "apply_modpack_and_collect_mods",
                Some("prelaunch"),
                false,
                "private launcher installs its own pack layout and does not expand third-party modpacks during Start",
            ),
            (
                "rotate_mods_to_original_mods",
                Some("prelaunch"),
                false,
                "private launcher does not rotate mods into original_mods",
            ),
            (
                "create_mods_dir",
                Some("prelaunch"),
                false,
                "private launcher uses the existing instance mods directory",
            ),
            (
                "apply_copies_to_mods_dir",
                Some("prelaunch"),
                false,
                "private launcher does not materialize a copied Mods tree during Start",
            ),
            (
                "connector_cache_copy",
                Some("prelaunch"),
                false,
                "the persistent game directory does not copy or merge .connector during Start",
            ),
            (
                "extras_copy",
                Some("prelaunch"),
                false,
                "the persistent game directory does not copy extra Mods entries during Start",
            ),
            (
                "modpack_config_yosbr",
                Some("apply_modpack_and_collect_mods"),
                true,
                "private launcher does not evaluate third-party modpack overrides during Start",
            ),
            (
                "modpack_extra_file",
                Some("apply_modpack_and_collect_mods"),
                true,
                "private launcher does not apply third-party modpack extras during Start",
            ),
        ] {
            self.record_unobserved(phase, parent, inclusive, reason);
        }
    }

    pub(crate) fn mark_endpoint(&self) {
        let Some(report) = &self.report else {
            return;
        };
        report.lock().expect("prelaunch attribution mutex poisoned").endpoint_mono_ns = Some(monotonic_ns());
    }

    pub(crate) fn write_sidecar(&self, flush_context: &'static str) {
        let (Some(path), Some(report)) = (&self.output_path, &self.report) else {
            return;
        };

        let bytes = {
            let mut report = report.lock().expect("prelaunch attribution mutex poisoned");
            report.flush_context = Some(flush_context);
            match serde_json::to_vec_pretty(&*report) {
                Ok(bytes) => bytes,
                Err(err) => {
                    log::warn!("Unable to serialize BootOptim prelaunch attribution: {err}");
                    return;
                },
            }
        };

        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            if let Err(err) = std::fs::create_dir_all(parent) {
                log::warn!("Unable to create BootOptim prelaunch attribution directory: {err}");
                return;
            }
        }

        if let Err(err) = std::fs::write(path, bytes) {
            log::warn!("Unable to write BootOptim prelaunch attribution sidecar: {err}");
        }
    }
}

fn configured_path(value: Option<OsString>) -> Option<PathBuf> {
    value.filter(|value| !value.is_empty()).map(PathBuf::from)
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
        if QueryPerformanceCounter(&mut ticks) == 0 || QueryPerformanceFrequency(&mut frequency) == 0 || frequency <= 0
        {
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
    use std::sync::OnceLock;
    use std::time::Instant;

    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_nanos().min(u64::MAX as u128) as u64
}

#[cfg(windows)]
fn process_cpu_ns() -> Option<u64> {
    #[repr(C)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn GetProcessTimes(
            process: isize,
            creation_time: *mut FileTime,
            exit_time: *mut FileTime,
            kernel_time: *mut FileTime,
            user_time: *mut FileTime,
        ) -> i32;
    }

    let mut creation = FileTime { low: 0, high: 0 };
    let mut exit = FileTime { low: 0, high: 0 };
    let mut kernel = FileTime { low: 0, high: 0 };
    let mut user = FileTime { low: 0, high: 0 };
    let ok = unsafe { GetProcessTimes(GetCurrentProcess(), &mut creation, &mut exit, &mut kernel, &mut user) };
    if ok == 0 {
        return None;
    }

    let ticks = |time: FileTime| ((time.high as u64) << 32) | time.low as u64;
    Some(ticks(kernel).saturating_add(ticks(user)).saturating_mul(100))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn process_cpu_ns() -> Option<u64> {
    #[repr(C)]
    struct Timespec {
        tv_sec: i64,
        tv_nsec: i64,
    }
    unsafe extern "C" {
        fn clock_gettime(clock_id: i32, tp: *mut Timespec) -> i32;
    }
    #[cfg(target_os = "linux")]
    const CLOCK_PROCESS_CPUTIME_ID: i32 = 2;
    #[cfg(target_os = "macos")]
    const CLOCK_PROCESS_CPUTIME_ID: i32 = 12;

    let mut ts = Timespec { tv_sec: 0, tv_nsec: 0 };
    if unsafe { clock_gettime(CLOCK_PROCESS_CPUTIME_ID, &mut ts) } != 0 {
        return None;
    }
    Some((ts.tv_sec.max(0) as u128 * 1_000_000_000_u128 + ts.tv_nsec.max(0) as u128).min(u64::MAX as u128) as u64)
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn process_cpu_ns() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "bootoptim-agent180-{name}-{}-{}.json",
            std::process::id(),
            RUN_COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn sidecar_serialization_preserves_monotonic_span_and_unobserved_invariants() {
        let path = unique_path("schema");
        let _ = std::fs::remove_file(&path);
        let probe = PrelaunchAttribution::new(Some(path.clone()), Some(123_456));
        let span = probe.begin("load_content", Some("prelaunch"), false);
        let mut counters = SpanCounters::default();
        counters.insert("mods_loaded", 7);
        probe.finish(span, counters);
        probe.record_unobserved("restore_prior", Some("prelaunch"), false, "not on measured path");
        probe.mark_endpoint();
        probe.write_sidecar("test");

        let bytes = std::fs::read(&path).expect("sidecar should exist");
        let json: serde_json::Value = serde_json::from_slice(&bytes).expect("valid json");
        assert_eq!(json["schema"], SCHEMA);
        assert_eq!(json["correlation"]["launcher_pre_java_begin_mono_ns"], 123_456);
        let spans = json["spans"].as_array().expect("spans array");
        let observed = spans.iter().find(|span| span["phase"] == "load_content").expect("load_content span");
        let start = observed["start_mono_ns"].as_u64().expect("start");
        let end = observed["end_mono_ns"].as_u64().expect("end");
        assert!(end >= start);
        assert_eq!(observed["wall_ns"].as_u64(), Some(end - start));
        assert_eq!(observed["counters"]["mods_loaded"], 7);
        let restore = spans.iter().find(|span| span["phase"] == "restore_prior").expect("restore span");
        assert_eq!(restore["observation"], "unobserved");
        assert!(restore["wall_ns"].is_null());

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn disabled_probe_is_functionally_passthrough_and_creates_no_sidecar() {
        let path = unique_path("disabled");
        let _ = std::fs::remove_file(&path);
        let probe = PrelaunchAttribution::new(None, None);

        let result = {
            let span = probe.begin("functional_result", None, false);
            let value = 40 + 2;
            probe.finish(span, SpanCounters::default());
            value
        };
        probe.complete_required_observations();
        probe.mark_endpoint();
        probe.write_sidecar("test-disabled");

        assert_eq!(result, 42);
        assert!(!path.exists());
        assert!(probe.report.is_none());
    }

    #[test]
    fn empty_environment_value_disables_probe() {
        assert!(configured_path(None).is_none());
        assert!(configured_path(Some(OsString::new())).is_none());
    }
}
