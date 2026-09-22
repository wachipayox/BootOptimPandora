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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TrainingReconcile {
    Clean,
    Matching,
    Discarded(&'static str),
}

#[derive(Debug)]
struct ParsedArgs {
    instance_dir: PathBuf,
    launcher_exe: Option<PathBuf>,
    upstream_commit: String,
    appcds_identity_normal_gui: bool,
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
        },
    };

    match prepare_launch(&parsed) {
        Ok(decision) => println!("{}", decision.as_str()),
        Err(_) => {
            eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=helper-error");
            println!("STOCK");
        },
    }
}

fn prepare_launch(parsed: &ParsedArgs) -> io::Result<PrepareDecision> {
    let mode = match env::var("BOOTOPTIM_APPCDS_MODE").ok().as_deref() {
        Some("auto") => Mode::Auto,
        _ => Mode::Plan,
    };

    let identity_request = IdentityDigestCache::request_state(parsed.appcds_identity_normal_gui);
    let cache_dir = parsed.instance_dir.join(".bootoptim").join("appcds");
    let mut identity_diag = IdentityPreflightDiagnostics::new(identity_request);

    let profile_scope = match acquire_appcds_profile_scope(&parsed.instance_dir) {
        Ok(scope) => scope,
        Err(_) => {
            eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=profile-namespace-ineligible");
            return Ok(PrepareDecision::Stock);
        },
    };
    if bind_appcds_cache_namespace(&cache_dir, profile_scope.namespace()).is_err() {
        eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=profile-cache-binding");
        return Ok(PrepareDecision::Stock);
    }
    identity_diag.classify_manifest(&cache_dir);

    let identity_requested = identity_request.authorized;
    let mut held_lock = None;
    let plan = if identity_requested {
        let Some(lock) = try_lock(&cache_dir.join("cache.lock"))? else {
            let mut stock_cache = IdentityDigestCache::stock();
            let _ = build_launch_plan_for_namespace_with_cache(parsed, profile_scope.namespace(), &mut stock_cache)?;
            identity_diag.capture_cache(&stock_cache, "lock-busy");
            eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=identity-lock-busy");
            return Ok(PrepareDecision::Stock);
        };
        held_lock = Some(lock);
        let mut identity_cache = IdentityDigestCache::begin(&cache_dir, true);
        let plan = build_launch_plan_for_namespace_with_cache(parsed, profile_scope.namespace(), &mut identity_cache)?;
        let publication = match identity_cache.finish() {
            Ok(outcome) => outcome,
            Err(_) => {
                eprintln!("BOOTOPTIM_INTERPOSER identity_cache=publish-failed fallback=future-stock");
                "failed"
            },
        };
        identity_diag.capture_cache(&identity_cache, publication);
        plan
    } else {
        let mut stock_cache = IdentityDigestCache::stock();
        let plan = build_launch_plan_for_namespace_with_cache(parsed, profile_scope.namespace(), &mut stock_cache)?;
        identity_diag.capture_cache(&stock_cache, "not-authorized");
        plan
    };

    if held_lock.is_none() {
        held_lock = try_lock(&cache_dir.join("cache.lock"))?;
    }
    let Some(_lock) = held_lock else {
        eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=lock-busy");
        return Ok(PrepareDecision::Stock);
    };
    let stable = persist_plan_and_compare(&cache_dir, &plan.bytes, &plan.sha256)?;

    let training_reconcile = reconcile_training(&cache_dir, &plan.sha256)?;

    if mode == Mode::Plan {
        eprintln!(
            "BOOTOPTIM_INTERPOSER status=plan-only deterministic={}",
            if stable { "true" } else { "false" }
        );
        return Ok(PrepareDecision::Stock);
    }

    if !cfg!(windows) {
        eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=appcds-windows-only-v0");
        return Ok(PrepareDecision::Stock);
    }

    if let TrainingReconcile::Discarded(reason) = training_reconcile {
        let state = if reason == "training-plan-mismatch" {
            CacheState::Stale
        } else {
            CacheState::Failed
        };
        write_state(&cache_dir, state, reason)?;
        eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason={reason}");
        return Ok(PrepareDecision::Stock);
    }

    if !plan.eligible {
        if training_state_present(&cache_dir) {
            invalidate_training(&cache_dir)?;
        }
        write_state(&cache_dir, CacheState::Failed, "identity-ineligible")?;
        eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=identity-ineligible");
        return Ok(PrepareDecision::Stock);
    }

    if matches!(training_reconcile, TrainingReconcile::Clean) {
        match try_adopt_exact_clone_candidate(&parsed.instance_dir, &cache_dir, profile_scope.namespace(), &plan)? {
            ExactCloneAdoption::None => {},
            ExactCloneAdoption::Adopted => {
                write_state(&cache_dir, CacheState::Ready, "exact-clone-plan-confirmed")?;
                eprintln!("BOOTOPTIM_INTERPOSER status=ready activation=exact-clone");
                return Ok(PrepareDecision::Ready);
            },
            ExactCloneAdoption::Rejected => {
                write_state(&cache_dir, CacheState::Stale, "exact-clone-candidate-rejected")?;
                eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=exact-clone-candidate-rejected");
                return Ok(PrepareDecision::Stock);
            },
        }
    }

    prepare_eligible_cache(&cache_dir, &plan.sha256)
}

