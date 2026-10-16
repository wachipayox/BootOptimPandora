//! JDK 25 AOT cache for Wachiland global-profile instances on PC.
//!
//! The cache (JEP 483/514) is built in **two steps**:
//!
//! 1. *Record*, during a normal game session: the first Start of an instance runs with
//!    `-XX:AOTMode=record -XX:AOTConfiguration=<profile>`. When the game closes, the JVM only
//!    writes the training profile (about 300 MB) and exits.
//! 2. *Create*, a launcher job: the launcher runs the same JVM with the same VM arguments and
//!    `-XX:AOTMode=create`, which turns the profile into the cache. If the job is cancelled or the
//!    launcher closes, the profile stays and the job resumes the next time the launcher opens or
//!    after the next Start, without training again.
//!
//! Later Starts run with `-XX:AOTCache=<file>` and reach the main menu faster.
//!
//! Layout, inside the instance directory so that deleting the instance deletes its cache and two
//! instances never share one:
//!
//! ```text
//! <instance>/aot-cache/
//!   state.json         the valid cache: key, file name, size (written last, atomically)
//!   <key16>.aot        the valid cache file (promoted by rename after a successful create)
//!   timings.json       sizes and durations of the last record/create (progress estimate only)
//!   training/          run.json + train-<nonce>.aotconf (+ cache-<nonce>.aot while creating)
//! ```
//!
//! Rules this module keeps:
//! * A cache is used only if `state.json` names the **current key** and the file has the recorded
//!   size. The key covers the exact JDK build, the applied profile revision, the profile's mod
//!   list, the classpath jars (name, size, mtime: the JVM checks the same) and the VM arguments.
//!   Any change retrains only that instance.
//! * A profile is kept only if the game exited with code 0 after writing it; `run.json` then
//!   records its size. A profile that is missing, has another size or belongs to another key is
//!   discarded and the next Start records again.
//! * The cache output never has the cache name. It is promoted by rename only after `create`
//!   exited with code 0 and the file is plausible.
//! * The JVM runs in `AOTMode=auto`: a missing, truncated or mismatching cache is ignored with a
//!   message, never an error. If a run with an AOT flag still exits with an error in the first
//!   [`java_default::EARLY_EXIT_WINDOW`], the cache is dropped and the Start is retried without
//!   it; if that retry starts fine, the backend turns the instance's option off.
//! * Everything falls back to a plain start. The log lines use the fixed `[WBOMBO-COMPAT]` prefix.

use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use bridge::message::QuickPlayLaunch;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::java_default;
use crate::profile_branch::{GlobalRevisionPin, ProfileEntryMetadata};

/// Mechanism id in the `[WBOMBO-COMPAT]` log lines.
pub(crate) const MECHANISM: &str = "jdk25-aot-cache";
pub(crate) const DIR_NAME: &str = "aot-cache";
const STATE_FILE: &str = "state.json";
const TIMINGS_FILE: &str = "timings.json";
/// Left by the one-step version of this feature; removed when seen.
const LEGACY_FAILURE_FILE: &str = "failure.json";
const TRAINING_DIR: &str = "training";
const RUN_FILE: &str = "run.json";
const SCHEMA: u32 = 2;
/// A real cache or profile of this pack is about 300 MB; anything under this is not finished.
const MIN_CACHE_BYTES: u64 = 1024 * 1024;
/// Free space needed before a training run: the profile and the cache, with a margin.
pub(crate) const MIN_FREE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Set to `off` to disable the mechanism (developer / support switch). `force-error` adds an
/// invalid AOT flag so the JVM refuses to start: it exercises the automatic disable.
const ENV_SWITCH: &str = "WBOMBO_AOT_CACHE";
/// What progress estimates assume before this instance has been measured once (fast PC, Beta).
const DEFAULT_PROFILE_BYTES: u64 = 305 * 1024 * 1024;
const DEFAULT_RECORD_SECS: f32 = 17.0;
const DEFAULT_CREATE_SECS: f32 = 15.0;

/// Why the cache is not used or trained for this Start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Disabled {
    /// Not applicable (another JDK, a manual choice that is not a problem): no log line.
    Silent(&'static str),
    /// The feature would apply but cannot: a `[WBOMBO-COMPAT] FALLBACK` line is logged.
    Logged(String),
}

/// What the Start knows about the instance and its profile before the VM arguments are final.
#[derive(Debug)]
pub struct LaunchRequest {
    pub instance_root: PathBuf,
    pub pin: GlobalRevisionPin,
    /// [`mods_fingerprint`] of the committed profile entries (no filesystem scan).
    pub mods_fingerprint: String,
    /// Another process of this instance is alive (a game or a `create` job): do not train.
    pub other_process_running: bool,
    /// This Start is the retry after a failed cache run: no cache, no training.
    pub skip_once: bool,
    /// The player turned "Optimize launch" off (or the launcher did): no cache, and the instance's
    /// cache directory is removed.
    pub disabled_by_setting: bool,
    /// Kept so a retry reproduces what the player asked for.
    pub quick_play: Option<QuickPlayLaunch>,
}

/// Facts known only when the launch command is assembled.
pub(crate) struct Facts<'a> {
    pub java_path: &'a Path,
    pub java_major: u32,
    pub classpath: &'a [PathBuf],
    /// Every VM argument before the main class, as the JVM will see them (without the AOT flags).
    pub jvm_args: Vec<OsString>,
    /// Working directory of the game; the `create` job runs there too.
    pub game_dir: &'a Path,
    /// `PATH` given to the game (natives directory appended).
    pub path_env: Option<OsString>,
    pub jvm_binary_enabled: bool,
    pub force_external_java: bool,
    pub sandbox: bool,
    pub wrapper_command: bool,
}

#[derive(Debug)]
pub enum RunKind {
    /// Started with `-XX:AOTCache`.
    Use,
    /// Started with `-XX:AOTMode=record`; the JVM writes the training profile to `profile` when
    /// the game closes.
    Record { profile: PathBuf },
}

/// A game process started with an AOT flag.
#[derive(Debug)]
pub struct ActiveRun {
    pub kind: RunKind,
    pub dir: PathBuf,
    pub key: String,
    pub inputs: KeyInputs,
    pub java_version: String,
    pub started: Instant,
    pub quick_play: Option<QuickPlayLaunch>,
    /// The player's Stop was already given extra time to let the JVM finish writing the profile.
    pub assembly_grace_used: bool,
    /// The player's Stop on a record run was already given time for the game to close normally.
    pub shutdown_grace_used: bool,
    /// When the launcher first saw that the game closed and the JVM went on to the profile.
    pub dump_started: Option<Instant>,
    /// The game showed a window at some point (so its disappearance means the game closed).
    pub had_window: bool,
}

pub(crate) struct Prepared {
    /// VM arguments to add before the main class.
    pub flags: Vec<String>,
    pub run: Option<ActiveRun>,
}

impl Prepared {
    fn none() -> Self {
        Self {
            flags: Vec::new(),
            run: None,
        }
    }
}

/// How a tracked process ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExitKind {
    /// The process ended by itself (or died) with this code.
    Exited(Option<i32>),
    /// The player asked the launcher to close or kill it. `Some(code)` if the process then
    /// ended by itself with that code, `None` if it was killed or its end was not observed.
    Stopped(Option<i32>),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ExitAction {
    Nothing,
    /// A record run wrote a complete profile: the `create` job can run.
    Recorded,
    /// A record run produced nothing usable; it was discarded.
    Discarded,
    /// The run failed early with an AOT flag: it was dropped, start again without it.
    RetryWithoutCache {
        reason: String,
    },
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct CacheState {
    schema: u32,
    key: String,
    file: String,
    size: u64,
    created_unix_secs: u64,
    jdk: String,
    inputs: KeyInputs,
}

/// `training/run.json`: what is needed to finish the training without the game.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct RunRecord {
    schema: u32,
    key: String,
    /// The profile file name inside `training/`.
    file: String,
    started_unix_secs: u64,
    java_version: String,
    inputs: KeyInputs,
    /// The game's JVM and VM arguments (everything before the main class, no AOT flags).
    java: String,
    args: Vec<String>,
    cwd: String,
    path_env: Option<String>,
    /// Set once the game exited with code 0 after writing the profile.
    recorded: Option<Recorded>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Recorded {
    size: u64,
    unix_secs: u64,
}

/// Measured sizes and durations, kept across keys; used only to estimate progress.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub(crate) struct Timings {
    pub profile_bytes: Option<u64>,
    pub record_secs: Option<f32>,
    pub create_secs: Option<f32>,
}

/// What the key is made of, kept readable in `state.json` to explain an invalidation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeyInputs {
    pub jdk: String,
    pub profile_id: String,
    pub revision_id: String,
    pub manifest_sha256: String,
    pub mods: String,
    pub classpath: String,
    pub jvm_args: String,
}

