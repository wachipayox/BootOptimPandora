use bootoptim_launch_interposer::{
    CacheLock, CacheState, Mode, build_plan, cache_dir, cds_compatible, classify, persist_plan,
    plan_sha256, promote, ready_archive, staging_archive, write_state,
};
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

struct Invocation {
    mode: Mode,
    java: OsString,
    java_args: Vec<OsString>,
}

fn parse_invocation() -> Option<Invocation> {
    let mut args = std::env::args_os().skip(1).peekable();
    let mut mode = Mode::Plan;
    if args.peek().is_some_and(|arg| arg == "--mode") {
        args.next();
        mode = match args.next()?.to_string_lossy().as_ref() {
            "plan" => Mode::Plan,
            "train" => Mode::Train,
            "auto" => Mode::Auto,
            _ => return None,
        };
    }
    let java = args.next()?;
    Some(Invocation {
        mode,
        java,
        java_args: args.collect(),
    })
}

fn xx_path_flag(prefix: &str, path: &Path) -> OsString {
    let mut out = OsString::from(prefix);
    out.push(path.as_os_str());
    out
}

fn spawn_java(java: &OsStr, original_args: &[OsString], prefix_args: &[OsString]) -> io::Result<i32> {
    let status = Command::new(java)
        .args(prefix_args)
        .args(original_args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;
    Ok(status.code().unwrap_or(1))
}

fn stock(inv: &Invocation) -> i32 {
    spawn_java(&inv.java, &inv.java_args, &[]).unwrap_or(1)
}

fn run(inv: &Invocation) -> io::Result<i32> {
    let game_dir = std::env::current_dir()?;
    let cache = cache_dir(&game_dir);

    // A contending launch must not wait for or mutate an archive owned by another process.
    let Some(_lock) = CacheLock::try_acquire(&cache)? else {
        return Ok(stock(inv));
    };

    let helper = std::env::current_exe()?;
    let separator = if cfg!(windows) { ';' } else { ':' };
    let java_path = PathBuf::from(&inv.java);
    let plan = match build_plan(&java_path, &inv.java_args, &game_dir, &helper, separator) {
        Ok(plan) => plan,
        Err(_) => return Ok(stock(inv)),
    };
    let plan_hash = plan_sha256(&plan)?;
    persist_plan(&cache, &plan)?;

    // Existing user/launcher CDS or agent configuration is never overridden.
    if !cds_compatible(&inv.java_args) {
        let _ = write_state(&cache, CacheState::Stale, &plan_hash);
        return Ok(stock(inv));
    }

    match inv.mode {
        Mode::Plan => {
            let state = classify(&cache, &plan_hash).unwrap_or(CacheState::Failed);
            let _ = write_state(&cache, state, &plan_hash);
            Ok(stock(inv))
        }
        Mode::Auto => {
            let state = classify(&cache, &plan_hash).unwrap_or(CacheState::Failed);
            let _ = write_state(&cache, state, &plan_hash);
            if state != CacheState::Ready {
                return Ok(stock(inv));
            }
            let flags = vec![
                OsString::from("-Xshare:auto"),
                xx_path_flag("-XX:SharedArchiveFile=", &ready_archive(&cache)),
            ];
            spawn_java(&inv.java, &inv.java_args, &flags)
        }
        Mode::Train => {
            let staging = staging_archive(&cache);
            let _ = write_state(&cache, CacheState::Generating, &plan_hash);
            let flags = vec![
                OsString::from("-Xshare:auto"),
                xx_path_flag("-XX:ArchiveClassesAtExit=", &staging),
            ];
            let exit = spawn_java(&inv.java, &inv.java_args, &flags)?;
            if exit == 0 {
                match promote(&cache, &staging, &plan_hash) {
                    Ok(()) => {
                        let _ = write_state(&cache, CacheState::Ready, &plan_hash);
                    }
                    Err(_) => {
                        let _ = std::fs::remove_file(&staging);
                        let _ = write_state(&cache, CacheState::Failed, &plan_hash);
                    }
                }
            } else {
                let _ = std::fs::remove_file(&staging);
                let _ = write_state(&cache, CacheState::Failed, &plan_hash);
            }
            Ok(exit)
        }
    }
}

fn main() {
    let Some(inv) = parse_invocation() else {
        std::process::exit(2);
    };
    // Once the opaque Java command is known, every helper failure is fail-open.
    let exit = run(&inv).unwrap_or_else(|_| stock(&inv));
    std::process::exit(exit);
}
