//! Opt-in aggregate attribution for `assets_verify_download`.
//!
//! This module is diagnostic only. It records one final aggregate snapshot per
//! asset launch and never persists asset paths, hashes, filenames, URLs or tokens.
//! When disabled, the stock SHA-1 path is used unchanged by the caller.

use std::{
    ffi::OsString,
    fs::File,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use serde::Serialize;
use sha1::{Digest, Sha1};

pub(crate) const ENV_NAME: &str = "BOOTOPTIM_ASSET_ATTRIBUTION";
const SCHEMA: &str = "bootoptim.asset_attribution.v1";

#[derive(Default)]
struct Counters {
    hash_attempts: AtomicU64,
    hash_hits: AtomicU64,
    hash_misses: AtomicU64,
    hash_io_errors: AtomicU64,
    hash_bytes_read: AtomicU64,
    hash_active: AtomicU64,
    hash_max_concurrency: AtomicU64,
    network_observed: AtomicBool,
    download_active: AtomicU64,
    download_max_concurrency: AtomicU64,
    downloaded_objects: AtomicU64,
    downloaded_bytes: AtomicU64,
    network_errors: AtomicU64,
    download_size_failures: AtomicU64,
    download_hash_failures: AtomicU64,
}

pub(crate) struct AssetAttributionProbe {
    output_path: PathBuf,
    planned_objects: u64,
    planned_bytes: u64,
    phase_begin_mono_ns: u64,
    launch_probe_active: bool,
    counters: Arc<Counters>,
    finished: AtomicBool,
}

#[derive(Serialize)]
struct Snapshot<'a> {
    schema: &'static str,
    outcome: &'a str,
    phase: &'static str,
    phase_begin_mono_ns: u64,
    snapshot_mono_ns: u64,
    correlation_clock: &'static str,
    launch_probe_active: bool,
    planned_objects: u64,
    planned_bytes: u64,
    hash_attempts: u64,
    hash_hits: u64,
    hash_misses: u64,
    hash_io_errors: u64,
    hash_bytes_read: u64,
    hash_max_concurrency: u64,
    downloaded_objects: u64,
    downloaded_bytes: u64,
    network_errors: u64,
    download_size_failures: u64,
    download_hash_failures: u64,
    any_network: bool,
    download_max_concurrency: u64,
    warm_ab_contaminated: bool,
    hash_wall_ns: Option<u64>,
    hash_cpu_ns: Option<u64>,
    hash_queue_wait_ns: Option<u64>,
    timing_observation: &'static str,
    error_snapshot_complete: bool,
}

struct CountingSha1Writer<'a> {
    hasher: &'a mut Sha1,
    bytes: u64,
}

impl Write for CountingSha1Writer<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.hasher.write(buf)?;
        self.bytes = self.bytes.saturating_add(written as u64);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.hasher.flush()
    }
}

pub(crate) struct DownloadGuard {
    counters: Arc<Counters>,
}

impl Drop for DownloadGuard {
    fn drop(&mut self) {
        self.counters.download_active.fetch_sub(1, Ordering::Relaxed);
    }
}

impl AssetAttributionProbe {
    pub(crate) fn for_launch(planned_objects: u64, planned_bytes: u64) -> Option<Arc<Self>> {
        Self::from_configured_path(
            configured_path(std::env::var_os(ENV_NAME)),
            planned_objects,
            planned_bytes,
            bridge::launch_probe::enabled(),
        )
    }

    fn from_configured_path(
        output_path: Option<PathBuf>,
        planned_objects: u64,
        planned_bytes: u64,
        launch_probe_active: bool,
    ) -> Option<Arc<Self>> {
        Some(Arc::new(Self {
            output_path: output_path?,
            planned_objects,
            planned_bytes,
            phase_begin_mono_ns: monotonic_ns(),
            launch_probe_active,
            counters: Arc::new(Counters::default()),
            finished: AtomicBool::new(false),
        }))
    }