fn prepare_eligible_cache(cache_dir: &Path, plan_sha256: &str) -> io::Result<PrepareDecision> {
    let (state, _) = classify_cache(cache_dir, plan_sha256)?;
    match state {
        CacheState::Ready => {
            if training_state_present(cache_dir) {
                invalidate_training(cache_dir)?;
                write_state(cache_dir, CacheState::Failed, "ambiguous-ready-and-training")?;
                eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=ambiguous-ready-and-training");
                return Ok(PrepareDecision::Stock);
            }
            write_state(cache_dir, CacheState::Ready, "identity-match")?;
            eprintln!("BOOTOPTIM_INTERPOSER status=ready activation=enabled");
            return Ok(PrepareDecision::Ready);
        },
        CacheState::Stale => {
            if training_state_present(cache_dir) {
                invalidate_training(cache_dir)?;
            }
            write_state(cache_dir, CacheState::Stale, "identity-mismatch")?;
            eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=stale");
            return Ok(PrepareDecision::Stock);
        },
        CacheState::Failed | CacheState::Generating => {
            cleanup_orphan_staging(cache_dir)?;
            if training_state_present(cache_dir) {
                invalidate_training(cache_dir)?;
            }
            write_state(cache_dir, CacheState::Failed, "incomplete-or-failed")?;
            eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=failed-state");
            return Ok(PrepareDecision::Stock);
        },
        CacheState::Absent => {},
    }

    let training_meta = cache_dir.join("training.meta");
    let training_archive = cache_dir.join("training.jsa");
    let training_complete = cache_dir.join("training.complete");

    if training_meta.exists() {
        if !training_meta.is_file() {
            invalidate_training(cache_dir)?;
            write_state(cache_dir, CacheState::Failed, "training-metadata-corrupt")?;
            eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=training-metadata-corrupt");
            return Ok(PrepareDecision::Stock);
        }

        let pending_plan = match read_training_plan(&training_meta) {
            Ok(plan) => plan,
            Err(_) => {
                invalidate_training(cache_dir)?;
                write_state(cache_dir, CacheState::Failed, "training-metadata-corrupt")?;
                eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=training-metadata-corrupt");
                return Ok(PrepareDecision::Stock);
            },
        };
        if pending_plan != plan_sha256 {
            invalidate_training(cache_dir)?;
            write_state(cache_dir, CacheState::Stale, "training-plan-mismatch")?;
            eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=training-plan-mismatch");
            return Ok(PrepareDecision::Stock);
        }

        if !training_complete.exists() {
            write_state(cache_dir, CacheState::Generating, "training-pending")?;
            eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=training-pending");
            return Ok(PrepareDecision::Stock);
        }

        if !training_complete.is_file() || fs::read(&training_complete)? != b"complete\n" {
            invalidate_training(cache_dir)?;
            write_state(cache_dir, CacheState::Failed, "training-completion-corrupt")?;
            eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=training-completion-corrupt");
            return Ok(PrepareDecision::Stock);
        }

        if !training_archive.is_file() {
            invalidate_training(cache_dir)?;
            write_state(cache_dir, CacheState::Failed, "training-complete-without-archive")?;
            eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=training-complete-without-archive");
            return Ok(PrepareDecision::Stock);
        }

        if fs::metadata(&training_archive)?.len() == 0 {
            invalidate_training(cache_dir)?;
            write_state(cache_dir, CacheState::Failed, "training-empty")?;
            eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=training-empty");
            return Ok(PrepareDecision::Stock);
        }

        promote_archive(cache_dir, &training_archive, plan_sha256)?;
        cleanup_training_files(cache_dir);
        write_state(cache_dir, CacheState::Ready, "promotion-complete")?;
        eprintln!("BOOTOPTIM_INTERPOSER status=ready promotion=complete");
        return Ok(PrepareDecision::Ready);
    }

    if training_archive.exists() || training_complete.exists() || cache_dir.join("training.invalid").exists() {
        invalidate_training(cache_dir)?;
        write_state(cache_dir, CacheState::Failed, "orphan-training-state")?;
        eprintln!("BOOTOPTIM_INTERPOSER status=fail-open reason=orphan-training-state");
        return Ok(PrepareDecision::Stock);
    }

    // The first eligible identity trains immediately. training.meta is the
    // persisted training identity; no archive can be consumed in this launch
    // because HotSpot has not produced training.jsa or a clean-exit marker yet.
    write_atomic_replace(
        &training_meta,
        format!("schema={}\nplan_sha256={}\n", SCHEMA_VERSION, plan_sha256).as_bytes(),
    )?;
    write_state(cache_dir, CacheState::Generating, "training")?;
    eprintln!("BOOTOPTIM_INTERPOSER status=generating activation=training");
    Ok(PrepareDecision::Train)
}

