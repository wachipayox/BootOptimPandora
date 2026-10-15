//! Applies the profile's distributed JVM options at Start (see `schema::profile_jvm`).
//!
//! Inputs are the effective options persisted in the instance's committed branch state, the
//! player's per-instance choices and facts about the JVM and machine. Nothing here contacts the
//! distribution service or walks the instance tree.

use std::{
    collections::HashMap,
    io::Write,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{LazyLock, Mutex},
    time::SystemTime,
};

use schema::{
    instance::InstanceConfiguration,
    profile_jvm::{self, JvmLaunchFacts, JvmPlanEntry, JvmPlanStatus, ManifestJvmOption},
};

/// Java runtime identity probed through the launch wrapper, cached per binary file state so a
/// custom Java costs one extra JVM start per launcher session, not one per Start.
/// Java seen at the last Start of each instance (by game directory), so the settings page can
/// show whether an option fits without starting a JVM.
static LAST_JAVA_BY_GAME_DIR: LazyLock<Mutex<HashMap<PathBuf, (Option<u32>, Option<String>)>>> =
    LazyLock::new(Default::default);

static JAVA_PROBE_CACHE: LazyLock<Mutex<HashMap<(PathBuf, u64, Option<SystemTime>), (u32, String)>>> =
    LazyLock::new(Default::default);

/// Where the JVM about to start comes from.
pub(crate) enum JavaRuntimeSource {
    /// Mojang's runtime for this Minecraft version: a Microsoft OpenJDK (HotSpot) build of the
    /// manifest's major version.
    Mojang { major_version: u32 },
    /// A player-selected or externally forced binary whose version and VM must be probed.
    External,
}

pub(crate) fn personal_jvm_args(configuration: &InstanceConfiguration) -> Vec<String> {
    match &configuration.jvm_flags {
        Some(flags) if flags.enabled => shell_words::split(&flags.flags)
            .unwrap_or_else(|_| flags.flags.split_whitespace().map(str::to_owned).collect()),
        _ => Vec::new(),
    }
}

/// Decides and logs the profile JVM arguments for one launch. Returns the arguments to place
/// before the player's memory setting and personal JVM arguments.
pub(crate) fn plan_launch_arguments(
    options: &[ManifestJvmOption],
    configuration: &InstanceConfiguration,
    java_binary: &Path,
    java_source: JavaRuntimeSource,
    launch_wrapper: &Path,
    game_dir: &Path,
) -> Vec<String> {
    if options.is_empty() {
        return Vec::new();
    }
    let personal = personal_jvm_args(configuration);
    let memory_enabled = configuration.memory.as_ref().is_some_and(|memory| memory.enabled);
    let (java_major, java_vm) = match java_source {
        JavaRuntimeSource::Mojang { major_version } => (Some(major_version), Some("hotspot".to_owned())),
        JavaRuntimeSource::External => match probe_java_runtime(java_binary, launch_wrapper) {
            Some((major, vm)) => (Some(major), Some(vm)),
            None => (None, None),
        },
    };
    if let Ok(mut last) = LAST_JAVA_BY_GAME_DIR.lock() {
        last.insert(game_dir.to_path_buf(), (java_major, java_vm.clone()));
    }
    let facts = machine_facts(&personal, java_major, java_vm);
    let plan = profile_jvm::plan(
        options,
        &configuration.profile_jvm_options,
        &facts,
        &profile_jvm::personal_flag_keys(&personal, memory_enabled),
    );
    log_plan(&plan, &facts);
    plan.into_iter()
        .filter_map(|entry| match entry.status {
            JvmPlanStatus::Applied { arg, .. } => Some(arg),
            _ => None,
        })
        .collect()
}

fn machine_facts(personal: &[String], java_major: Option<u32>, java_vm: Option<String>) -> JvmLaunchFacts {
    JvmLaunchFacts {
        platform: "pc".into(),
        java_major,
        java_vm,
        logical_cpus: profile_jvm::active_processor_count(personal)
            .or_else(|| std::thread::available_parallelism().ok().map(|count| count.get() as i64)),
        memory_mib: total_memory_mib(),
    }
}