    pub(crate) fn hash_path(&self, path: &Path, expected_hash: [u8; 20]) -> bool {
        self.counters.hash_attempts.fetch_add(1, Ordering::Relaxed);
        let active = self.counters.hash_active.fetch_add(1, Ordering::Relaxed) + 1;
        update_max(&self.counters.hash_max_concurrency, active);

        let mut bytes_read = 0_u64;
        let result = (|| -> io::Result<bool> {
            let mut file = File::open(path)?;
            let mut hasher = Sha1::new();
            {
                let mut writer = CountingSha1Writer {
                    hasher: &mut hasher,
                    bytes: 0,
                };
                let copy_result = io::copy(&mut file, &mut writer);
                bytes_read = writer.bytes;
                copy_result?;
            }
            let actual_hash: [u8; 20] = hasher.finalize().into();
            Ok(expected_hash == actual_hash)
        })();

        self.counters.hash_active.fetch_sub(1, Ordering::Relaxed);
        self.counters.hash_bytes_read.fetch_add(bytes_read, Ordering::Relaxed);

        match result {
            Ok(true) => {
                self.counters.hash_hits.fetch_add(1, Ordering::Relaxed);
                true
            },
            Ok(false) => {
                self.counters.hash_misses.fetch_add(1, Ordering::Relaxed);
                false
            },
            Err(_) => {
                self.counters.hash_io_errors.fetch_add(1, Ordering::Relaxed);
                self.counters.hash_misses.fetch_add(1, Ordering::Relaxed);
                false
            },
        }
    }

    pub(crate) fn begin_download(&self) -> DownloadGuard {
        self.counters.network_observed.store(true, Ordering::Relaxed);
        let active = self.counters.download_active.fetch_add(1, Ordering::Relaxed) + 1;
        update_max(&self.counters.download_max_concurrency, active);
        DownloadGuard {
            counters: Arc::clone(&self.counters),
        }
    }