fn reconcile_training(cache_dir: &Path, plan_sha256: &str) -> io::Result<TrainingReconcile> {
    let training_meta = cache_dir.join("training.meta");
    let training_archive = cache_dir.join("training.jsa");
    let training_complete = cache_dir.join("training.complete");
    let invalid = cache_dir.join("training.invalid");

    if invalid.exists() {
        invalidate_training(cache_dir)?;
        return Ok(TrainingReconcile::Discarded("training-invalidated"));
    }

    if !training_meta.exists() {
        if training_archive.exists() || training_complete.exists() {
            invalidate_training(cache_dir)?;
            return Ok(TrainingReconcile::Discarded("orphan-training-state"));
        }
        return Ok(TrainingReconcile::Clean);
    }

    if !training_meta.is_file() {
        invalidate_training(cache_dir)?;
        return Ok(TrainingReconcile::Discarded("training-metadata-corrupt"));
    }

    let pending_plan = match read_training_plan(&training_meta) {
        Ok(plan) => plan,
        Err(_) => {
            invalidate_training(cache_dir)?;
            return Ok(TrainingReconcile::Discarded("training-metadata-corrupt"));
        },
    };
    if pending_plan != plan_sha256 {
        invalidate_training(cache_dir)?;
        return Ok(TrainingReconcile::Discarded("training-plan-mismatch"));
    }

    if training_complete.exists() {
        if !training_complete.is_file() || fs::read(&training_complete)? != b"complete\n" {
            invalidate_training(cache_dir)?;
            return Ok(TrainingReconcile::Discarded("training-completion-corrupt"));
        }
        if !training_archive.is_file() || fs::metadata(&training_archive)?.len() == 0 {
            invalidate_training(cache_dir)?;
            return Ok(TrainingReconcile::Discarded("training-complete-without-archive"));
        }
    }

    Ok(TrainingReconcile::Matching)
}

fn read_training_plan(path: &Path) -> io::Result<String> {
    let text = fs::read_to_string(path)?;
    let mut lines = text.lines();
    let expected_schema = format!("schema={}", SCHEMA_VERSION);
    if lines.next() != Some(expected_schema.as_str()) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "training schema"));
    }
    let plan_line = lines.next().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "training plan"))?;
    if lines.next().is_some() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "training metadata trailing data"));
    }
    let plan = plan_line
        .strip_prefix("plan_sha256=")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "training plan"))?;
    if plan.len() != 64 || !plan.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "training plan digest"));
    }
    Ok(plan.to_string())
}

fn training_state_present(cache_dir: &Path) -> bool {
    ["training.meta", "training.jsa", "training.complete", "training.invalid"]
        .into_iter()
        .any(|name| cache_dir.join(name).exists())
}

fn invalidate_training(cache_dir: &Path) -> io::Result<()> {
    let invalid = cache_dir.join("training.invalid");

    // Persist the tombstone before touching campaign files. A mismatching
    // preflight can race the Java process that is still writing training.jsa
    // and will later write training.complete through Pandora. The tombstone is
    // therefore intentionally not auto-cleared: even if those late writes
    // recreate canonical paths, no later preflight can consume them or start a
    // second writer in the same namespace. A deliberate cache reset starts the
    // next campaign.
    if !invalid.exists() {
        write_atomic_replace(&invalid, b"invalid\n")?;
    }

    // Once the tombstone exists, cleanup is best-effort. Windows may refuse to
    // remove training.jsa while HotSpot still has it open; that is safe because
    // training.invalid remains the authoritative no-consume/no-retrain latch.
    for name in ["training.meta", "training.jsa", "training.complete"] {
        match fs::remove_file(cache_dir.join(name)) {
            Ok(()) => {},
            Err(error) if error.kind() == io::ErrorKind::NotFound => {},
            Err(_) => {},
        }
    }
    Ok(())
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
    let mut appcds_identity_normal_gui = false;
    let mut identity_authority_seen = false;
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
        } else if args[i] == OsStr::new("--appcds-identity-authority") {
            if identity_authority_seen {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "duplicate identity authority"));
            }
            identity_authority_seen = true;
            i += 1;
            appcds_identity_normal_gui = match args.get(i).and_then(|s| s.to_str()) {
                Some("normal-gui") => true,
                Some("unknown") => false,
                _ => return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid identity authority")),
            };
        } else {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "unknown control argument"));
        }
        i += 1;
    }
    let java_exe = args
        .get(i)
        .cloned()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing java"))?;
    let java_args = args.get(i + 1..).unwrap_or_default().to_vec();
    let instance_dir = instance_dir.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing instance"))?;
    Ok(ParsedArgs {
        instance_dir,
        launcher_exe,
        upstream_commit,
        appcds_identity_normal_gui,
        java_exe,
        java_args,
    })
}
