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
    let fake_java = args[2].clone();
    let _ = std::fs::remove_file(&trace_path);
    unsafe {
        std::env::set_var("BOOTOPTIM_LAUNCH_PROBE", &trace_path);
    }

    let action = bridge::modal_action::ModalAction::normal_launch();
    bridge::launch_probe::request(action.probe_key());
    bridge::launch_probe::backend_dispatch(action.probe_key());
    bridge::launch_probe::instance_config_loaded();
    action.clear_trackers();
    action.clear_trackers();

    let assets = action.push_tracker("Verifying integrity of game assets".into());
    let libraries = action.push_tracker("Verifying integrity of game libraries".into());
    assets.set_finished(bridge::modal_action::ProgressTrackerFinishType::Normal);
    libraries.set_finished(bridge::modal_action::ProgressTrackerFinishType::Normal);

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let mut command = PandoraCommand::new(fake_java);
        command.arg("com.moulberry.pandora.LaunchWrapper");
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