    pub(crate) fn record_downloaded_body(&self, bytes: usize) {
        self.counters.downloaded_objects.fetch_add(1, Ordering::Relaxed);
        self.counters.downloaded_bytes.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    pub(crate) fn record_network_error(&self) {
        self.counters.network_errors.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_size_failure(&self) {
        self.counters.download_size_failures.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_download_hash_failure(&self) {
        self.counters.download_hash_failures.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn finish(&self, outcome: &'static str) {
        if self.finished.swap(true, Ordering::Relaxed) {
            return;
        }

        let any_network = self.counters.network_observed.load(Ordering::Relaxed);
        let hash_misses = self.counters.hash_misses.load(Ordering::Relaxed);
        let hash_io_errors = self.counters.hash_io_errors.load(Ordering::Relaxed);
        let downloaded_objects = self.counters.downloaded_objects.load(Ordering::Relaxed);
        let warm_ab_contaminated = any_network || hash_misses != 0 || hash_io_errors != 0 || downloaded_objects != 0;

        let snapshot = Snapshot {
            schema: SCHEMA,
            outcome,
            phase: "assets_verify_download",
            phase_begin_mono_ns: self.phase_begin_mono_ns,
            snapshot_mono_ns: monotonic_ns(),
            correlation_clock: monotonic_clock_name(),
            launch_probe_active: self.launch_probe_active,
            planned_objects: self.planned_objects,
            planned_bytes: self.planned_bytes,
            hash_attempts: self.counters.hash_attempts.load(Ordering::Relaxed),
            hash_hits: self.counters.hash_hits.load(Ordering::Relaxed),
            hash_misses,
            hash_io_errors,
            hash_bytes_read: self.counters.hash_bytes_read.load(Ordering::Relaxed),
            hash_max_concurrency: self.counters.hash_max_concurrency.load(Ordering::Relaxed),
            downloaded_objects,
            downloaded_bytes: self.counters.downloaded_bytes.load(Ordering::Relaxed),
            network_errors: self.counters.network_errors.load(Ordering::Relaxed),
            download_size_failures: self.counters.download_size_failures.load(Ordering::Relaxed),
            download_hash_failures: self.counters.download_hash_failures.load(Ordering::Relaxed),
            any_network,
            download_max_concurrency: self.counters.download_max_concurrency.load(Ordering::Relaxed),
            warm_ab_contaminated,
            hash_wall_ns: None,
            hash_cpu_ns: None,
            hash_queue_wait_ns: None,
            timing_observation: "unobserved: aggregate hash wall/cpu and semaphore wait would require per-object timing or scheduler instrumentation",
            error_snapshot_complete: outcome == "ok",
        };

        let Ok(mut bytes) = serde_json::to_vec(&snapshot) else {
            return;
        };
        bytes.push(b'\n');
        if let Some(parent) = self.output_path.parent().filter(|path| !path.as_os_str().is_empty()) {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&self.output_path, bytes);
    }
}

fn configured_path(value: Option<OsString>) -> Option<PathBuf> {
    value.filter(|value| !value.is_empty()).map(PathBuf::from)
}

fn update_max(max: &AtomicU64, value: u64) {
    let mut current = max.load(Ordering::Relaxed);
    while value > current {
        match max.compare_exchange_weak(current, value, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(observed) => current = observed,
        }
    }
}

#[cfg(windows)]
fn monotonic_clock_name() -> &'static str {
    "windows_qpc_ns"
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn monotonic_clock_name() -> &'static str {
    "clock_monotonic_ns"
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn monotonic_clock_name() -> &'static str {
    "unavailable"
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

    #[cfg(target_os = "linux")]
    const CLOCK_MONOTONIC: i32 = 1;
    #[cfg(target_os = "macos")]
    const CLOCK_MONOTONIC: i32 = 6;

    unsafe extern "C" {
        fn clock_gettime(clock_id: i32, tp: *mut Timespec) -> i32;
    }

    let mut value = Timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe {
        if clock_gettime(CLOCK_MONOTONIC, &mut value) != 0 {
            return 0;
        }
    }
    (value.tv_sec.max(0) as u128 * 1_000_000_000_u128 + value.tv_nsec.max(0) as u128).min(u64::MAX as u128) as u64
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn monotonic_ns() -> u64 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn temp_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("bootoptim-asset-probe-{label}-{}.json", std::process::id()))
    }

    fn expected_sha1(bytes: &[u8]) -> [u8; 20] {
        Sha1::digest(bytes).into()
    }

    fn read_snapshot(path: &Path) -> Value {
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    #[test]
    fn asset_attribution_disabled_creates_no_probe() {
        assert!(AssetAttributionProbe::from_configured_path(None, 1, 2, false).is_none());
    }

    #[test]
    fn asset_attribution_hit_has_no_network_and_is_warm_clean() {
        let data_path = temp_path("hit-data");
        let out_path = temp_path("hit-out");
        let _ = std::fs::remove_file(&out_path);
        std::fs::write(&data_path, b"asset-hit").unwrap();

        let probe = AssetAttributionProbe::from_configured_path(Some(out_path.clone()), 1, 9, true).unwrap();
        assert!(probe.hash_path(&data_path, expected_sha1(b"asset-hit")));
        probe.finish("ok");

        let snapshot = read_snapshot(&out_path);
        assert_eq!(snapshot["hash_attempts"], 1);
        assert_eq!(snapshot["hash_hits"], 1);
        assert_eq!(snapshot["hash_misses"], 0);
        assert_eq!(snapshot["hash_bytes_read"], 9);
        assert_eq!(snapshot["any_network"], false);
        assert_eq!(snapshot["warm_ab_contaminated"], false);
        assert_eq!(snapshot["hash_wall_ns"], Value::Null);

        let _ = std::fs::remove_file(data_path);
        let _ = std::fs::remove_file(out_path);
    }

    #[test]
    fn asset_attribution_miss_and_download_is_contaminated() {
        let data_path = temp_path("miss-data");
        let out_path = temp_path("miss-out");
        std::fs::write(&data_path, b"old").unwrap();

        let probe = AssetAttributionProbe::from_configured_path(Some(out_path.clone()), 1, 17, false).unwrap();
        assert!(!probe.hash_path(&data_path, expected_sha1(b"new")));
        {
            let _guard = probe.begin_download();
            probe.record_downloaded_body(17);
        }
        probe.finish("ok");

        let snapshot = read_snapshot(&out_path);
        assert_eq!(snapshot["hash_misses"], 1);
        assert_eq!(snapshot["downloaded_objects"], 1);
        assert_eq!(snapshot["downloaded_bytes"], 17);
        assert_eq!(snapshot["any_network"], true);
        assert_eq!(snapshot["warm_ab_contaminated"], true);

        let _ = std::fs::remove_file(data_path);
        let _ = std::fs::remove_file(out_path);
    }

    #[test]
    fn asset_attribution_hash_io_and_validation_errors_are_counted() {
        let data_path = temp_path("missing");
        let out_path = temp_path("error-out");
        let _ = std::fs::remove_file(&data_path);

        let probe = AssetAttributionProbe::from_configured_path(Some(out_path.clone()), 1, 4, false).unwrap();
        assert!(!probe.hash_path(&data_path, expected_sha1(b"data")));
        probe.record_network_error();
        probe.record_size_failure();
        probe.record_download_hash_failure();
        probe.finish("error");

        let snapshot = read_snapshot(&out_path);
        assert_eq!(snapshot["hash_io_errors"], 1);
        assert_eq!(snapshot["hash_misses"], 1);
        assert_eq!(snapshot["network_errors"], 1);
        assert_eq!(snapshot["download_size_failures"], 1);
        assert_eq!(snapshot["download_hash_failures"], 1);
        assert_eq!(snapshot["warm_ab_contaminated"], true);
        assert_eq!(snapshot["error_snapshot_complete"], false);

        let _ = std::fs::remove_file(out_path);
    }
}
