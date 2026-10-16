//! JDK 25 AOT cache for Wachiland global-profile instances on PC.
//!
//! The cache (JEP 483/514) is trained **during a normal game session**: the first Start of an
//! instance runs with `-XX:AOTCacheOutput=<file>`; when the game closes by itself the JVM
//! assembles the cache before it exits (one step). Later Starts of that instance run with
//! `-XX:AOTCache=<file>` and reach the main menu much faster.
//!
//! Layout, inside the instance directory so that deleting the instance deletes its cache and two
//! instances never share one:
//!
//! ```text
//! <instance>/aot-cache/
//!   state.json         the valid cache: key, file name, size (written last, atomically)
//!   <key16>.aot        the valid cache file (promoted by rename after a complete training)
//!   failure.json       how often this key failed (caps retraining)
//!   training/          one run in progress: run.json + train-<nonce>.aot (+ .aot.config)
//! ```
//!
//! Rules this module keeps:
//! * A cache is used only if `state.json` names the **current key** and the file has the recorded
//!   size. The key covers the exact JDK build, the applied profile revision, the profile's mod
//!   list, the classpath jars (name, size, mtime: the JVM checks the same) and the VM arguments.
//!   Any change retrains only that instance.
//! * Training output never has the cache name. It is promoted by rename only after the game
//!   exited with code 0, the JVM left no `.config` marker (it removes it when the assembly
//!   finished) and the file is plausible. Anything else (stop, crash, power loss, launcher kill)
//!   leaves a `training/` directory that is discarded at the next Start.
//! * The JVM runs in `AOTMode=auto`: a missing, truncated or mismatching cache is ignored with a
//!   message, never an error. If a run with the cache still exits with an error in the first
//!   [`EARLY_EXIT_WINDOW`], the cache is dropped, the failure is recorded and the Start is
//!   retried without it. Two failures for one key disable the feature for that key.
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
const FAILURE_FILE: &str = "failure.json";
const TRAINING_DIR: &str = "training";
const RUN_FILE: &str = "run.json";
const SCHEMA: u32 = 1;
/// A real cache of this pack is about 300 MB; anything under this is not a finished cache.
const MIN_CACHE_BYTES: u64 = 1024 * 1024;
/// Free space needed before a training run: the cache itself plus the JVM's temporary
/// configuration, with a margin.
pub(crate) const MIN_FREE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Failures of one key after which the cache is not tried again for that key.
const MAX_FAILURES: u32 = 2;
/// Set to `off` to disable the mechanism (developer / support switch).
const ENV_SWITCH: &str = "WBOMBO_AOT_CACHE";

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
    /// Another process of this instance is alive (it may be a training run): do not start a second.
    pub other_process_running: bool,
    /// This Start is the retry after a failed cache run: no cache, no training.
    pub skip_once: bool,
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
    pub jvm_binary_enabled: bool,
    pub force_external_java: bool,
    pub sandbox: bool,
    pub wrapper_command: bool,
}

