use std::collections::BTreeMap;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const SCHEMA_VERSION: u32 = 1;
const UPSTREAM_DEFAULT: &str = "4eb6c7849561151695288443c106519774ee05ea";
const HELPER_VERSION: &str = env!("CARGO_PKG_VERSION");
static UNIQUE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Plan,
    Auto,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CacheState {
    Absent,
    Generating,
    Ready,
    Stale,
    Failed,
}

impl CacheState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Absent => "ABSENT",
            Self::Generating => "GENERATING",
            Self::Ready => "READY",
            Self::Stale => "STALE",
            Self::Failed => "FAILED",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PrepareDecision {
    Stock,
    Train,
    Ready,
}

impl PrepareDecision {
    fn as_str(self) -> &'static str {
        match self {
            Self::Stock => "STOCK",
            Self::Train => "TRAIN",
            Self::Ready => "READY",
        }
    }
}

#[derive(Debug)]
struct ParsedArgs {
    instance_dir: PathBuf,
    launcher_exe: Option<PathBuf>,
    upstream_commit: String,
    java_exe: OsString,
    java_args: Vec<OsString>,
}

#[derive(Debug, Clone)]
struct EncodedOs {
    display: String,
    encoded_hex: String,
}

#[derive(Debug, Clone)]
struct Artifact {
    role: &'static str,
    path: EncodedOs,
    size: u64,
    sha256: String,
}

#[derive(Debug, Clone)]
struct ArgFingerprint {
    index: usize,
    kind: &'static str,
    sha256: String,
    safe_literal: Option<String>,
}

#[derive(Debug, Clone)]
struct LaunchPlan {
    bytes: Vec<u8>,
    sha256: String,
    eligible: bool,
}

#[derive(Debug, Clone)]
struct ReadyMetadata {
    plan_sha256: String,
    archive_sha256: String,
    archive_size: u64,
    helper_version: String,
}

struct LockGuard {
    _file: File,
    #[cfg(not(windows))]
    path: PathBuf,
}

#[cfg(not(windows))]
impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn main() {
    let parsed = match parse_args(env::args_os().skip(1).collect()) {
        Ok(v) => v,
        Err(_) => {
            eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=invalid-control-args");
            println!("STOCK");
            return;
        }
    };

    match prepare_launch(&parsed) {
        Ok(decision) => println!("{}", decision.as_str()),
        Err(_) => {
            eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=helper-error");
            println!("STOCK");
        }
    }
}

