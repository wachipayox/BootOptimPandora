use std::collections::BTreeMap;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
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
            std::process::exit(2);
        }
    };

    let code = match run(&parsed) {
        Ok(code) => code,
        Err(_) => {
            eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=helper-error");
            spawn_java(&parsed.java_exe, &parsed.java_args, &[])
                .ok()
                .and_then(|status| status.code())
                .unwrap_or(1)
        }
    };
    std::process::exit(code);
}

fn run(parsed: &ParsedArgs) -> io::Result<i32> {
    let mode = match env::var("BOOTOPTIM_APPCDS_MODE").ok().as_deref() {
        Some("auto") => Mode::Auto,
        _ => Mode::Plan,
    };

    let cache_dir = parsed.instance_dir.join(".bootoptim").join("appcds");
    fs::create_dir_all(&cache_dir)?;

    let plan = build_launch_plan(parsed)?;
    let stable = persist_plan_and_compare(&cache_dir, &plan.bytes, &plan.sha256)?;

    if mode == Mode::Plan {
        eprintln!("BOOTOPTIM_INTERPOSER status=plan-only deterministic={}", if stable { "true" } else { "false" });
        return spawn_java(&parsed.java_exe, &parsed.java_args, &[])?.code().map_or(1, |c| c);
    }

    if !cfg!(windows) {
        eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=appcds-windows-only-v0");
        return spawn_java(&parsed.java_exe, &parsed.java_args, &[])?.code().map_or(1, |c| c);
    }

    if !stable {
        write_state(&cache_dir, CacheState::Absent, "plan-not-yet-proven")?;
        eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=plan-not-yet-proven");
        return spawn_java(&parsed.java_exe, &parsed.java_args, &[])?.code().map_or(1, |c| c);
    }

    if !plan.eligible {
        write_state(&cache_dir, CacheState::Failed, "identity-ineligible")?;
        eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=identity-ineligible");
        return spawn_java(&parsed.java_exe, &parsed.java_args, &[])?.code().map_or(1, |c| c);
    }

    let Some(_lock) = try_lock(&cache_dir.join("cache.lock"))? else {
        eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=lock-busy");
        return spawn_java(&parsed.java_exe, &parsed.java_args, &[])?.code().map_or(1, |c| c);
    };

    let (state, ready_meta) = classify_cache(&cache_dir, &plan.sha256)?;

    match state {
        CacheState::Ready => {
            let ready_path = cache_dir.join("ready.jsa");
            write_state(&cache_dir, CacheState::Ready, "identity-match")?;
            let flags = shared_archive_flags(CacheState::Ready, &ready_path);
            eprintln!("BOOTOPTIM_INTERPOSER status=ready activation=enabled");
            spawn_java(&parsed.java_exe, &parsed.java_args, &flags)?.code().map_or(1, |c| c)
        }
        CacheState::Absent => {
            let staging = cache_dir.join(format!("staging-{}.jsa", unique_suffix()));
            write_state(&cache_dir, CacheState::Generating, "training")?;
            let flags = vec![
                OsString::from("-Xshare:auto"),
                os_flag("-XX:ArchiveClassesAtExit=", &staging),
            ];
            eprintln!("BOOTOPTIM_INTERPOSER status=generating activation=training");
            let status = spawn_java(&parsed.java_exe, &parsed.java_args, &flags)?;
            if status.success() && staging.is_file() && fs::metadata(&staging).map(|m| m.len()).unwrap_or(0) > 0 {
                match promote_archive(&cache_dir, &staging, &plan.sha256) {
                    Ok(_) => {
                        write_state(&cache_dir, CacheState::Ready, "promotion-complete")?;
                        eprintln!("BOOTOPTIM_INTERPOSER status=ready promotion=complete");
                    }
                    Err(_) => {
                        let _ = fs::remove_file(&staging);
                        write_state(&cache_dir, CacheState::Failed, "promotion-failed")?;
                        eprintln!("BOOTOPTIM_INTERPOSER status=failed reason=promotion-failed");
                    }
                }
            } else {
                let _ = fs::remove_file(&staging);
                write_state(&cache_dir, CacheState::Failed, "training-incomplete")?;
                eprintln!("BOOTOPTIM_INTERPOSER status=failed reason=training-incomplete");
            }
            status.code().map_or(1, |c| c)
        }
        CacheState::Stale => {
            let _ = ready_meta;
            write_state(&cache_dir, CacheState::Stale, "identity-mismatch")?;
            eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=stale");
            spawn_java(&parsed.java_exe, &parsed.java_args, &[])?.code().map_or(1, |c| c)
        }
        CacheState::Failed | CacheState::Generating => {
            cleanup_orphan_staging(&cache_dir)?;
            write_state(&cache_dir, CacheState::Failed, "incomplete-or-failed")?;
            eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=failed-state");
            spawn_java(&parsed.java_exe, &parsed.java_args, &[])?.code().map_or(1, |c| c)
        }
    }
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

fn spawn_java(java_exe: &OsStr, args: &[OsString], prefix: &[OsString]) -> io::Result<ExitStatus> {
    let mut cmd = Command::new(java_exe);
    cmd.args(prefix);
    cmd.args(args);
    cmd.stdin(Stdio::inherit());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());
    cmd.status()
}