impl KeyInputs {
    pub(crate) fn key(&self) -> String {
        let mut hasher = Sha256::new();
        for part in [
            "wbombo-aot-key-v1",
            &self.jdk,
            &self.profile_id,
            &self.revision_id,
            &self.manifest_sha256,
            &self.mods,
            &self.classpath,
            &self.jvm_args,
        ] {
            hasher.update((part.len() as u64).to_le_bytes());
            hasher.update(part.as_bytes());
        }
        hex::encode(hasher.finalize())
    }
}

fn short(key: &str) -> &str {
    &key[..16.min(key.len())]
}

fn unix_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs())
}

// ---- key inputs ------------------------------------------------------------------------------

/// The exact JDK build: a hash of the runtime's `release` file plus the size of `lib/modules`,
/// and the runtime version for the log. `None` when the runtime cannot be identified.
pub(crate) fn jdk_identity(java_path: &Path) -> Option<(String, String)> {
    let home = java_path.parent()?.parent()?;
    let release = fs::read(home.join("release")).ok()?;
    let text = String::from_utf8_lossy(&release);
    let value = |name: &str| {
        text.lines().find_map(|line| {
            let rest = line.strip_prefix(name)?.strip_prefix('=')?;
            Some(rest.trim().trim_matches('"').to_owned())
        })
    };
    let version = value("JAVA_RUNTIME_VERSION").or_else(|| value("JAVA_VERSION"))?;
    let modules_len = fs::metadata(home.join("lib").join("modules")).ok()?.len();
    let mut hasher = Sha256::new();
    hasher.update(&release);
    hasher.update(modules_len.to_le_bytes());
    Some((version, hex::encode(hasher.finalize())))
}

/// Major version of a `release` runtime version (`25.0.1+8-LTS` → 25).
pub(crate) fn runtime_major(version: &str) -> Option<u32> {
    version.split(['.', '+', '-']).next()?.parse().ok()
}

/// Hash of the profile's mod files (path and content hash) from the committed branch state. No
/// filesystem access: the profile layout already stores the hash of every managed file.
pub(crate) fn mods_fingerprint(entries: &BTreeMap<String, ProfileEntryMetadata>) -> String {
    let mut hasher = Sha256::new();
    for (path, entry) in entries {
        if path.starts_with("mods/") {
            hasher.update(path.as_bytes());
            hasher.update([0]);
            hasher.update(entry.source_sha256.as_bytes());
            hasher.update([0]);
        }
    }
    hex::encode(hasher.finalize())
}

/// Hash of the classpath jars by path, size and modification time: the JVM refuses a cache
/// when a jar's size or mtime differs from training. `None` if one cannot be read.
pub(crate) fn classpath_fingerprint(classpath: &[PathBuf]) -> Option<String> {
    let mut hasher = Sha256::new();
    for path in classpath {
        let metadata = fs::metadata(path).ok()?;
        let modified = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_nanos();
        hasher.update(path.as_os_str().as_encoded_bytes());
        hasher.update([0]);
        hasher.update(metadata.len().to_le_bytes());
        hasher.update(modified.to_le_bytes());
    }
    Some(hex::encode(hasher.finalize()))
}

/// Hash of the VM arguments that shape the VM and the loaded classes. `-D` properties and the
/// cache's own flags are left out: they do not change what the JVM validates, and the
/// launcher version appears in a `-D` property.
pub(crate) fn jvm_args_fingerprint(args: &[OsString]) -> String {
    let mut hasher = Sha256::new();
    for arg in args {
        let bytes = arg.as_encoded_bytes();
        if bytes.starts_with(b"-D") || is_aot_flag(arg) {
            continue;
        }
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    }
    hex::encode(hasher.finalize())
}

/// Flags that select or conflict with another class-data archive. If the player sets one, ours
/// stays out of the way.
fn is_aot_flag(arg: &OsStr) -> bool {
    let Some(arg) = arg.to_str() else {
        return false;
    };
    [
        "-XX:AOT",
        "-Xshare",
        "-XX:SharedArchiveFile",
        "-XX:ArchiveClassesAtExit",
        "-XX:+AutoCreateSharedArchive",
    ]
    .iter()
    .any(|prefix| arg.starts_with(prefix))
}

// ---- decision --------------------------------------------------------------------------------

pub(crate) struct Dir {
    root: PathBuf,
}

impl Dir {
    pub(crate) fn new(instance_root: &Path) -> Self {
        Self {
            root: instance_root.join(DIR_NAME),
        }
    }

    fn state_path(&self) -> PathBuf {
        self.root.join(STATE_FILE)
    }

    fn timings_path(&self) -> PathBuf {
        self.root.join(TIMINGS_FILE)
    }

    fn training(&self) -> PathBuf {
        self.root.join(TRAINING_DIR)
    }

    fn cache_file(&self, key: &str) -> PathBuf {
        self.root.join(format!("{}.aot", short(key)))
    }

    fn read_state(&self) -> Option<CacheState> {
        serde_json::from_slice(&fs::read(self.state_path()).ok()?).ok()
    }

    fn read_run(&self) -> Option<RunRecord> {
        let record: RunRecord = serde_json::from_slice(&fs::read(self.training().join(RUN_FILE)).ok()?).ok()?;
        (record.schema == SCHEMA && !record.file.contains(['/', '\\'])).then_some(record)
    }

    pub(crate) fn read_timings(&self) -> Timings {
        fs::read(self.timings_path())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn update_timings(&self, update: impl FnOnce(&mut Timings)) {
        let mut timings = self.read_timings();
        update(&mut timings);
        let _ = self.write_json(&self.timings_path(), &timings);
    }

    fn write_json<T: Serialize>(&self, path: &Path, value: &T) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
        crate::fs::write_safe(path, &bytes)
    }

    /// Removes every cache file except `keep`, and the state if `keep` is `None`.
    fn remove_caches_except(&self, keep: Option<&Path>) {
        let Ok(read_dir) = fs::read_dir(&self.root) else {
            return;
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|extension| extension == "aot") && Some(path.as_path()) != keep {
                let _ = fs::remove_file(&path);
            }
        }
        if keep.is_none() {
            let _ = fs::remove_file(self.state_path());
        }
        let _ = fs::remove_file(self.root.join(LEGACY_FAILURE_FILE));
    }

    fn discard_training(&self) {
        let _ = fs::remove_dir_all(self.training());
    }

    /// The cache named by `state.json` if it is for `key` and the file has the recorded size.
    fn valid_cache(&self, key: &str) -> Option<(PathBuf, u64)> {
        let state = self.read_state()?;
        if state.schema != SCHEMA || state.key != key || state.size < MIN_CACHE_BYTES {
            return None;
        }
        let path = self.root.join(&state.file);
        (path == self.cache_file(key) && fs::metadata(&path).ok()?.len() == state.size).then_some((path, state.size))
    }

    /// A recorded training whose profile is intact: present with the size written in `run.json`.
    fn pending_record(&self) -> Option<RunRecord> {
        let record = self.read_run()?;
        let recorded = record.recorded.as_ref()?;
        let size = fs::metadata(self.training().join(&record.file)).ok()?.len();
        (size == recorded.size && size >= MIN_CACHE_BYTES).then_some(record)
    }

    /// Promotes the output of a successful `create`: it becomes the valid cache.
    fn promote(&self, record: &RunRecord, output: &Path) -> Result<u64, String> {
        let size = fs::metadata(output).map_err(|_| "no-output")?.len();
        if size < MIN_CACHE_BYTES {
            return Err(format!("output-too-small ({size} bytes)"));
        }
        let target = self.cache_file(&record.key);
        let _ = fs::remove_file(&target);
        fs::rename(output, &target).map_err(|error| format!("rename-failed: {error}"))?;
        let state = CacheState {
            schema: SCHEMA,
            key: record.key.clone(),
            file: target.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
            size,
            created_unix_secs: unix_secs(),
            jdk: record.java_version.clone(),
            inputs: record.inputs.clone(),
        };
        if let Err(error) = self.write_json(&self.state_path(), &state) {
            let _ = fs::remove_file(&target);
            return Err(format!("state-write-failed: {error}"));
        }
        self.remove_caches_except(Some(&target));
        self.discard_training();
        Ok(size)
    }
}

/// `force-error` makes the JVM refuse to start (test of the automatic disable).
pub(crate) fn env_switch() -> Option<String> {
    std::env::var(ENV_SWITCH).ok().map(|value| value.to_ascii_lowercase())
}

pub(crate) fn env_disabled() -> bool {
    env_switch().is_some_and(|value| value == "off" || value == "0")
}

/// Pre-checks that need nothing from the disk.
fn eligibility(request: &LaunchRequest, facts: &Facts) -> Result<(), Disabled> {
    if env_disabled() {
        return Err(Disabled::Silent("disabled-by-environment"));
    }
    if request.disabled_by_setting {
        return Err(Disabled::Silent("disabled-by-instance-setting"));
    }
    if request.skip_once {
        return Err(Disabled::Silent("retry-without-cache"));
    }
    if facts.jvm_binary_enabled {
        return Err(Disabled::Logged("custom-java-binary".into()));
    }
    if facts.force_external_java {
        return Err(Disabled::Logged("forced-external-java".into()));
    }
    if facts.java_major != java_default::JAVA_MAJOR {
        return Err(Disabled::Silent("java-not-25"));
    }
    if facts.sandbox {
        return Err(Disabled::Logged("sandbox".into()));
    }
    if facts.wrapper_command {
        return Err(Disabled::Logged("wrapper-command".into()));
    }
    if std::env::var_os("BOOTOPTIM_LAUNCH_INTERPOSER").is_some() {
        return Err(Disabled::Logged("legacy-appcds-active".into()));
    }
    if facts.jvm_args.iter().any(|arg| is_aot_flag(arg)) {
        return Err(Disabled::Logged("player-class-data-flags".into()));
    }
    Ok(())
}