fn prepare_launch(parsed: &ParsedArgs) -> io::Result<PrepareDecision> {
    let mode = match env::var("BOOTOPTIM_APPCDS_MODE").ok().as_deref() {
        Some("auto") => Mode::Auto,
        _ => Mode::Plan,
    };

    let cache_dir = parsed.instance_dir.join(".bootoptim").join("appcds");
    fs::create_dir_all(&cache_dir)?;

    // Hashing is read-only and can happen outside the cache lock. Publication of
    // the plan and every state transition is serialized so launch-plan.match can
    // never describe a different concurrent preflight.
    let plan = build_launch_plan(parsed)?;
    let Some(_lock) = try_lock(&cache_dir.join("cache.lock"))? else {
        eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=lock-busy");
        return Ok(PrepareDecision::Stock);
    };
    let stable = persist_plan_and_compare(&cache_dir, &plan.bytes, &plan.sha256)?;

    if mode == Mode::Plan {
        eprintln!("BOOTOPTIM_INTERPOSER status=plan-only deterministic={}", if stable { "true" } else { "false" });
        return Ok(PrepareDecision::Stock);
    }

    if !cfg!(windows) {
        eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=appcds-windows-only-v0");
        return Ok(PrepareDecision::Stock);
    }

    if !stable {
        write_state(&cache_dir, CacheState::Absent, "plan-not-yet-proven")?;
        eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=plan-not-yet-proven");
        return Ok(PrepareDecision::Stock);
    }

    if !plan.eligible {
        write_state(&cache_dir, CacheState::Failed, "identity-ineligible")?;
        eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=identity-ineligible");
        return Ok(PrepareDecision::Stock);
    }

    let (state, _) = classify_cache(&cache_dir, &plan.sha256)?;
    match state {
        CacheState::Ready => {
            cleanup_training_files(&cache_dir);
            write_state(&cache_dir, CacheState::Ready, "identity-match")?;
            eprintln!("BOOTOPTIM_INTERPOSER status=ready activation=enabled");
            return Ok(PrepareDecision::Ready);
        }
        CacheState::Stale => {
            write_state(&cache_dir, CacheState::Stale, "identity-mismatch")?;
            eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=stale");
            return Ok(PrepareDecision::Stock);
        }
        CacheState::Failed | CacheState::Generating => {
            cleanup_orphan_staging(&cache_dir)?;
            write_state(&cache_dir, CacheState::Failed, "incomplete-or-failed")?;
            eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=failed-state");
            return Ok(PrepareDecision::Stock);
        }
        CacheState::Absent => {}
    }

    let training_meta = cache_dir.join("training.meta");
    let training_archive = cache_dir.join("training.jsa");
    let training_complete = cache_dir.join("training.complete");
    if training_meta.is_file() {
        let pending_plan = read_training_plan(&training_meta)?;
        if pending_plan != plan.sha256 {
            write_state(&cache_dir, CacheState::Stale, "training-plan-mismatch")?;
            eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=training-plan-mismatch");
            return Ok(PrepareDecision::Stock);
        }

        if !training_complete.is_file() {
            write_state(&cache_dir, CacheState::Generating, "training-pending")?;
            eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=training-pending");
            return Ok(PrepareDecision::Stock);
        }

        if !training_archive.is_file() {
            write_state(&cache_dir, CacheState::Failed, "training-complete-without-archive")?;
            eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=training-complete-without-archive");
            return Ok(PrepareDecision::Stock);
        }

        if fs::metadata(&training_archive)?.len() == 0 {
            write_state(&cache_dir, CacheState::Failed, "training-empty")?;
            eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=training-empty");
            return Ok(PrepareDecision::Stock);
        }

        promote_archive(&cache_dir, &training_archive, &plan.sha256)?;
        let _ = fs::remove_file(&training_meta);
        let _ = fs::remove_file(&training_complete);
        write_state(&cache_dir, CacheState::Ready, "promotion-complete")?;
        eprintln!("BOOTOPTIM_INTERPOSER status=ready promotion=complete");
        return Ok(PrepareDecision::Ready);
    }

    if training_archive.exists() || training_complete.exists() {
        write_state(&cache_dir, CacheState::Failed, "orphan-training-state")?;
        eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=orphan-training-state");
        return Ok(PrepareDecision::Stock);
    }

    write_atomic_replace(&training_meta, format!("schema={}\nplan_sha256={}\n", SCHEMA_VERSION, plan.sha256).as_bytes())?;
    write_state(&cache_dir, CacheState::Generating, "training")?;
    eprintln!("BOOTOPTIM_INTERPOSER status=generating activation=training");
    Ok(PrepareDecision::Train)
}

fn read_training_plan(path: &Path) -> io::Result<String> {
    let text = fs::read_to_string(path)?;
    let mut schema_ok = false;
    let mut plan = None;
    for line in text.lines() {
        if line == format!("schema={}", SCHEMA_VERSION) {
            schema_ok = true;
        } else if let Some(value) = line.strip_prefix("plan_sha256=") {
            plan = Some(value.to_string());
        }
    }
    if !schema_ok {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "training schema"));
    }
    plan.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "training plan"))
}

fn cleanup_training_files(cache_dir: &Path) {
    let _ = fs::remove_file(cache_dir.join("training.meta"));
    let _ = fs::remove_file(cache_dir.join("training.jsa"));
    let _ = fs::remove_file(cache_dir.join("training.complete"));
}

fn parse_args(args: Vec<OsString>) -> io::Result<ParsedArgs> {
    let mut instance_dir = None;
    let mut launcher_exe = None;
    let mut upstream_commit = UPSTREAM_DEFAULT.to_string();
    let mut i = 0usize;
    while i < args.len() {
        if args[i] == OsStr::new("--") {
            i += 1;
            break;
        }
        if args[i] == OsStr::new("--instance-dir") {
            i += 1;
            instance_dir = args.get(i).map(PathBuf::from);
        } else if args[i] == OsStr::new("--launcher-exe") {
            i += 1;
            launcher_exe = args.get(i).map(PathBuf::from);
        } else if args[i] == OsStr::new("--upstream-commit") {
            i += 1;
            upstream_commit = args.get(i).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        } else {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "unknown control argument"));
        }
        i += 1;
    }
    let java_exe = args.get(i).cloned().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing java"))?;
    let java_args = args.get(i + 1..).unwrap_or_default().to_vec();
    let instance_dir = instance_dir.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing instance"))?;
    Ok(ParsedArgs { instance_dir, launcher_exe, upstream_commit, java_exe, java_args })
}