#[derive(Debug)]
pub enum RunKind {
    /// Started with `-XX:AOTCache`.
    Use,
    /// Started with `-XX:AOTCacheOutput`; the assembled cache lands in `output`.
    Train { output: PathBuf },
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
    /// The player's Stop was already given extra time to let the JVM finish assembling.
    pub assembly_grace_used: bool,
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
    /// A training run produced a complete cache and it is now the valid one.
    Promoted,
    /// A training run produced nothing usable; it was discarded.
    Discarded,
    /// The run failed early with the cache: it was dropped, start again without it.
    RetryWithoutCache,
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

#[derive(Debug, Serialize, Deserialize)]
struct RunRecord {
    schema: u32,
    key: String,
    file: String,
    started_unix_secs: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct FailureRecord {
    key: String,
    count: u32,
    last_reason: String,
    last_unix_secs: u64,
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
    ["-XX:AOT", "-Xshare", "-XX:SharedArchiveFile", "-XX:ArchiveClassesAtExit", "-XX:+AutoCreateSharedArchive"]
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

    fn failure_path(&self) -> PathBuf {
        self.root.join(FAILURE_FILE)
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

    fn read_failure(&self) -> Option<FailureRecord> {
        serde_json::from_slice(&fs::read(self.failure_path()).ok()?).ok()
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

    fn record_failure(&self, key: &str, reason: &str) -> u32 {
        let count = match self.read_failure() {
            Some(previous) if previous.key == key => previous.count + 1,
            _ => 1,
        };
        let _ = fs::create_dir_all(&self.root);
        let _ = self.write_json(
            &self.failure_path(),
            &FailureRecord {
                key: key.to_owned(),
                count,
                last_reason: reason.to_owned(),
                last_unix_secs: unix_secs(),
            },
        );
        count
    }

    fn failure_count(&self, key: &str) -> u32 {
        self.read_failure().filter(|failure| failure.key == key).map_or(0, |failure| failure.count)
    }

    /// Promotes a finished training run: the file in `output` becomes the valid cache. A run
    /// left by a launcher that died is promoted too when it is provably complete (see
    /// [`complete_output`]).
    fn promote(&self, key: &str, output: &Path, jdk: &str, inputs: &KeyInputs) -> Result<u64, String> {
        let size = complete_output(output)?;
        let target = self.cache_file(key);
        let _ = fs::remove_file(&target);
        fs::rename(output, &target).map_err(|error| format!("rename-failed: {error}"))?;
        let state = CacheState {
            schema: SCHEMA,
            key: key.to_owned(),
            file: target.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
            size,
            created_unix_secs: unix_secs(),
            jdk: jdk.to_owned(),
            inputs: inputs.clone(),
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

/// Size of `output` if it is a finished cache: present, large enough and without the temporary
/// `.config` file the JVM writes next to it and removes only when the assembly finished.
fn complete_output(output: &Path) -> Result<u64, String> {
    let mut config = output.as_os_str().to_owned();
    config.push(".config");
    if Path::new(&config).exists() {
        return Err("assembly-unfinished".into());
    }
    let size = fs::metadata(output).map_err(|_| "no-output")?.len();
    if size < MIN_CACHE_BYTES {
        return Err(format!("output-too-small ({size} bytes)"));
    }
    Ok(size)
}

pub(crate) fn env_disabled() -> bool {
    std::env::var(ENV_SWITCH).is_ok_and(|value| value.eq_ignore_ascii_case("off") || value == "0")
}

/// Pre-checks that need nothing from the disk.
fn eligibility(request: &LaunchRequest, facts: &Facts) -> Result<(), Disabled> {
    if env_disabled() {
        return Err(Disabled::Silent("disabled-by-environment"));
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
    Use { cache: PathBuf, size: u64 },
    Train { output: PathBuf },
    Disabled(Disabled),
}

/// Looks at the directory and decides. Cleans up what is stale: an invalid cache, a leftover
/// training directory (promoting it first when it is provably complete).
pub(crate) fn decide(
    dir: &Dir,
    inputs: &KeyInputs,
    jdk: &str,
    other_process_running: bool,
    free_bytes: Option<u64>,
) -> Decision {
    let key = inputs.key();

    if !other_process_running {
        recover_or_discard_training(dir, &key, jdk, inputs);
    }

    if let Some((cache, size)) = dir.valid_cache(&key) {
        return Decision::Use { cache, size };
    }
    if dir.read_state().is_some() {
        log::info!("[aot-cache] cache for this instance no longer matches its key; removing it to train again");
        dir.remove_caches_except(None);
    }

    if dir.failure_count(&key) >= MAX_FAILURES {
        return Decision::Disabled(Disabled::Logged(format!(
            "disabled-after-{MAX_FAILURES}-failures (delete aot-cache/failure.json to try again)"
        )));
    }
    if other_process_running {
        return Decision::Disabled(Disabled::Silent("instance-already-running"));
    }
    if let Some(free) = free_bytes
        && free < MIN_FREE_BYTES
    {
        return Decision::Disabled(Disabled::Logged(format!("low-disk-space ({} MiB free)", free / (1024 * 1024))));
    }

    let training = dir.training();
    let _ = fs::remove_dir_all(&training);
    if let Err(error) = fs::create_dir_all(&training) {
        return Decision::Disabled(Disabled::Logged(format!("cannot-create-training-dir ({error})")));
    }
    let file = format!("train-{:016x}.aot", rand::random::<u64>());
    let record = RunRecord {
        schema: SCHEMA,
        key: key.clone(),
        file: file.clone(),
        started_unix_secs: unix_secs(),
    };
    if let Err(error) = dir.write_json(&training.join(RUN_FILE), &record) {
        dir.discard_training();
        return Decision::Disabled(Disabled::Logged(format!("cannot-write-run-record ({error})")));
    }
    Decision::Train {
        output: training.join(file),
    }
}

/// A `training/` directory with no live process behind it: promote it if the run was complete
/// and for this key (the launcher died after the JVM finished), otherwise delete it.
fn recover_or_discard_training(dir: &Dir, key: &str, jdk: &str, inputs: &KeyInputs) {
    let training = dir.training();
    if !training.exists() {
        return;
    }
    let record: Option<RunRecord> = fs::read(training.join(RUN_FILE)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok());
    if let Some(record) = record
        && record.schema == SCHEMA
        && record.key == key
        && !record.file.contains(['/', '\\'])
    {
        match dir.promote(key, &training.join(&record.file), jdk, inputs) {
            Ok(size) => {
                log::info!("[aot-cache] recovered a finished training run left by an earlier session ({} MiB)", size / (1024 * 1024));
                return;
            },
            Err(reason) => log::info!("[aot-cache] discarding an unfinished training run: {reason}"),
        }
    } else {
        log::info!("[aot-cache] discarding a training run for another key");
    }
    dir.discard_training();
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
    log::warn!("[WBOMBO-COMPAT] FALLBACK project=Pandora mechanism={MECHANISM} mod=java@{java_version} reason={reason}");
}

/// Decides for this Start and returns the VM arguments to add. Never fails: every problem is a
/// plain start.
pub(crate) fn prepare(request: &LaunchRequest, facts: &Facts) -> Prepared {
    let identity = jdk_identity(facts.java_path);
    let java_version = identity.as_ref().map_or("unknown", |(version, _)| version.as_str()).to_owned();

    let result = (|| -> Result<(Decision, KeyInputs, String), Disabled> {
        eligibility(request, facts)?;
        let (version, jdk_hash) = identity.ok_or_else(|| Disabled::Logged("jdk-release-unreadable".into()))?;
        if runtime_major(&version) != Some(java_default::JAVA_MAJOR) {
            return Err(Disabled::Silent("java-not-25"));
        }
        let classpath = classpath_fingerprint(facts.classpath)
            .ok_or_else(|| Disabled::Logged("classpath-unreadable".into()))?;
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
    match decision {
        Decision::Use { cache, size } => {
            log_active(&version, "use", &key, &format!(" size_mib={}", size / (1024 * 1024)));
            Prepared {
                flags: vec![format!("-XX:AOTCache={}", flag_path(&cache))],
                run: Some(ActiveRun {
                    kind: RunKind::Use,
                    dir: dir.root,
                    key,
                    inputs,
                    java_version: version,
                    started: Instant::now(),
                    quick_play: request.quick_play.clone(),
                    assembly_grace_used: false,
                }),
            }
        },
        Decision::Train { output } => {
            log_active(&version, "train", &key, "");
            Prepared {
                flags: vec![format!("-XX:AOTCacheOutput={}", flag_path(&output))],
                run: Some(ActiveRun {
                    kind: RunKind::Train { output },
                    dir: dir.root,
                    key,
                    inputs,
                    java_version: version,
                    started: Instant::now(),
                    quick_play: request.quick_play.clone(),
                    assembly_grace_used: false,
                }),
            }
        },
        Decision::Disabled(disabled) => {
            log_disabled(&version, &disabled);
            Prepared::none()
        },
    }
}

fn log_disabled(java_version: &str, disabled: &Disabled) {
    match disabled {
        Disabled::Silent(reason) => log::debug!("[aot-cache] not used: {reason}"),
        Disabled::Logged(reason) => log_fallback(java_version, reason),
    }
}

/// A training run whose game already exited and whose JVM is assembling the cache (the JVM's
/// temporary `.config` file exists). The launcher gives a Stop more time in that phase instead of
/// killing the JVM and losing the cache.
pub(crate) fn is_assembling(run: &ActiveRun) -> bool {
    match &run.kind {
        RunKind::Train { output } => {
            let mut config = output.as_os_str().to_owned();
            config.push(".config");
            Path::new(&config).exists()
        },
        RunKind::Use => false,
    }
}

/// Extra time a Stop gives an assembling JVM before the launcher kills it anyway.
pub(crate) const ASSEMBLY_GRACE: Duration = Duration::from_secs(600);

/// Handles the end of a tracked process: promotes a finished training, drops a cache that
/// failed early, and tells the caller whether to start again without the cache.
pub(crate) fn finish_run(run: &ActiveRun, exit: ExitKind) -> ExitAction {
    finish_run_after(run, exit, run.started.elapsed())
}

fn finish_run_after(run: &ActiveRun, exit: ExitKind, lived: Duration) -> ExitAction {
    let dir = Dir {
        root: run.dir.clone(),
    };

    // A Stop is the player's doing: never an AOT failure, never a retry.
    if let ExitKind::Exited(code) = exit
        && let Some(reason) = java_default::early_exit_reason(code, lived)
    {
        let failures = dir.record_failure(&run.key, &reason);
        dir.remove_caches_except(None);
        dir.discard_training();
        log_fallback(&run.java_version, &format!("early-exit ({reason}); retrying without the cache, failures={failures}"));
        return ExitAction::RetryWithoutCache;
    }

    match (&run.kind, exit) {
        (RunKind::Use, ExitKind::Exited(Some(0))) => {
            let _ = fs::remove_file(dir.failure_path());
            ExitAction::Nothing
        },
        (RunKind::Use, _) => ExitAction::Nothing,
        (RunKind::Train { .. }, ExitKind::Stopped(code)) if code != Some(0) => {
            dir.discard_training();
            log::info!("[aot-cache] training discarded: the game was stopped before the cache was assembled");
            ExitAction::Discarded
        },
        (RunKind::Train { .. }, ExitKind::Exited(code)) if code != Some(0) => {
            dir.discard_training();
            log::info!("[aot-cache] training discarded: the game exited with {code:?}");
            ExitAction::Discarded
        },
        (RunKind::Train { output }, ExitKind::Exited(_) | ExitKind::Stopped(_)) => {
            match dir.promote(&run.key, output, &run.java_version, &run.inputs) {
                Ok(size) => {
                    log_active(
                        &run.java_version,
                        "trained",
                        &run.key,
                        &format!(" size_mib={} seconds_since_start={}", size / (1024 * 1024), lived.as_secs()),
                    );
                    ExitAction::Promoted
                },
                Err(reason) => {
                    dir.discard_training();
                    log::info!("[aot-cache] training discarded: {reason}");
                    ExitAction::Discarded
                },
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir()
                .join(format!("pandora-aot-{label}-{}", uuid::Uuid::from_bytes(rand::random())));
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

    fn big_file(path: &Path) {
        fs::write(path, vec![7u8; MIN_CACHE_BYTES as usize + 10]).unwrap();
    }

    /// Runs `decide` for a fresh training and returns its output path.
    fn train_output(dir: &Dir, inputs: &KeyInputs) -> PathBuf {
        match decide(dir, inputs, "25.0.1+8-LTS", false, Some(u64::MAX)) {
            Decision::Train { output } => output,
            other => panic!("expected Train, got {other:?}"),
        }
    }

    fn run_for(dir: &Dir, inputs: &KeyInputs, output: PathBuf) -> ActiveRun {
        ActiveRun {
            kind: RunKind::Train { output },
            dir: dir.root.clone(),
            key: inputs.key(),
            inputs: inputs.clone(),
            java_version: "25.0.1+8-LTS".into(),
            started: Instant::now(),
            quick_play: None,
            assembly_grace_used: false,
        }
    }

    fn use_run(dir: &Dir, inputs: &KeyInputs) -> ActiveRun {
        ActiveRun {
            kind: RunKind::Use,
            ..run_for(dir, inputs, PathBuf::new())
        }
    }

    /// Trains and promotes a cache for `inputs`, returning its path.
    fn promoted(dir: &Dir, inputs: &KeyInputs) -> PathBuf {
        let output = train_output(dir, inputs);
        big_file(&output);
        let run = run_for(dir, inputs, output);
        assert_eq!(
            finish_run_after(&run, ExitKind::Exited(Some(0)), Duration::from_secs(600)),
            ExitAction::Promoted
        );
        dir.cache_file(&inputs.key())
    }

    fn assert_trains(dir: &Dir, inputs: &KeyInputs) {
        assert!(matches!(
            decide(dir, inputs, "25.0.1+8-LTS", false, Some(u64::MAX)),
            Decision::Train { .. }
        ));
    }

    fn assert_uses(dir: &Dir, inputs: &KeyInputs) {
        assert!(matches!(
            decide(dir, inputs, "25.0.1+8-LTS", false, Some(u64::MAX)),
            Decision::Use { .. }
        ));
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
        assert_ne!(before, classpath_fingerprint(&[b.clone(), a.clone()]).unwrap(), "order matters, as for the JVM");
        root.jar("b.jar", 21);
        assert_ne!(before, classpath_fingerprint(&[a.clone(), b.clone()]).unwrap());
        fs::remove_file(&b).unwrap();
        assert!(classpath_fingerprint(&[a, b]).is_none());
    }

    #[test]
    fn first_start_trains_and_a_complete_run_becomes_the_cache() {
        let root = TestRoot::new("flow");
        let dir = Dir::new(&root.instance());
        let inputs = inputs();
        let output = train_output(&dir, &inputs);
        assert!(output.starts_with(dir.training()));
        assert!(dir.training().join(RUN_FILE).exists());
        assert!(dir.valid_cache(&inputs.key()).is_none());

        let cache = promoted(&dir, &inputs);
        assert_eq!(cache, dir.cache_file(&inputs.key()));
        assert!(!dir.training().exists(), "the training directory is gone after promotion");
        assert!(matches!(
            decide(&dir, &inputs, "25.0.1+8-LTS", false, Some(u64::MAX)),
            Decision::Use { cache: used, .. } if used == cache
        ));
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
        let output = train_output(&dir, &new_revision);
        assert!(!old_cache.exists(), "the stale cache is removed");
        assert!(dir.read_state().is_none());
        assert!(other_cache.exists(), "another instance keeps its own cache");
        assert_uses(&other_instance, &old);

        big_file(&output);
        let run = run_for(&dir, &new_revision, output);
        assert_eq!(
            finish_run_after(&run, ExitKind::Exited(Some(0)), Duration::from_secs(300)),
            ExitAction::Promoted
        );
        assert!(dir.cache_file(&new_revision.key()).exists());
        assert!(!old_cache.exists());
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
        assert_trains(&dir, &inputs);
        assert!(!cache.exists());
    }

    #[test]
    fn a_missing_cache_file_or_unreadable_state_trains_again() {
        let root = TestRoot::new("missing");
        let dir = Dir::new(&root.instance());
        let inputs = inputs();
        let cache = promoted(&dir, &inputs);
        fs::remove_file(&cache).unwrap();
        assert_trains(&dir, &inputs);

        promoted(&dir, &inputs);
        fs::write(dir.state_path(), b"{ not json").unwrap();
        assert_trains(&dir, &inputs);
    }

    #[test]
    fn partial_training_is_discarded_and_never_promoted() {
        let root = TestRoot::new("partial");
        let dir = Dir::new(&root.instance());
        let inputs = inputs();

        // Power loss or launcher kill in the middle of the JVM's assembly: the output exists but
        // the temporary configuration was not removed.
        let output = train_output(&dir, &inputs);
        big_file(&output);
        let mut config = output.as_os_str().to_owned();
        config.push(".config");
        fs::write(&config, b"temporary").unwrap();
        let next = train_output(&dir, &inputs);
        assert!(!output.exists(), "the partial file was removed");
        assert!(dir.valid_cache(&inputs.key()).is_none());
        assert_ne!(next, output);

        // A leftover that is too small is not a cache either.
        fs::write(&next, b"tiny").unwrap();
        train_output(&dir, &inputs);
        assert!(!next.exists());
        assert!(dir.valid_cache(&inputs.key()).is_none());
    }

    #[test]
    fn a_finished_run_left_by_a_dead_launcher_is_recovered() {
        let root = TestRoot::new("recover");
        let dir = Dir::new(&root.instance());
        let inputs = inputs();
        let output = train_output(&dir, &inputs);
        big_file(&output); // finished: big enough and no .config
        assert_uses(&dir, &inputs);
        assert!(!dir.training().exists());
    }

    #[test]
    fn a_leftover_for_another_key_is_discarded_not_promoted() {
        let root = TestRoot::new("otherkey");
        let dir = Dir::new(&root.instance());
        let old = inputs();
        let output = train_output(&dir, &old);
        big_file(&output);
        let mut new_revision = inputs();
        new_revision.revision_id = "rev_2".into();
        assert_trains(&dir, &new_revision);
        assert!(dir.valid_cache(&old.key()).is_none());
        assert!(!output.exists());
    }

    #[test]
    fn a_live_process_blocks_a_second_training_and_keeps_its_directory() {
        let root = TestRoot::new("live");
        let dir = Dir::new(&root.instance());
        let inputs = inputs();
        let output = train_output(&dir, &inputs);
        let decision = decide(&dir, &inputs, "j", true, Some(u64::MAX));
        assert_eq!(decision, Decision::Disabled(Disabled::Silent("instance-already-running")));
        assert!(output.parent().unwrap().exists(), "the running training is untouched");
    }

    #[test]
    fn a_live_process_may_still_use_a_valid_cache() {
        let root = TestRoot::new("live-use");
        let dir = Dir::new(&root.instance());
        let inputs = inputs();
        promoted(&dir, &inputs);
        assert!(matches!(decide(&dir, &inputs, "j", true, Some(u64::MAX)), Decision::Use { .. }));
    }

    #[test]
    fn low_disk_space_skips_training_with_a_logged_reason() {
        let root = TestRoot::new("disk");
        let dir = Dir::new(&root.instance());
        let decision = decide(&dir, &inputs(), "j", false, Some(MIN_FREE_BYTES - 1));
        assert!(
            matches!(decision, Decision::Disabled(Disabled::Logged(reason)) if reason.starts_with("low-disk-space"))
        );
        assert!(!dir.training().exists());
        assert!(
            matches!(decide(&dir, &inputs(), "j", false, None), Decision::Train { .. }),
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
        assert_eq!(action, ExitAction::RetryWithoutCache);
        assert!(!cache.exists());
        assert!(dir.read_state().is_none());
        assert_eq!(dir.failure_count(&inputs.key()), 1);
        // One failure: the next Start trains again.
        assert_trains(&dir, &inputs);
    }

    #[test]
    fn an_early_error_during_training_retries_without_the_cache_and_discards_the_run() {
        let root = TestRoot::new("early-train");
        let dir = Dir::new(&root.instance());
        let inputs = inputs();
        let output = train_output(&dir, &inputs);
        let run = run_for(&dir, &inputs, output);
        assert_eq!(
            finish_run_after(&run, ExitKind::Exited(Some(1)), Duration::from_secs(2)),
            ExitAction::RetryWithoutCache
        );
        assert!(!dir.training().exists());
        assert_eq!(dir.failure_count(&inputs.key()), 1);
    }

    #[test]
    fn two_failures_for_one_key_disable_the_cache_until_the_key_changes() {
        let root = TestRoot::new("cap");
        let dir = Dir::new(&root.instance());
        let inputs = inputs();
        for _ in 0..MAX_FAILURES {
            promoted(&dir, &inputs);
            finish_run_after(&use_run(&dir, &inputs), ExitKind::Exited(Some(1)), Duration::from_secs(1));
        }
        assert!(matches!(
            decide(&dir, &inputs, "j", false, Some(u64::MAX)),
            Decision::Disabled(Disabled::Logged(reason)) if reason.starts_with("disabled-after-")
        ));
        let mut new_revision = inputs.clone();
        new_revision.revision_id = "rev_2".into();
        assert_trains(&dir, &new_revision);
    }

    #[test]
    fn a_healthy_run_with_the_cache_clears_its_failure_count() {
        let root = TestRoot::new("heal");
        let dir = Dir::new(&root.instance());
        let inputs = inputs();
        promoted(&dir, &inputs);
        finish_run_after(&use_run(&dir, &inputs), ExitKind::Exited(Some(1)), Duration::from_secs(1));
        promoted(&dir, &inputs);
        assert_eq!(dir.failure_count(&inputs.key()), 1);
        let action = finish_run_after(&use_run(&dir, &inputs), ExitKind::Exited(Some(0)), Duration::from_secs(900));
        assert_eq!(action, ExitAction::Nothing);
        assert_eq!(dir.failure_count(&inputs.key()), 0);
    }

    #[test]
    fn a_game_crash_late_in_a_session_keeps_the_cache_and_does_not_count() {
        let root = TestRoot::new("late-crash");
        let dir = Dir::new(&root.instance());
        let inputs = inputs();
        let cache = promoted(&dir, &inputs);
        let action = finish_run_after(&use_run(&dir, &inputs), ExitKind::Exited(Some(255)), Duration::from_secs(400));
        assert_eq!(action, ExitAction::Nothing);
        assert!(cache.exists());
        assert_eq!(dir.failure_count(&inputs.key()), 0);
    }

    #[test]
    fn training_that_ends_in_a_crash_or_a_stop_leaves_no_cache() {
        let root = TestRoot::new("crash-train");
        let dir = Dir::new(&root.instance());
        let inputs = inputs();

        for exit in [
            ExitKind::Exited(Some(255)),
            ExitKind::Exited(None),
            ExitKind::Stopped(None),
            ExitKind::Stopped(Some(1)),
        ] {
            let output = train_output(&dir, &inputs);
            big_file(&output); // even a plausible file must not be promoted after an unclean end
            let run = run_for(&dir, &inputs, output);
            assert_eq!(finish_run_after(&run, exit, Duration::from_secs(400)), ExitAction::Discarded, "{exit:?}");
            assert!(dir.valid_cache(&inputs.key()).is_none());
            assert!(!dir.training().exists());
        }
        assert_eq!(dir.failure_count(&inputs.key()), 0, "crashes of the game are not the cache's failure");
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
    fn a_stop_after_a_finished_assembly_still_promotes() {
        let root = TestRoot::new("stop-done");
        let dir = Dir::new(&root.instance());
        let inputs = inputs();
        let output = train_output(&dir, &inputs);
        big_file(&output);
        let run = run_for(&dir, &inputs, output);
        assert_eq!(
            finish_run_after(&run, ExitKind::Stopped(Some(0)), Duration::from_secs(600)),
            ExitAction::Promoted
        );
    }

    #[test]
    fn assembling_is_visible_through_the_temporary_config() {
        let root = TestRoot::new("assembling");
        let dir = Dir::new(&root.instance());
        let inputs = inputs();
        let output = train_output(&dir, &inputs);
        let run = run_for(&dir, &inputs, output.clone());
        assert!(!is_assembling(&run));
        let mut config = output.as_os_str().to_owned();
        config.push(".config");
        fs::write(&config, b"x").unwrap();
        assert!(is_assembling(&run));
        assert!(!is_assembling(&use_run(&dir, &inputs)));
    }

    fn facts<'a>(java: &'a Path, classpath: &'a [PathBuf]) -> Facts<'a> {
        Facts {
            java_path: java,
            java_major: 25,
            classpath,
            jvm_args: vec![OsString::from("-Xmx8g")],
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
            quick_play: None,
        }
    }

    /// Rebuilds the `ActiveRun` the way `prepare` makes it, for a given output.
    fn prepare_run_for_exit(request: &LaunchRequest, java: &Path, classpath: &[PathBuf], output: PathBuf) -> ActiveRun {
        let facts = facts(java, classpath);
        let (version, jdk) = jdk_identity(java).unwrap();
        let inputs = KeyInputs {
            jdk,
            profile_id: request.pin.profile_id.clone(),
            revision_id: request.pin.revision_id.clone(),
            manifest_sha256: request.pin.manifest_sha256.clone(),
            mods: request.mods_fingerprint.clone(),
            classpath: classpath_fingerprint(classpath).unwrap(),
            jvm_args: jvm_args_fingerprint(&facts.jvm_args),
        };
        let mut run = run_for(&Dir::new(&request.instance_root), &inputs, output);
        run.java_version = version;
        run
    }

    #[test]
    fn prepare_trains_then_uses_and_writes_forward_slash_paths() {
        let root = TestRoot::new("prepare");
        let java = root.fake_jdk("25.0.1+8-LTS", 10);
        let classpath = vec![root.jar("a.jar", 10)];
        let request = request(&root);

        let first = prepare(&request, &facts(&java, &classpath));
        assert_eq!(first.flags.len(), 1);
        let flag = &first.flags[0];
        assert!(flag.starts_with("-XX:AOTCacheOutput="), "{flag}");
        assert!(!flag.contains('\\'), "{flag}");
        let Some(ActiveRun {
            kind: RunKind::Train { output },
            ..
        }) = first.run
        else {
            panic!("expected a training run");
        };
        big_file(&output);
        let run = prepare_run_for_exit(&request, &java, &classpath, output);
        assert_eq!(
            finish_run_after(&run, ExitKind::Exited(Some(0)), Duration::from_secs(300)),
            ExitAction::Promoted
        );

        let second = prepare(&request, &facts(&java, &classpath));
        assert_eq!(second.flags.len(), 1);
        assert!(second.flags[0].starts_with("-XX:AOTCache="), "{}", second.flags[0]);
        assert!(!second.flags[0].contains('\\'));
        assert!(matches!(second.run, Some(ActiveRun { kind: RunKind::Use, .. })));

        // A -D property does not change the key; a VM argument that shapes the VM does.
        let mut with_property = facts(&java, &classpath);
        with_property.jvm_args.push(OsString::from("-Dfoo=bar"));
        assert!(prepare(&request, &with_property).flags[0].starts_with("-XX:AOTCache="));
        let mut with_heap = facts(&java, &classpath);
        with_heap.jvm_args = vec![OsString::from("-Xmx4g")];
        assert!(prepare(&request, &with_heap).flags[0].starts_with("-XX:AOTCacheOutput="));
    }

    #[test]
    fn prepare_changes_nothing_when_the_cache_does_not_apply() {
        let root = TestRoot::new("not-applicable");
        let java = root.fake_jdk("25.0.1+8-LTS", 10);
        let classpath = vec![root.jar("a.jar", 10)];
        let request = request(&root);

        let cases: Vec<(&str, Box<dyn Fn(&mut Facts)>)> = vec![
            ("custom java", Box::new(|f| f.jvm_binary_enabled = true)),
            ("forced java", Box::new(|f| f.force_external_java = true)),
            ("java 21", Box::new(|f| f.java_major = 21)),
            ("sandbox", Box::new(|f| f.sandbox = true)),
            ("wrapper command", Box::new(|f| f.wrapper_command = true)),
            ("player aot flag", Box::new(|f| f.jvm_args.push(OsString::from("-XX:AOTCache=C:/mine.aot")))),
            ("player cds flag", Box::new(|f| f.jvm_args.push(OsString::from("-XX:SharedArchiveFile=C:/mine.jsa")))),
            ("player xshare", Box::new(|f| f.jvm_args.push(OsString::from("-Xshare:off")))),
        ];
        for (name, change) in cases {
            let mut facts = facts(&java, &classpath);
            change(&mut facts);
            let prepared = prepare(&request, &facts);
            assert!(prepared.flags.is_empty() && prepared.run.is_none(), "{name}");
        }
        assert!(!Dir::new(&request.instance_root).root.exists(), "nothing is created when the cache does not apply");

        let mut retry = self::request(&root);
        retry.skip_once = true;
        assert!(prepare(&retry, &facts(&java, &classpath)).flags.is_empty());
    }

    #[test]
    fn prepare_rejects_a_runtime_that_is_not_java_25_even_if_told_so() {
        let root = TestRoot::new("jdk21");
        let java = root.fake_jdk("21.0.7+6-LTS", 10);
        let classpath = vec![root.jar("a.jar", 10)];
        let prepared = prepare(&request(&root), &facts(&java, &classpath));
        assert!(prepared.flags.is_empty() && prepared.run.is_none());
    }

    #[test]
    fn prepare_without_a_release_file_or_with_a_missing_jar_does_not_apply() {
        let root = TestRoot::new("unreadable");
        let java = root.fake_jdk("25.0.1+8-LTS", 10);
        let classpath = vec![root.jar("a.jar", 10)];
        let request = request(&root);

        let missing_jar = vec![root.0.join("gone.jar")];
        assert!(prepare(&request, &facts(&java, &missing_jar)).flags.is_empty());

        fs::remove_file(java.parent().unwrap().parent().unwrap().join("release")).unwrap();
        assert!(prepare(&request, &facts(&java, &classpath)).flags.is_empty());
    }

    #[test]
    fn flag_paths_use_forward_slashes() {
        assert_eq!(flag_path(Path::new(r"C:\Users\x\aot-cache\a.aot")), "C:/Users/x/aot-cache/a.aot");
    }
}