/// The outcome of looking at the instance's cache directory.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Decision {
    Use {
        cache: PathBuf,
        size: u64,
    },
    Record {
        profile: PathBuf,
    },
    /// A recorded profile for this key waits for its `create` job: plain start, keep it.
    CreatePending,
    Disabled(Disabled),
}

/// What the `create` job needs from the game's command line. Written into `run.json` when a
/// record run starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandSpec {
    pub java: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub path_env: Option<String>,
}

/// Looks at the directory and decides. Cleans up what is stale: an invalid cache, a training
/// directory for another key or with a damaged profile.
pub(crate) fn decide(
    dir: &Dir,
    inputs: &KeyInputs,
    java_version: &str,
    spec: &CommandSpec,
    other_process_running: bool,
    free_bytes: Option<u64>,
) -> Decision {
    let key = inputs.key();

    if let Some((cache, size)) = dir.valid_cache(&key) {
        if !other_process_running {
            dir.discard_training();
        }
        return Decision::Use { cache, size };
    }
    if dir.read_state().is_some() {
        log::info!("[aot-cache] cache for this instance no longer matches its key; removing it to train again");
        dir.remove_caches_except(None);
    }
    if other_process_running {
        return Decision::Disabled(Disabled::Silent("instance-already-running"));
    }

    match dir.pending_record() {
        Some(record) if record.key == key => return Decision::CreatePending,
        Some(_) => log::info!("[aot-cache] discarding a recorded training profile for another key"),
        None if dir.training().exists() => log::info!("[aot-cache] discarding an unfinished training run"),
        None => {},
    }
    dir.discard_training();

    if let Some(free) = free_bytes
        && free < MIN_FREE_BYTES
    {
        return Decision::Disabled(Disabled::Logged(format!("low-disk-space ({} MiB free)", free / (1024 * 1024))));
    }

    let training = dir.training();
    if let Err(error) = fs::create_dir_all(&training) {
        return Decision::Disabled(Disabled::Logged(format!("cannot-create-training-dir ({error})")));
    }
    let file = format!("train-{:016x}.aotconf", rand::random::<u64>());
    let record = RunRecord {
        schema: SCHEMA,
        key: key.clone(),
        file: file.clone(),
        started_unix_secs: unix_secs(),
        java_version: java_version.to_owned(),
        inputs: inputs.clone(),
        java: spec.java.clone(),
        args: spec.args.clone(),
        cwd: spec.cwd.clone(),
        path_env: spec.path_env.clone(),
        recorded: None,
    };
    if let Err(error) = dir.write_json(&training.join(RUN_FILE), &record) {
        dir.discard_training();
        return Decision::Disabled(Disabled::Logged(format!("cannot-write-run-record ({error})")));
    }
    Decision::Record {
        profile: training.join(file),
    }
}

/// Free bytes on the volume of `path`, or `None` if unknown.
pub(crate) fn free_disk_bytes(path: &Path) -> Option<u64> {
    #[cfg(windows)]
    {
        use windows::{Win32::Storage::FileSystem::GetDiskFreeSpaceExW, core::HSTRING};
        let mut free = 0u64;
        unsafe { GetDiskFreeSpaceExW(&HSTRING::from(path.as_os_str()), Some(&mut free), None, None) }.ok()?;
        Some(free)
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
        let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } != 0 {
            return None;
        }
        Some(stat.f_bavail as u64 * stat.f_frsize as u64)
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = path;
        None
    }
}

fn flag_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

// ---- Start and exit hooks --------------------------------------------------------------------

pub(crate) fn log_active(java_version: &str, mode: &str, key: &str, detail: &str) {
    log::info!(
        "[WBOMBO-COMPAT] ACTIVE project=Pandora mechanism={MECHANISM} mod=java@{java_version} mode={mode} key={}{detail}",
        short(key)
    );
}

pub(crate) fn log_fallback(java_version: &str, reason: &str) {
    log::warn!(
        "[WBOMBO-COMPAT] FALLBACK project=Pandora mechanism={MECHANISM} mod=java@{java_version} reason={reason}"
    );
}

/// The launch command as text, for the `create` job. `None` if an argument is not valid Unicode
/// (it could not be written to `run.json` faithfully).
fn command_spec(facts: &Facts) -> Option<CommandSpec> {
    Some(CommandSpec {
        java: facts.java_path.to_str()?.to_owned(),
        args: facts.jvm_args.iter().map(|arg| arg.to_str().map(str::to_owned)).collect::<Option<_>>()?,
        cwd: facts.game_dir.to_str()?.to_owned(),
        path_env: match &facts.path_env {
            Some(path) => Some(path.to_str()?.to_owned()),
            None => None,
        },
    })
}

/// Decides for this Start and returns the VM arguments to add. Never fails: every problem is a
/// plain start.
pub(crate) fn prepare(request: &LaunchRequest, facts: &Facts) -> Prepared {
    let identity = jdk_identity(facts.java_path);
    let java_version = identity.as_ref().map_or("unknown", |(version, _)| version.as_str()).to_owned();

    if request.disabled_by_setting && !request.other_process_running {
        remove_all(&request.instance_root);
    }

    let result = (|| -> Result<(Decision, KeyInputs, String), Disabled> {
        eligibility(request, facts)?;
        let (version, jdk_hash) = identity.ok_or_else(|| Disabled::Logged("jdk-release-unreadable".into()))?;
        if runtime_major(&version) != Some(java_default::JAVA_MAJOR) {
            return Err(Disabled::Silent("java-not-25"));
        }
        let classpath =
            classpath_fingerprint(facts.classpath).ok_or_else(|| Disabled::Logged("classpath-unreadable".into()))?;
        let spec = command_spec(facts).ok_or_else(|| Disabled::Logged("non-unicode-launch-arguments".into()))?;
        let inputs = KeyInputs {
            jdk: jdk_hash,
            profile_id: request.pin.profile_id.clone(),
            revision_id: request.pin.revision_id.clone(),
            manifest_sha256: request.pin.manifest_sha256.clone(),
            mods: request.mods_fingerprint.clone(),
            classpath,
            jvm_args: jvm_args_fingerprint(&facts.jvm_args),
        };
        let dir = Dir::new(&request.instance_root);
        let decision = decide(
            &dir,
            &inputs,
            &version,
            &spec,
            request.other_process_running,
            free_disk_bytes(&request.instance_root),
        );
        Ok((decision, inputs, version))
    })();

    let (decision, inputs, version) = match result {
        Ok(parts) => parts,
        Err(disabled) => {
            log_disabled(&java_version, &disabled);
            return Prepared::none();
        },
    };
    let key = inputs.key();
    let dir = Dir::new(&request.instance_root);
    let force_error = env_switch().as_deref() == Some("force-error");
    let run = |kind| ActiveRun {
        kind,
        dir: dir.root.clone(),
        key: key.clone(),
        inputs: inputs.clone(),
        java_version: version.clone(),
        started: Instant::now(),
        quick_play: request.quick_play.clone(),
        assembly_grace_used: false,
        shutdown_grace_used: false,
        dump_started: None,
        had_window: false,
    };
    let (mut flags, active) = match decision {
        Decision::Use { cache, size } => {
            log_active(&version, "use", &key, &format!(" size_mib={}", size / (1024 * 1024)));
            (vec![format!("-XX:AOTCache={}", flag_path(&cache))], run(RunKind::Use))
        },
        Decision::Record { profile } => {
            log_active(&version, "record", &key, "");
            (
                vec![
                    "-XX:AOTMode=record".to_owned(),
                    format!("-XX:AOTConfiguration={}", flag_path(&profile)),
                ],
                run(RunKind::Record { profile }),
            )
        },
        Decision::CreatePending => {
            log::info!("[aot-cache] a recorded profile waits for its create step; this Start runs without the cache");
            return Prepared::none();
        },
        Decision::Disabled(disabled) => {
            log_disabled(&version, &disabled);
            return Prepared::none();
        },
    };
    if force_error {
        log::warn!("[aot-cache] {ENV_SWITCH}=force-error: adding an invalid AOT flag on purpose");
        flags.push("-XX:AOTMode=wbombo-forced-error".to_owned());
    }
    Prepared {
        flags,
        run: Some(active),
    }
}

fn log_disabled(java_version: &str, disabled: &Disabled) {
    match disabled {
        Disabled::Silent(reason) => log::debug!("[aot-cache] not used: {reason}"),
        Disabled::Logged(reason) => log_fallback(java_version, reason),
    }
}