/// The decisions the next Start would take, for the instance settings page. Does not start a JVM.
pub(crate) fn settings_view(
    options: Vec<ManifestJvmOption>,
    configuration: &InstanceConfiguration,
    game_dir: &Path,
) -> profile_jvm::ProfileJvmOptionsView {
    let personal = personal_jvm_args(configuration);
    let memory_enabled = configuration.memory.as_ref().is_some_and(|memory| memory.enabled);
    let (java_major, java_vm) = LAST_JAVA_BY_GAME_DIR
        .lock()
        .ok()
        .and_then(|last| last.get(game_dir).cloned())
        .unwrap_or((None, None));
    let facts = machine_facts(&personal, java_major, java_vm);
    let plan = profile_jvm::plan(
        &options,
        &configuration.profile_jvm_options,
        &facts,
        &profile_jvm::personal_flag_keys(&personal, memory_enabled),
    );
    profile_jvm::ProfileJvmOptionsView {
        options: options.into_iter().zip(plan).collect(),
        facts,
    }
}

/// One `[WBOMBO-COMPAT]` line per applied or skipped option, then one trace line with every
/// decision. Only option ids, values and machine facts are logged; no paths or player data.
fn log_plan(plan: &[JvmPlanEntry], facts: &JvmLaunchFacts) {
    let java = facts.java_label();
    let (mut applied, mut disabled, mut overridden, mut skipped, mut discarded) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for entry in plan {
        if entry.discarded_player_value {
            discarded.push(entry.id.clone());
        }
        match &entry.status {
            JvmPlanStatus::Applied { arg, source } => {
                log::info!(
                    "[WBOMBO-COMPAT] ACTIVE project=pandora mechanism=jvm-option/{} mod={java} arg={arg}",
                    entry.id
                );
                applied.push(format!("{}={arg}({source})", entry.id));
            },
            JvmPlanStatus::Disabled { source } => disabled.push(format!("{}({source})", entry.id)),
            JvmPlanStatus::Overridden { flag_key, .. } => overridden.push(format!("{}:{flag_key}", entry.id)),
            JvmPlanStatus::Skipped { reason } => {
                log::warn!(
                    "[WBOMBO-COMPAT] FALLBACK project=pandora mechanism=jvm-option/{} mod={java} reason={reason}",
                    entry.id
                );
                skipped.push(format!("{}:{reason}", entry.id));
            },
        }
    }
    log::info!(
        "[profile-jvm] {java} logical_cpus={} memory_mib={} applied=[{}] disabled=[{}] overridden_by_player=[{}] skipped=[{}] discarded_player_values=[{}]",
        facts.logical_cpus.map_or_else(|| "unknown".into(), |value| value.to_string()),
        facts.memory_mib.map_or_else(|| "unknown".into(), |value| value.to_string()),
        applied.join(" "),
        disabled.join(" "),
        overridden.join(" "),
        skipped.join(" "),
        discarded.join(" "),
    );
}

fn probe_java_runtime(binary: &Path, launch_wrapper: &Path) -> Option<(u32, String)> {
    let metadata = std::fs::metadata(binary).ok()?;
    let key = (binary.to_path_buf(), metadata.len(), metadata.modified().ok());
    if let Some(cached) = JAVA_PROBE_CACHE.lock().ok()?.get(&key) {
        return Some(cached.clone());
    }
    let mut command = std::process::Command::new(binary);
    command.arg("-jar").arg(launch_wrapper);
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut process = command.spawn().ok()?;
    let mut stdin = process.stdin.take()?;
    stdin
        .write_all(b"printproperty\njava.specification.version\nprintproperty\njava.vm.name\nexit\n")
        .ok()?;
    drop(stdin);
    let output = process.wait_with_output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let mut lines = text.lines().map(str::trim);
    let version = lines.next()?;
    let major = version.strip_prefix("1.").unwrap_or(version).parse::<u32>().ok()?;
    let vm = if lines.next()?.to_ascii_lowercase().contains("openj9") {
        "openj9"
    } else {
        "hotspot"
    };
    let result = (major, vm.to_owned());
    JAVA_PROBE_CACHE.lock().ok()?.insert(key, result.clone());
    Some(result)
}

#[cfg(windows)]
fn total_memory_mib() -> Option<i64> {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    unsafe { GlobalMemoryStatusEx(&mut status) }.ok()?;
    i64::try_from(status.ullTotalPhys / (1024 * 1024)).ok()
}

#[cfg(target_os = "linux")]
fn total_memory_mib() -> Option<i64> {
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = meminfo.lines().find(|line| line.starts_with("MemTotal:"))?;
    let kib = line.split_whitespace().nth(1)?.parse::<i64>().ok()?;
    Some(kib / 1024)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn total_memory_mib() -> Option<i64> {
    None
}
