#![deny(unused_must_use)]

use std::{ffi::OsStr, path::Path, sync::Arc};

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

mod command;
mod exit_status;
mod path_cache;
mod process;
mod spawner;

pub use command::*;
pub use process::*;
pub use exit_status::*;

pub fn is_command_available(command: &'static str) -> bool {
    path_cache::get_command_path_cached(OsStr::new(command)).is_some()
}

pub fn get_command_path(command: &'static str) -> Option<Arc<Path>> {
    path_cache::get_command_path(OsStr::new(command))
}

#[cfg(windows)]
fn bootoptim_probe_contract_selftest(args: &[std::ffi::OsString]) -> std::io::Result<()> {
    use std::{io::{Error, ErrorKind}, path::PathBuf};

    if args.len() != 3 {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "probe contract selftest expects trace path and fake java path",
        ));
    }

    let trace_path = PathBuf::from(&args[1]);
    let configured_trace = std::env::var_os("BOOTOPTIM_LAUNCH_PROBE")
        .map(PathBuf::from)
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "BOOTOPTIM_LAUNCH_PROBE is missing"))?;
    if configured_trace != trace_path {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "probe contract selftest trace path does not match BOOTOPTIM_LAUNCH_PROBE",
        ));
    }

    let trace = std::fs::read_to_string(&trace_path)?;
    let first = trace.lines().find(|line| !line.trim().is_empty())
        .ok_or_else(|| Error::new(ErrorKind::InvalidData, "probe trace is empty"))?;
    if !first.contains("\"phase\":\"launcher_pre_java\"")
        || !first.contains("\"event\":\"begin\"")
    {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "probe trace is not armed with launcher_pre_java.begin",
        ));
    }

    let fake_java = args[2].clone();
    let instance_dir = trace_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "trace path needs a parent directory"))?
        .to_path_buf();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let mut command = PandoraCommand::new(fake_java);
        command.arg("com.moulberry.pandora.LaunchWrapper");
        command.current_dir(&instance_dir);
        command.stdin(PandoraStdioWriteMode::Null);
        command.stdout(PandoraStdioReadMode::Null);
        command.stderr(PandoraStdioReadMode::Null);
        let _child = command.spawn().await?;
        Ok::<(), std::io::Error>(())
    })
}

#[cfg(windows)]
pub fn set_traverse_acls(args: Vec<std::ffi::OsString>) -> std::io::Result<()> {
    const SELFTEST_SENTINEL: &str = "__bootoptim_probe_contract_selftest__";
    if args.first().and_then(|value| value.to_str()) == Some(SELFTEST_SENTINEL) {
        return bootoptim_probe_contract_selftest(&args);
    }
    crate::windows::appcontainer::set_traverse_acls(args)
}