/// Removes the whole cache directory of an instance (option turned off).
pub(crate) fn remove_all(instance_root: &Path) {
    let root = instance_root.join(DIR_NAME);
    if root.exists() {
        log::info!("[aot-cache] removing the AOT cache of an instance whose option is off");
        let _ = fs::remove_dir_all(root);
    }
}

/// Bytes of the profile written so far by a record run whose game closed. The JVM creates the
/// file only when it starts writing at exit, so its presence also tells that the game is closed.
pub(crate) fn profile_bytes_written(run: &ActiveRun) -> Option<u64> {
    match &run.kind {
        RunKind::Record { profile } => fs::metadata(profile).ok().map(|metadata| metadata.len()),
        RunKind::Use => None,
    }
}

/// A record run whose JVM is writing the profile. The launcher gives a Stop more time in that
/// phase instead of killing the JVM and losing the profile.
pub(crate) fn is_assembling(run: &ActiveRun) -> bool {
    profile_bytes_written(run).is_some()
}

/// A record run whose game is closed while its JVM still works on the profile. On the reference PC
/// the JVM prepares the profile for ≈ 16 s after the game closes before it creates the file, so
/// the game's window is the earlier signal: it had a visible window and has none now.
pub(crate) fn is_recording_after_close(run: &mut ActiveRun, pid: u32) -> bool {
    if !matches!(run.kind, RunKind::Record { .. }) {
        return false;
    }
    if is_assembling(run) {
        return true;
    }
    match has_visible_window(pid) {
        Some(true) => {
            run.had_window = true;
            false
        },
        Some(false) => run.had_window,
        None => false,
    }
}

/// Whether the process has a visible top-level window of the game (the JVM launcher's error dialog
/// does not count); `None` where this is not known.
fn has_visible_window(pid: u32) -> Option<bool> {
    visible_windows(pid).map(|windows| windows.game)
}

/// Whether the process shows the error dialog of `javaw` ("Java Virtual Machine Launcher"), which
/// it shows when the JVM refuses to start, e.g. on a rejected AOT flag. Windows only.
pub(crate) fn shows_jvm_error_dialog(pid: u32) -> bool {
    visible_windows(pid).is_some_and(|windows| windows.jvm_error_dialog)
}

struct VisibleWindows {
    game: bool,
    jvm_error_dialog: bool,
}

fn visible_windows(pid: u32) -> Option<VisibleWindows> {
    #[cfg(windows)]
    {
        use windows::{
            Win32::{
                Foundation::{HWND, LPARAM},
                UI::WindowsAndMessaging::{
                    EnumWindows, GetClassNameW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
                },
            },
            core::BOOL,
        };
        struct Search {
            pid: u32,
            found: VisibleWindows,
        }
        unsafe extern "system" fn visit(window: HWND, data: LPARAM) -> BOOL {
            let search = unsafe { &mut *(data.0 as *mut Search) };
            let mut owner = 0u32;
            unsafe { GetWindowThreadProcessId(window, Some(&mut owner)) };
            if owner != search.pid || !unsafe { IsWindowVisible(window) }.as_bool() {
                return BOOL(1);
            }
            let mut class = [0u16; 16];
            let class_len = unsafe { GetClassNameW(window, &mut class) }.max(0) as usize;
            let class = &class[..class_len];
            let mut title = [0u16; 64];
            let title_len = unsafe { GetWindowTextW(window, &mut title) }.max(0) as usize;
            let title = &title[..title_len];
            // A standard dialog (#32770) with the title the `javaw` launcher gives its error box.
            if String::from_utf16_lossy(class) == "#32770"
                && String::from_utf16_lossy(title) == "Java Virtual Machine Launcher"
            {
                search.found.jvm_error_dialog = true;
            } else {
                search.found.game = true;
            }
            BOOL(1)
        }
        let mut search = Search { pid, found: VisibleWindows { game: false, jvm_error_dialog: false } };
        let _ = unsafe { EnumWindows(Some(visit), LPARAM(&mut search as *mut Search as isize)) };
        Some(search.found)
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        None
    }
}

/// Extra time a Stop gives a JVM that is writing the profile before the launcher kills it anyway.
pub(crate) const ASSEMBLY_GRACE: Duration = Duration::from_secs(600);

/// Time a Stop gives a record run's game to close normally (saving the world) before the JVM
/// starts writing the profile. The plain Stop gives 3 s, which would lose the training.
pub(crate) const SHUTDOWN_GRACE: Duration = Duration::from_secs(60);

/// Handles the end of a tracked process: keeps a complete profile for the `create` job, drops a
/// cache that failed early, and tells the caller whether to start again without the cache.
pub(crate) fn finish_run(run: &ActiveRun, exit: ExitKind) -> ExitAction {
    finish_run_after(run, exit, run.started.elapsed())
}

pub(crate) fn finish_run_after(run: &ActiveRun, exit: ExitKind, lived: Duration) -> ExitAction {
    let dir = Dir { root: run.dir.clone() };

    // A Stop is the player's doing: never an AOT failure, never a retry.
    if let ExitKind::Exited(code) = exit
        && let Some(reason) = java_default::early_exit_reason(code, lived)
    {
        dir.remove_caches_except(None);
        dir.discard_training();
        log_fallback(&run.java_version, &format!("early-exit ({reason}); retrying without the cache"));
        return ExitAction::RetryWithoutCache { reason };
    }

    match (&run.kind, exit) {
        (RunKind::Use, _) => ExitAction::Nothing,
        (RunKind::Record { profile }, ExitKind::Exited(Some(0)) | ExitKind::Stopped(Some(0))) => {
            match mark_recorded(&dir, profile) {
                Ok(size) => {
                    let record_secs = run.dump_started.map(|started| started.elapsed().as_secs_f32());
                    dir.update_timings(|timings| {
                        timings.profile_bytes = Some(size);
                        if let Some(record_secs) = record_secs {
                            timings.record_secs = Some(record_secs);
                        }
                    });
                    log_active(
                        &run.java_version,
                        "recorded",
                        &run.key,
                        &format!(
                            " profile_mib={} write_seconds={}",
                            size / (1024 * 1024),
                            record_secs.map_or_else(|| "?".to_owned(), |secs| format!("{secs:.1}"))
                        ),
                    );
                    ExitAction::Recorded
                },
                Err(reason) => {
                    dir.discard_training();
                    log::info!("[aot-cache] training discarded: {reason}");
                    ExitAction::Discarded
                },
            }
        },
        (RunKind::Record { .. }, ExitKind::Stopped(code)) => {
            dir.discard_training();
            log::info!(
                "[aot-cache] training discarded: the game was stopped before the profile was written ({code:?})"
            );
            ExitAction::Discarded
        },
        (RunKind::Record { .. }, ExitKind::Exited(code)) => {
            dir.discard_training();
            log::info!("[aot-cache] training discarded: the game exited with {code:?}");
            ExitAction::Discarded
        },
    }
}

/// Writes the profile's size into `run.json` after the game exited with code 0.
fn mark_recorded(dir: &Dir, profile: &Path) -> Result<u64, String> {
    let mut record = dir.read_run().ok_or("run-record-unreadable")?;
    if dir.training().join(&record.file) != profile {
        return Err("run-record-mismatch".into());
    }
    let size = fs::metadata(profile).map_err(|_| "no-profile")?.len();
    if size < MIN_CACHE_BYTES {
        return Err(format!("profile-too-small ({size} bytes)"));
    }
    record.recorded = Some(Recorded {
        size,
        unix_secs: unix_secs(),
    });
    dir.write_json(&dir.training().join(RUN_FILE), &record)
        .map_err(|error| format!("run-record-write-failed: {error}"))?;
    Ok(size)
}

// ---- create job ------------------------------------------------------------------------------

/// A recorded profile ready for the `create` step.
#[derive(Debug, Clone)]
pub struct PendingCreate {
    dir: PathBuf,
    record: RunRecord,
    output: PathBuf,
}

impl PendingCreate {
    pub(crate) fn java_version(&self) -> &str {
        &self.record.java_version
    }

    /// The `create` command: the game's JVM and VM arguments, then the AOT flags. The result goes
    /// to a temporary name inside `training/` and is promoted by [`finish_create`].
    pub(crate) fn command(&self) -> command::PandoraCommand {
        let profile = self.dir.join(TRAINING_DIR).join(&self.record.file);
        let mut command = command::PandoraCommand::new(OsString::from(&self.record.java));
        for arg in &self.record.args {
            command.arg(arg.clone());
        }
        command.arg("-XX:AOTMode=create");
        command.arg(format!("-XX:AOTConfiguration={}", flag_path(&profile)));
        command.arg(format!("-XX:AOTCache={}", flag_path(&self.output)));
        command.current_dir(Path::new(&self.record.cwd));
        if let Some(path) = &self.record.path_env {
            command.env("PATH", path.clone());
        }
        command.stdin(command::PandoraStdioWriteMode::Null);
        command.stdout(command::PandoraStdioReadMode::Null);
        command.stderr(command::PandoraStdioReadMode::Null);
        command
    }
}

