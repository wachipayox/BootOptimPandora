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

#[doc(hidden)]
pub fn bootoptim_packaged_probe_contract_finalize_for_test() {
    let mut command = PandoraCommand::new("java.exe");
    command.arg("com.moulberry.pandora.LaunchWrapper");
    spawner::probe_minecraft_command_ready(&command);
    spawner::probe_event("java_spawn", "begin", None);
    spawner::probe_event("java_spawn", "end", Some("ok"));
    spawner::probe_event("launcher_pre_java", "end", Some("ok"));
}

pub fn is_command_available(command: &'static str) -> bool {
    path_cache::get_command_path_cached(OsStr::new(command)).is_some()
}

pub fn get_command_path(command: &'static str) -> Option<Arc<Path>> {
    path_cache::get_command_path(OsStr::new(command))
}

#[cfg(windows)]
pub fn set_traverse_acls(args: Vec<std::ffi::OsString>) -> std::io::Result<()> {
    if std::env::var_os("BOOTOPTIM_PACKAGED_PROBE_SELFTEST_COMMAND").as_deref()
        == Some(std::ffi::OsStr::new("1"))
    {
        bootoptim_packaged_probe_contract_finalize_for_test();
        return Ok(());
    }
    crate::windows::appcontainer::set_traverse_acls(args)
}
