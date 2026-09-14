use std::path::PathBuf;

const SELFTEST_ENV: &str = "BOOTOPTIM_PACKAGED_PROBE_SELFTEST";
const PROBE_ENV: &str = "BOOTOPTIM_LAUNCH_PROBE";
const MODAL_KEY: usize = 0;
const PARENT_TRACKER_KEY: usize = 0xB007_0165;

pub(crate) fn run_if_requested() -> Option<Result<(), String>> {
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
    let _ = std::fs::remove_file(&path);

    // Exercise the same bridge root state machine used by StartInstanceByName.
    crate::launch_probe::request(MODAL_KEY);
    crate::launch_probe::backend_dispatch(MODAL_KEY);
    crate::launch_probe::instance_config_loaded();
    crate::launch_probe::modal_clear(MODAL_KEY);
    crate::launch_probe::modal_clear(MODAL_KEY);
    crate::launch_probe::tracker_created(MODAL_KEY, PARENT_TRACKER_KEY, "Launching");
    crate::launch_probe::tracker_add_count(MODAL_KEY, PARENT_TRACKER_KEY, 2, 7);

    // Exercise command-side command-ready markers and the direct-Java/root tail.
    command::bootoptim_packaged_probe_contract_finalize_for_test();

    let trace = std::fs::read_to_string(&path)
        .map_err(|error| format!("unable to read packaged probe trace: {error}"))?;
    validate(&trace)?;
    println!("BOOTOPTIM_PACKAGED_PROBE_SELFTEST=ok path={}", path.display());
    Ok(())
}

fn has(line: &str, phase: &str, event: &str) -> bool {
    line.contains(&format!("\"phase\":\"{phase}\""))
        && line.contains(&format!("\"event\":\"{event}\""))
}

fn count(lines: &[&str], phase: &str, event: &str) -> usize {
    lines.iter().filter(|line| has(line, phase, event)).count()
}

fn position(lines: &[&str], phase: &str, event: &str) -> Option<usize> {
    lines.iter().position(|line| has(line, phase, event))
}

fn validate(trace: &str) -> Result<(), String> {
    let lines: Vec<_> = trace.lines().filter(|line| !line.trim().is_empty()).collect();
    let first = lines.first().ok_or_else(|| "packaged probe trace is empty".to_string())?;
    if !has(first, "launcher_pre_java", "begin") {
        return Err("first record is not launcher_pre_java.begin".into());
    }
    if trace.contains("inclusive_end") || trace.contains("inclusive_begin") {
        return Err("legacy inclusive_* marker present in packaged trace".into());
    }
    if count(&lines, "launcher_pre_java", "begin") != 1 || count(&lines, "launcher_pre_java", "end") != 1 {
        return Err("root begin/end must each occur exactly once".into());
    }
    if count(&lines, "java_spawn", "begin") != 1 || count(&lines, "java_spawn", "end") != 1 {
        return Err("java_spawn begin/end must each occur exactly once".into());
    }

    for phase in ["classpath_resolution", "native_extraction", "wrapper_arguments"] {
        let matching: Vec<_> = lines.iter().filter(|line| line.contains(&format!("\"phase\":\"{phase}\""))).collect();
        if matching.len() != 1 {
            return Err(format!("{phase} must occur exactly once"));
        }
        let line = matching[0];
        if !has(line, phase, "unobserved")
            || !line.contains("\"observed\":false")
            || !line.contains("\"duration_ns\":null")
        {
            return Err(format!("{phase} must be explicit unobserved"));
        }
    }

    let java_begin = position(&lines, "java_spawn", "begin").unwrap();
    let java_end = position(&lines, "java_spawn", "end").unwrap();
    let root_end = position(&lines, "launcher_pre_java", "end").unwrap();
    if !(java_begin < java_end && java_end < root_end) {
        return Err("java_spawn must close before launcher_pre_java.end".into());
    }

    let observed_phase = ["instance_config", "account_selection", "prelaunch", "version_loader_resolution"]
        .iter()
        .any(|phase| count(&lines, phase, "begin") == 1 && count(&lines, phase, "end") == 1);
    let explicit_unobserved = lines.iter().any(|line| {
        line.contains("\"event\":\"unobserved\"")
            && line.contains("\"observed\":false")
            && line.contains("\"duration_ns\":null")
    });
    if !observed_phase && !explicit_unobserved {
        return Err("trace contains neither a valid observed phase nor explicit unobserved phase".into());
    }

    Ok(())
}