/// The instance's recorded profile, if one is intact and waiting. A training directory with
/// nothing usable is removed (call only when no process of the instance is alive).
pub(crate) fn pending_create(instance_root: &Path) -> Option<PendingCreate> {
    let dir = Dir::new(instance_root);
    if !dir.training().exists() {
        return None;
    }
    let Some(record) = dir.pending_record() else {
        log::info!("[aot-cache] discarding a training directory without a complete profile");
        dir.discard_training();
        return None;
    };
    // A leftover output of an interrupted create is never used.
    if let Ok(read_dir) = fs::read_dir(dir.training()) {
        for entry in read_dir.flatten() {
            if entry.path().extension().is_some_and(|extension| extension == "aot") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    let output = dir.training().join(format!("cache-{:016x}.aot", rand::random::<u64>()));
    Some(PendingCreate {
        dir: dir.root,
        record,
        output,
    })
}

/// How a `create` job ended.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CreateOutcome {
    /// The cache is now valid.
    Promoted { size: u64 },
    /// Cancelled (by the player or by closing the launcher): the profile is kept for later.
    Postponed,
    /// The JVM rejected the profile or failed to build the cache: the training is discarded and
    /// the option should be turned off for the instance.
    Failed { reason: String },
}

/// Handles the end of a `create` job. `exit_code` is `None` if the process was killed.
pub(crate) fn finish_create(
    job: &PendingCreate,
    exit_code: Option<i32>,
    cancelled: bool,
    took: Duration,
) -> CreateOutcome {
    let dir = Dir { root: job.dir.clone() };
    if cancelled {
        let _ = fs::remove_file(&job.output);
        log::info!(
            "[aot-cache] create step cancelled after {}s; the training profile is kept for later",
            took.as_secs()
        );
        return CreateOutcome::Postponed;
    }
    if exit_code != Some(0) {
        dir.discard_training();
        let reason = format!("create-failed (exit {exit_code:?} after {}s)", took.as_secs());
        log_fallback(&job.record.java_version, &reason);
        return CreateOutcome::Failed { reason };
    }
    match dir.promote(&job.record, &job.output) {
        Ok(size) => {
            dir.update_timings(|timings| timings.create_secs = Some(took.as_secs_f32()));
            log_active(
                &job.record.java_version,
                "created",
                &job.record.key,
                &format!(" size_mib={} create_seconds={:.1}", size / (1024 * 1024), took.as_secs_f32()),
            );
            CreateOutcome::Promoted { size }
        },
        Err(reason) => {
            dir.discard_training();
            let reason = format!("create-output-invalid ({reason})");
            log_fallback(&job.record.java_version, &reason);
            CreateOutcome::Failed { reason }
        },
    }
}

// ---- progress estimate -----------------------------------------------------------------------

/// Where an optimization is, for the progress estimate.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Progress {
    /// The JVM is writing the profile: bytes so far and time since the launcher noticed.
    Recording {
        written: u64,
        elapsed: Duration,
    },
    /// The `create` job runs.
    Creating { elapsed: Duration },
}

