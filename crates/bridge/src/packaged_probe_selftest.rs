use std::path::PathBuf;

const SELFTEST_ENV: &str = "BOOTOPTIM_PACKAGED_PROBE_SELFTEST_BRIDGE";
const PROBE_ENV: &str = "BOOTOPTIM_LAUNCH_PROBE";
const MODAL_KEY: usize = 0;
const PARENT_TRACKER_KEY: usize = 0xB007_0165;

pub(crate) fn finish_bridge_stage_if_requested() -> Option<Result<(), String>> {
    if std::env::var_os(SELFTEST_ENV).as_deref() != Some(std::ffi::OsStr::new("1")) {
        return None;
    }
    Some(run())
}

fn run() -> Result<(), String> {
    let path = std::env::var_os(PROBE_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| format!("{PROBE_ENV} must name a fresh JSONL file"))?;

    // `probe_request` has already called launch_probe::request(0), exactly as a
    // StartInstanceByName launch does. Drive the same bridge state machine far
    // enough to cover the historical post-join parent count that used to open
    // the invalid inclusive classpath/native/wrapper envelopes. With no managed
    // Java tracker, that old external/configured-Java shape was 5/7.
    crate::launch_probe::backend_dispatch(MODAL_KEY);
    crate::launch_probe::instance_config_loaded();
    crate::launch_probe::modal_clear(MODAL_KEY);
    crate::launch_probe::modal_clear(MODAL_KEY);
    crate::launch_probe::tracker_created(MODAL_KEY, PARENT_TRACKER_KEY, "Launching");
    crate::launch_probe::tracker_add_count(MODAL_KEY, PARENT_TRACKER_KEY, 2, 7);
    crate::launch_probe::tracker_add_count(MODAL_KEY, PARENT_TRACKER_KEY, 5, 7);

    let trace = std::fs::read_to_string(&path)
        .map_err(|error| format!("unable to read bridge-stage packaged probe trace: {error}"))?;
    let first = trace.lines().find(|line| !line.trim().is_empty())
        .ok_or_else(|| "bridge-stage packaged probe trace is empty".to_string())?;
    if !first.contains("\"phase\":\"launcher_pre_java\"")
        || !first.contains("\"event\":\"begin\"")
    {
        return Err("first record is not launcher_pre_java.begin".into());
    }
    if trace.contains("inclusive_begin") || trace.contains("inclusive_end") {
        return Err("legacy inclusive_* marker appeared during bridge stage".into());
    }

    println!("BOOTOPTIM_PACKAGED_PROBE_SELFTEST_BRIDGE=ok path={}", path.display());
    Ok(())
}