/// Approximate progress in 0..1. Java reports none: the record part compares the profile's size
/// with the last measured size (≈ 305 MB the first time) and the time with the last record; the
/// create part compares the time with the last create (≈ 15 s the first time). Capped at 0.99.
pub(crate) fn estimate_progress(progress: Progress, timings: &Timings) -> f32 {
    let record_secs = timings.record_secs.filter(|secs| *secs > 0.5).unwrap_or(DEFAULT_RECORD_SECS);
    let create_secs = timings.create_secs.filter(|secs| *secs > 0.5).unwrap_or(DEFAULT_CREATE_SECS);
    let record_share = record_secs / (record_secs + create_secs);
    let fraction = match progress {
        Progress::Recording { written, elapsed } => {
            let expected = timings
                .profile_bytes
                .filter(|bytes| *bytes >= MIN_CACHE_BYTES)
                .unwrap_or(DEFAULT_PROFILE_BYTES);
            let by_size = written as f32 / expected as f32;
            let by_time = elapsed.as_secs_f32() / record_secs;
            record_share * by_size.max(by_time).min(0.99)
        },
        Progress::Creating { elapsed } => {
            record_share + (1.0 - record_share) * (elapsed.as_secs_f32() / create_secs).min(0.99)
        },
    };
    fraction.clamp(0.0, 0.99)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new(label: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("pandora-aot-{label}-{}", uuid::Uuid::from_bytes(rand::random())));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }

        /// A fake runtime `bin/java.exe` + `release` + `lib/modules` under this root.
        fn fake_jdk(&self, runtime_version: &str, modules_len: usize) -> PathBuf {
            let home = self.0.join("jdk");
            fs::create_dir_all(home.join("bin")).unwrap();
            fs::create_dir_all(home.join("lib")).unwrap();
            fs::write(home.join("bin").join("java.exe"), b"x").unwrap();
            fs::write(
                home.join("release"),
                format!(
                    "JAVA_VERSION=\"{}\"\nJAVA_RUNTIME_VERSION=\"{runtime_version}\"\n",
                    runtime_version.split('+').next().unwrap()
                ),
            )
            .unwrap();
            fs::write(home.join("lib").join("modules"), vec![0u8; modules_len]).unwrap();
            home.join("bin").join("java.exe")
        }

        fn instance(&self) -> PathBuf {
            let path = self.0.join("instance");
            fs::create_dir_all(&path).unwrap();
            path
        }

        fn jar(&self, name: &str, size: usize) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, vec![1u8; size]).unwrap();
            path
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn inputs() -> KeyInputs {
        KeyInputs {
            jdk: "jdk-a".into(),
            profile_id: "profile_wachiland-elite-beta".into(),
            revision_id: "rev_1".into(),
            manifest_sha256: "a".repeat(64),
            mods: "mods-a".into(),
            classpath: "cp-a".into(),
            jvm_args: "args-a".into(),
        }
    }

    fn spec() -> CommandSpec {
        CommandSpec {
            java: "C:/jdk/bin/java.exe".into(),
            args: vec!["-Xmx8g".into(), "-cp".into(), "C:/a.jar".into()],
            cwd: "C:/instance/.minecraft".into(),
            path_env: Some("C:/natives".into()),
        }
    }

    fn big_file(path: &Path) {
        fs::write(path, vec![7u8; MIN_CACHE_BYTES as usize + 10]).unwrap();
    }

    fn decide_now(dir: &Dir, inputs: &KeyInputs) -> Decision {
        decide(dir, inputs, "25.0.1+8-LTS", &spec(), false, Some(u64::MAX))
    }

    /// Runs `decide` for a fresh training and returns its profile path.
    fn record_profile(dir: &Dir, inputs: &KeyInputs) -> PathBuf {
        match decide_now(dir, inputs) {
            Decision::Record { profile } => profile,
            other => panic!("expected Record, got {other:?}"),
        }
    }

    fn run_for(dir: &Dir, inputs: &KeyInputs, profile: PathBuf) -> ActiveRun {
        ActiveRun {
            kind: RunKind::Record { profile },
            dir: dir.root.clone(),
            key: inputs.key(),
            inputs: inputs.clone(),
            java_version: "25.0.1+8-LTS".into(),
            started: Instant::now(),
            quick_play: None,
            assembly_grace_used: false,
            shutdown_grace_used: false,
            dump_started: None,
            had_window: false,
        }
    }

    fn use_run(dir: &Dir, inputs: &KeyInputs) -> ActiveRun {
        ActiveRun {
            kind: RunKind::Use,
            ..run_for(dir, inputs, PathBuf::new())
        }
    }

    /// Records a complete profile for `inputs` (the game exited with 0).
    fn recorded(dir: &Dir, inputs: &KeyInputs) -> PathBuf {
        let profile = record_profile(dir, inputs);
        big_file(&profile);
        let run = run_for(dir, inputs, profile.clone());
        assert_eq!(
            finish_run_after(&run, ExitKind::Exited(Some(0)), Duration::from_secs(600)),
            ExitAction::Recorded
        );
        profile
    }

    /// Simulates the `create` job: the JVM writes the output and exits with `code`.
    fn create(dir: &Dir, code: Option<i32>) -> CreateOutcome {
        let job = pending_create(dir.root.parent().unwrap()).expect("a pending profile");
        if code == Some(0) {
            big_file(&job.output);
        }
        finish_create(&job, code, false, Duration::from_secs(15))
    }

    /// Records and creates a cache for `inputs`, returning its path.
    fn promoted(dir: &Dir, inputs: &KeyInputs) -> PathBuf {
        recorded(dir, inputs);
        assert!(matches!(create(dir, Some(0)), CreateOutcome::Promoted { .. }));
        dir.cache_file(&inputs.key())
    }

    fn assert_records(dir: &Dir, inputs: &KeyInputs) {
        assert!(matches!(decide_now(dir, inputs), Decision::Record { .. }));
    }

    fn assert_uses(dir: &Dir, inputs: &KeyInputs) {
        assert!(matches!(decide_now(dir, inputs), Decision::Use { .. }));
    }

    #[test]
    fn key_is_stable_and_changes_with_every_input() {
        let base = inputs();
        assert_eq!(base.key(), inputs().key());
        let variants: Vec<Box<dyn Fn(&mut KeyInputs)>> = vec![
            Box::new(|i| i.jdk = "jdk-b".into()),
            Box::new(|i| i.profile_id = "profile_wachiland-elite".into()),
            Box::new(|i| i.revision_id = "rev_2".into()),
            Box::new(|i| i.manifest_sha256 = "b".repeat(64)),
            Box::new(|i| i.mods = "mods-b".into()),
            Box::new(|i| i.classpath = "cp-b".into()),
            Box::new(|i| i.jvm_args = "args-b".into()),
        ];
        for change in variants {
            let mut other = inputs();
            change(&mut other);
            assert_ne!(other.key(), base.key());
        }
        // Field boundaries are part of the hash: moving a character between fields changes it.
        let mut left = inputs();
        left.jdk = "ab".into();
        left.profile_id = "c".into();
        let mut right = inputs();
        right.jdk = "a".into();
        right.profile_id = "bc".into();
        assert_ne!(left.key(), right.key());
    }

    #[test]
    fn mods_fingerprint_covers_only_mods_and_their_hashes() {
        let entry = |hash: &str| ProfileEntryMetadata {
            logical_identity: "x".into(),
            source_sha256: hash.into(),
            source_size_bytes: 1,
            source_modified_unix_nanos: 1,
            origin: crate::profile_branch::ProfileEntryOrigin::LocalProfile {
                profile_uuid: uuid::Uuid::nil(),
            },
            ownership: crate::profile_branch::ProfileEntryOwnership::Inherited,
            policy: crate::profile_branch::ProfileFilePolicy::default(),
        };
        let mut entries = BTreeMap::new();
        entries.insert("mods/a.jar".to_owned(), entry("h1"));
        entries.insert("config/a.toml".to_owned(), entry("c1"));
        let base = mods_fingerprint(&entries);

        entries.insert("config/a.toml".to_owned(), entry("c2"));
        assert_eq!(mods_fingerprint(&entries), base, "configs are not part of the mod list");

        entries.insert("mods/a.jar".to_owned(), entry("h2"));
        assert_ne!(mods_fingerprint(&entries), base, "a changed mod changes it");

        entries.insert("mods/a.jar".to_owned(), entry("h1"));
        entries.insert("mods/b.jar".to_owned(), entry("h3"));
        assert_ne!(mods_fingerprint(&entries), base, "an added mod changes it");
    }

    #[test]
    fn jvm_args_fingerprint_ignores_properties_and_cache_flags_but_not_vm_shape() {
        let args = |list: &[&str]| list.iter().map(OsString::from).collect::<Vec<_>>();
        let base = jvm_args_fingerprint(&args(&["-Xmx8g", "-XX:+UseG1GC", "-Dminecraft.launcher.version=1"]));
        assert_eq!(
            base,
            jvm_args_fingerprint(&args(&[
                "-Xmx8g",
                "-XX:+UseG1GC",
                "-Dminecraft.launcher.version=2",
                "-XX:AOTCache=C:/x.aot"
            ]))
        );
        assert_ne!(base, jvm_args_fingerprint(&args(&["-Xmx6g", "-XX:+UseG1GC"])));
        assert_ne!(base, jvm_args_fingerprint(&args(&["-Xmx8g", "-XX:+UseZGC"])));
    }

    #[test]
    fn jdk_identity_reads_the_exact_build_and_changes_with_it() {
        let root = TestRoot::new("jdk");
        let java = root.fake_jdk("25.0.1+8-LTS", 10);
        let (version, id_a) = jdk_identity(&java).unwrap();
        assert_eq!(version, "25.0.1+8-LTS");
        assert_eq!(runtime_major(&version), Some(25));

        let java = root.fake_jdk("25.0.1+9-LTS", 10);
        let (_, id_b) = jdk_identity(&java).unwrap();
        assert_ne!(id_a, id_b, "another build of the same version is another JDK");

        let java = root.fake_jdk("25.0.1+9-LTS", 11);
        assert_ne!(jdk_identity(&java).unwrap().1, id_b, "a different lib/modules is another JDK");

        fs::remove_file(java.parent().unwrap().parent().unwrap().join("release")).unwrap();
        assert!(jdk_identity(&java).is_none());
        assert_eq!(runtime_major("21.0.7"), Some(21));
        assert_eq!(runtime_major("8u202"), None);
    }

    #[test]
    fn classpath_fingerprint_follows_size_and_presence() {
        let root = TestRoot::new("cp");
        let a = root.jar("a.jar", 10);
        let b = root.jar("b.jar", 20);
        let before = classpath_fingerprint(&[a.clone(), b.clone()]).unwrap();
        assert_eq!(before, classpath_fingerprint(&[a.clone(), b.clone()]).unwrap());
        assert_ne!(
            before,
            classpath_fingerprint(&[b.clone(), a.clone()]).unwrap(),
            "order matters, as for the JVM"
        );
        root.jar("b.jar", 21);
        assert_ne!(before, classpath_fingerprint(&[a.clone(), b.clone()]).unwrap());
        fs::remove_file(&b).unwrap();
        assert!(classpath_fingerprint(&[a, b]).is_none());
    }

    #[test]
    fn first_start_records_then_create_makes_the_cache() {
        let root = TestRoot::new("flow");
        let dir = Dir::new(&root.instance());
        let inputs = inputs();
        let profile = record_profile(&dir, &inputs);
        assert!(profile.starts_with(dir.training()));
        assert_eq!(dir.read_run().unwrap().args, spec().args, "run.json keeps the command for create");
        assert!(dir.valid_cache(&inputs.key()).is_none());

        big_file(&profile);
        let run = run_for(&dir, &inputs, profile.clone());
        assert_eq!(
            finish_run_after(&run, ExitKind::Exited(Some(0)), Duration::from_secs(600)),
            ExitAction::Recorded
        );
        assert!(profile.exists(), "the profile waits for create");
        assert_eq!(
            decide_now(&dir, &inputs),
            Decision::CreatePending,
            "a Start before create does not train again"
        );
        assert!(profile.exists());

        assert!(matches!(create(&dir, Some(0)), CreateOutcome::Promoted { .. }));
        assert!(!dir.training().exists(), "the training directory is gone after promotion");
        let cache = dir.cache_file(&inputs.key());
        assert!(matches!(decide_now(&dir, &inputs), Decision::Use { cache: used, .. } if used == cache));
        assert!(dir.read_timings().create_secs.is_some());
        assert!(dir.read_timings().profile_bytes.is_some());
    }

    #[test]
    fn the_create_command_repeats_the_game_vm_arguments() {
        let root = TestRoot::new("command");
        let dir = Dir::new(&root.instance());
        recorded(&dir, &inputs());
        let job = pending_create(&root.instance()).unwrap();
        let args: Vec<String> = job.command().args_os().iter().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert_eq!(&args[..3], &spec().args[..]);
        assert_eq!(args[3], "-XX:AOTMode=create");
        assert!(args[4].starts_with("-XX:AOTConfiguration=") && args[4].ends_with(".aotconf"));
        assert!(args[5].starts_with("-XX:AOTCache=") && !args[5].contains('\\'));
    }

    #[test]
    fn a_cancelled_create_keeps_the_profile_and_resumes_without_training() {
        let root = TestRoot::new("cancel");
        let dir = Dir::new(&root.instance());
        let inputs = inputs();
        let profile = recorded(&dir, &inputs);
        let job = pending_create(&root.instance()).unwrap();
        fs::write(&job.output, b"partial").unwrap();
        assert_eq!(finish_create(&job, None, true, Duration::from_secs(5)), CreateOutcome::Postponed);
        assert!(profile.exists());
        assert!(!job.output.exists(), "a partial output is removed");
        assert_eq!(decide_now(&dir, &inputs), Decision::CreatePending);
        assert!(matches!(create(&dir, Some(0)), CreateOutcome::Promoted { .. }));
        assert_uses(&dir, &inputs);
    }

    #[test]
    fn a_failed_create_discards_the_training_and_reports_it() {
        let root = TestRoot::new("create-fail");
        let dir = Dir::new(&root.instance());
        let inputs = inputs();
        recorded(&dir, &inputs);
        assert!(
            matches!(create(&dir, Some(1)), CreateOutcome::Failed { reason } if reason.starts_with("create-failed"))
        );
        assert!(!dir.training().exists());
        assert!(dir.valid_cache(&inputs.key()).is_none());
        // An exit 0 with no usable output is a failure too.
        recorded(&dir, &inputs);
        let job = pending_create(&root.instance()).unwrap();
        assert!(matches!(
            finish_create(&job, Some(0), false, Duration::from_secs(1)),
            CreateOutcome::Failed { reason } if reason.starts_with("create-output-invalid")
        ));
    }

    #[test]
    fn a_damaged_or_missing_profile_trains_again() {
        let root = TestRoot::new("damaged");
        let dir = Dir::new(&root.instance());
        let inputs = inputs();
        let profile = recorded(&dir, &inputs);
        fs::write(&profile, b"truncated").unwrap();
        assert!(pending_create(&root.instance()).is_none(), "no create on a damaged profile");
        assert!(!dir.training().exists());
        assert_records(&dir, &inputs);

        let profile = recorded(&dir, &inputs);
        fs::remove_file(&profile).unwrap();
        assert_records(&dir, &inputs);
    }

    #[test]
    fn a_profile_for_another_key_is_discarded() {
        let root = TestRoot::new("otherkey");
        let dir = Dir::new(&root.instance());
        let old = inputs();
        let profile = recorded(&dir, &old);
        let mut new_revision = inputs();
        new_revision.revision_id = "rev_2".into();
        assert_records(&dir, &new_revision);
        assert!(!profile.exists());
    }

    #[test]
    fn a_changed_key_drops_only_this_instances_cache_and_trains_again() {
        let root = TestRoot::new("rekey");
        let dir = Dir::new(&root.instance());
        let old = inputs();
        let old_cache = promoted(&dir, &old);
        let other_instance = Dir::new(&root.0.join("other-instance"));
        let other_cache = promoted(&other_instance, &old);

        let mut new_revision = inputs();
        new_revision.revision_id = "rev_2".into();
        assert_records(&dir, &new_revision);
        assert!(!old_cache.exists(), "the stale cache is removed");
        assert!(dir.read_state().is_none());
        assert!(other_cache.exists(), "another instance keeps its own cache");
        assert_uses(&other_instance, &old);
    }

    #[test]
    fn a_cache_with_the_wrong_size_is_never_used() {
        let root = TestRoot::new("size");
        let dir = Dir::new(&root.instance());
        let inputs = inputs();
        let cache = promoted(&dir, &inputs);
        let bytes = fs::read(&cache).unwrap();
        fs::write(&cache, &bytes[..bytes.len() / 2]).unwrap();
        assert!(dir.valid_cache(&inputs.key()).is_none());
        assert_records(&dir, &inputs);
        assert!(!cache.exists());
    }

    #[test]
    fn a_missing_cache_file_or_unreadable_state_trains_again() {
        let root = TestRoot::new("missing");
        let dir = Dir::new(&root.instance());
        let inputs = inputs();
        let cache = promoted(&dir, &inputs);
        fs::remove_file(&cache).unwrap();
        assert_records(&dir, &inputs);

        // The pending record from the line above is for the same key: finish it first.
        fs::remove_dir_all(dir.training()).unwrap();
        promoted(&dir, &inputs);
        fs::write(dir.state_path(), b"{ not json").unwrap();
        assert_records(&dir, &inputs);
    }

    #[test]
    fn unfinished_recording_is_discarded() {
        let root = TestRoot::new("partial");
        let dir = Dir::new(&root.instance());
        let inputs = inputs();
        // Launcher killed while the JVM wrote the profile: run.json has no `recorded`.
        let profile = record_profile(&dir, &inputs);
        big_file(&profile);
        assert!(pending_create(&root.instance()).is_none());
        assert!(!profile.exists());
        let next = record_profile(&dir, &inputs);
        assert_ne!(next, profile);
    }

    #[test]
    fn a_live_process_blocks_a_second_training_and_keeps_its_directory() {
        let root = TestRoot::new("live");
        let dir = Dir::new(&root.instance());
        let inputs = inputs();
        let profile = record_profile(&dir, &inputs);
        let decision = decide(&dir, &inputs, "j", &spec(), true, Some(u64::MAX));
        assert_eq!(decision, Decision::Disabled(Disabled::Silent("instance-already-running")));
        assert!(profile.parent().unwrap().exists(), "the running training is untouched");
    }

    #[test]
    fn a_live_process_may_still_use_a_valid_cache() {
        let root = TestRoot::new("live-use");
        let dir = Dir::new(&root.instance());
        let inputs = inputs();
        promoted(&dir, &inputs);
        assert!(matches!(decide(&dir, &inputs, "j", &spec(), true, Some(u64::MAX)), Decision::Use { .. }));
    }

    #[test]
    fn low_disk_space_skips_training_with_a_logged_reason() {
        let root = TestRoot::new("disk");
        let dir = Dir::new(&root.instance());
        let decision = decide(&dir, &inputs(), "j", &spec(), false, Some(MIN_FREE_BYTES - 1));
        assert!(
            matches!(decision, Decision::Disabled(Disabled::Logged(reason)) if reason.starts_with("low-disk-space"))
        );
        assert!(!dir.training().exists());
        assert!(
            matches!(decide(&dir, &inputs(), "j", &spec(), false, None), Decision::Record { .. }),
            "unknown space does not block"
        );
    }

    #[test]
    fn an_early_error_with_the_cache_drops_it_and_asks_for_a_retry() {
        let root = TestRoot::new("early");
        let dir = Dir::new(&root.instance());
        let inputs = inputs();
        let cache = promoted(&dir, &inputs);

        let action = finish_run_after(&use_run(&dir, &inputs), ExitKind::Exited(Some(1)), Duration::from_secs(3));
        assert!(matches!(action, ExitAction::RetryWithoutCache { .. }));
        assert!(!cache.exists());
        assert!(dir.read_state().is_none());
        assert_records(&dir, &inputs);
    }

    #[test]
    fn an_early_error_while_recording_retries_without_the_cache_and_discards_the_run() {
        let root = TestRoot::new("early-train");
        let dir = Dir::new(&root.instance());
        let inputs = inputs();
        let profile = record_profile(&dir, &inputs);
        let run = run_for(&dir, &inputs, profile);
        assert!(matches!(
            finish_run_after(&run, ExitKind::Exited(Some(1)), Duration::from_secs(2)),
            ExitAction::RetryWithoutCache { .. }
        ));
        assert!(!dir.training().exists());
    }

    #[test]
    fn a_game_crash_late_in_a_session_keeps_the_cache() {
        let root = TestRoot::new("late-crash");
        let dir = Dir::new(&root.instance());
        let inputs = inputs();
        let cache = promoted(&dir, &inputs);
        let action = finish_run_after(&use_run(&dir, &inputs), ExitKind::Exited(Some(255)), Duration::from_secs(400));
        assert_eq!(action, ExitAction::Nothing);
        assert!(cache.exists());
    }

    #[test]
    fn recording_that_ends_in_a_crash_or_a_kill_keeps_no_profile() {
        let root = TestRoot::new("crash-train");
        let dir = Dir::new(&root.instance());
        let inputs = inputs();
        for exit in [
            ExitKind::Exited(Some(255)),
            ExitKind::Exited(None),
            ExitKind::Stopped(None),
            ExitKind::Stopped(Some(1)),
        ] {
            let profile = record_profile(&dir, &inputs);
            big_file(&profile); // even a plausible file is not kept after an unclean end
            let run = run_for(&dir, &inputs, profile);
            assert_eq!(finish_run_after(&run, exit, Duration::from_secs(400)), ExitAction::Discarded, "{exit:?}");
            assert!(!dir.training().exists());
        }
    }

    #[test]
    fn a_stop_never_triggers_a_retry_even_inside_the_early_window() {
        let root = TestRoot::new("stop-early");
        let dir = Dir::new(&root.instance());
        let inputs = inputs();
        let cache = promoted(&dir, &inputs);
        let action = finish_run_after(&use_run(&dir, &inputs), ExitKind::Stopped(Some(1)), Duration::from_secs(2));
        assert_eq!(action, ExitAction::Nothing);
        assert!(cache.exists());
    }

    #[test]
    fn a_stop_that_lets_the_game_close_normally_keeps_the_profile() {
        let root = TestRoot::new("stop-done");
        let dir = Dir::new(&root.instance());
        let inputs = inputs();
        let profile = record_profile(&dir, &inputs);
        big_file(&profile);
        let run = run_for(&dir, &inputs, profile);
        assert_eq!(
            finish_run_after(&run, ExitKind::Stopped(Some(0)), Duration::from_secs(600)),
            ExitAction::Recorded
        );
    }

    #[test]
    fn writing_the_profile_is_visible_through_the_file() {
        let root = TestRoot::new("assembling");
        let dir = Dir::new(&root.instance());
        let inputs = inputs();
        let profile = record_profile(&dir, &inputs);
        let run = run_for(&dir, &inputs, profile.clone());
        assert!(!is_assembling(&run));
        fs::write(&profile, b"xyz").unwrap();
        assert!(is_assembling(&run));
        assert_eq!(profile_bytes_written(&run), Some(3));
        assert!(!is_assembling(&use_run(&dir, &inputs)));
    }

    #[test]
    fn a_closed_window_counts_only_after_the_game_showed_one() {
        let root = TestRoot::new("window");
        let dir = Dir::new(&root.instance());
        let inputs = inputs();
        let profile = record_profile(&dir, &inputs);
        let mut run = run_for(&dir, &inputs, profile.clone());
        // This test process has no window: before the game ever showed one it is still starting.
        assert!(!is_recording_after_close(&mut run, std::process::id()));
        run.had_window = true;
        assert_eq!(is_recording_after_close(&mut run, std::process::id()), cfg!(windows));
        run.had_window = false;
        fs::write(&profile, b"x").unwrap();
        assert!(is_recording_after_close(&mut run, std::process::id()), "the profile file is always a signal");
        assert!(!is_recording_after_close(&mut use_run(&dir, &inputs), std::process::id()));
    }

    #[test]
    fn progress_is_monotonic_capped_and_uses_the_last_measurements() {
        let none = Timings::default();
        let at = |progress| estimate_progress(progress, &none);
        let start = at(Progress::Recording {
            written: 0,
            elapsed: Duration::ZERO,
        });
        let half_record = at(Progress::Recording {
            written: DEFAULT_PROFILE_BYTES / 2,
            elapsed: Duration::from_secs(1),
        });
        let create_start = at(Progress::Creating {
            elapsed: Duration::ZERO,
        });
        let create_half = at(Progress::Creating {
            elapsed: Duration::from_secs_f32(DEFAULT_CREATE_SECS / 2.0),
        });
        let overdue = at(Progress::Creating {
            elapsed: Duration::from_secs(3600),
        });
        assert_eq!(start, 0.0);
        assert!(
            start < half_record && half_record < create_start && create_start < create_half && create_half < overdue
        );
        assert!(overdue <= 0.99);
        // Time also moves the record part when the file grows late.
        assert!(
            at(Progress::Recording {
                written: 0,
                elapsed: Duration::from_secs(10)
            }) > 0.0
        );
        // A measured slow create gives the create part more weight.
        let slow = Timings {
            profile_bytes: Some(DEFAULT_PROFILE_BYTES),
            record_secs: Some(10.0),
            create_secs: Some(90.0),
        };
        assert!(
            estimate_progress(
                Progress::Creating {
                    elapsed: Duration::ZERO
                },
                &slow
            ) < create_start
        );
    }

    fn facts<'a>(java: &'a Path, classpath: &'a [PathBuf], game_dir: &'a Path) -> Facts<'a> {
        Facts {
            java_path: java,
            java_major: 25,
            classpath,
            jvm_args: vec![OsString::from("-Xmx8g")],
            game_dir,
            path_env: None,
            jvm_binary_enabled: false,
            force_external_java: false,
            sandbox: false,
            wrapper_command: false,
        }
    }

    fn request(root: &TestRoot) -> LaunchRequest {
        LaunchRequest {
            instance_root: root.instance(),
            pin: GlobalRevisionPin {
                profile_id: "profile_wachiland-elite-beta".into(),
                revision_id: "rev_1".into(),
                manifest_sha256: "a".repeat(64),
            },
            mods_fingerprint: "mods".into(),
            other_process_running: false,
            skip_once: false,
            disabled_by_setting: false,
            quick_play: None,
        }
    }

    #[test]
    fn prepare_records_then_uses_and_writes_forward_slash_paths() {
        let root = TestRoot::new("prepare");
        let java = root.fake_jdk("25.0.1+8-LTS", 10);
        let classpath = vec![root.jar("a.jar", 10)];
        let request = request(&root);
        let game_dir = root.instance();

        let first = prepare(&request, &facts(&java, &classpath, &game_dir));
        assert_eq!(first.flags[0], "-XX:AOTMode=record");
        assert!(first.flags[1].starts_with("-XX:AOTConfiguration="), "{}", first.flags[1]);
        assert!(!first.flags[1].contains('\\'), "{}", first.flags[1]);
        let Some(mut run) = first.run else {
            panic!("expected a record run");
        };
        let RunKind::Record { profile } = &run.kind else {
            panic!("expected a record run");
        };
        big_file(profile);
        run.dump_started = Some(Instant::now());
        assert_eq!(
            finish_run_after(&run, ExitKind::Exited(Some(0)), Duration::from_secs(300)),
            ExitAction::Recorded
        );
        assert!(Dir::new(&request.instance_root).read_timings().record_secs.is_some());

        // Until create ran, a Start is plain and keeps the profile.
        let waiting = prepare(&request, &facts(&java, &classpath, &game_dir));
        assert!(waiting.flags.is_empty() && waiting.run.is_none());
        let dir = Dir::new(&request.instance_root);
        assert!(matches!(create(&dir, Some(0)), CreateOutcome::Promoted { .. }));

        let second = prepare(&request, &facts(&java, &classpath, &game_dir));
        assert_eq!(second.flags.len(), 1);
        assert!(second.flags[0].starts_with("-XX:AOTCache="), "{}", second.flags[0]);
        assert!(!second.flags[0].contains('\\'));
        assert!(matches!(second.run, Some(ActiveRun { kind: RunKind::Use, .. })));

        // A -D property does not change the key; a VM argument that shapes the VM does.
        let mut with_property = facts(&java, &classpath, &game_dir);
        with_property.jvm_args.push(OsString::from("-Dfoo=bar"));
        assert!(prepare(&request, &with_property).flags[0].starts_with("-XX:AOTCache="));
        let mut with_heap = facts(&java, &classpath, &game_dir);
        with_heap.jvm_args = vec![OsString::from("-Xmx4g")];
        assert_eq!(prepare(&request, &with_heap).flags[0], "-XX:AOTMode=record");
    }

    #[test]
    fn prepare_changes_nothing_when_the_cache_does_not_apply() {
        let root = TestRoot::new("not-applicable");
        let java = root.fake_jdk("25.0.1+8-LTS", 10);
        let classpath = vec![root.jar("a.jar", 10)];
        let request = request(&root);
        let game_dir = root.instance();

        let cases: Vec<(&str, Box<dyn Fn(&mut Facts)>)> = vec![
            ("custom java", Box::new(|f| f.jvm_binary_enabled = true)),
            ("forced java", Box::new(|f| f.force_external_java = true)),
            ("java 21", Box::new(|f| f.java_major = 21)),
            ("sandbox", Box::new(|f| f.sandbox = true)),
            ("wrapper command", Box::new(|f| f.wrapper_command = true)),
            (
                "player aot flag",
                Box::new(|f| f.jvm_args.push(OsString::from("-XX:AOTCache=C:/mine.aot"))),
            ),
            (
                "player cds flag",
                Box::new(|f| f.jvm_args.push(OsString::from("-XX:SharedArchiveFile=C:/mine.jsa"))),
            ),
            ("player xshare", Box::new(|f| f.jvm_args.push(OsString::from("-Xshare:off")))),
        ];
        for (name, change) in cases {
            let mut facts = facts(&java, &classpath, &game_dir);
            change(&mut facts);
            let prepared = prepare(&request, &facts);
            assert!(prepared.flags.is_empty() && prepared.run.is_none(), "{name}");
        }
        assert!(
            !Dir::new(&request.instance_root).root.exists(),
            "nothing is created when the cache does not apply"
        );

        let mut retry = self::request(&root);
        retry.skip_once = true;
        assert!(prepare(&retry, &facts(&java, &classpath, &game_dir)).flags.is_empty());
    }

    #[test]
    fn the_instance_setting_turns_it_off_and_frees_the_disk() {
        let root = TestRoot::new("setting");
        let java = root.fake_jdk("25.0.1+8-LTS", 10);
        let classpath = vec![root.jar("a.jar", 10)];
        let game_dir = root.instance();
        let dir = Dir::new(&root.instance());
        promoted(&dir, &inputs());
        let mut request = request(&root);
        request.disabled_by_setting = true;
        let prepared = prepare(&request, &facts(&java, &classpath, &game_dir));
        assert!(prepared.flags.is_empty() && prepared.run.is_none());
        assert!(!dir.root.exists(), "turning the option off removes the instance's cache");
    }

    #[test]
    fn prepare_rejects_a_runtime_that_is_not_java_25_even_if_told_so() {
        let root = TestRoot::new("jdk21");
        let java = root.fake_jdk("21.0.7+6-LTS", 10);
        let classpath = vec![root.jar("a.jar", 10)];
        let game_dir = root.instance();
        let prepared = prepare(&request(&root), &facts(&java, &classpath, &game_dir));
        assert!(prepared.flags.is_empty() && prepared.run.is_none());
    }

    #[test]
    fn prepare_without_a_release_file_or_with_a_missing_jar_does_not_apply() {
        let root = TestRoot::new("unreadable");
        let java = root.fake_jdk("25.0.1+8-LTS", 10);
        let classpath = vec![root.jar("a.jar", 10)];
        let request = request(&root);
        let game_dir = root.instance();

        let missing_jar = vec![root.0.join("gone.jar")];
        assert!(prepare(&request, &facts(&java, &missing_jar, &game_dir)).flags.is_empty());

        fs::remove_file(java.parent().unwrap().parent().unwrap().join("release")).unwrap();
        assert!(prepare(&request, &facts(&java, &classpath, &game_dir)).flags.is_empty());
    }

    #[test]
    fn flag_paths_use_forward_slashes() {
        assert_eq!(flag_path(Path::new(r"C:\Users\x\aot-cache\a.aot")), "C:/Users/x/aot-cache/a.aot");
    }
}
