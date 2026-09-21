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
pub use crate::windows::defender::{DefenderProcessAction, DefenderProcessLocalState, DefenderProcessResult, local_state as defender_process_local_state, request as request_defender_process_action, run_elevated as run_defender_process_action_elevated};

#[cfg(windows)]
pub fn set_traverse_acls(args: Vec<std::ffi::OsString>) -> std::io::Result<()> {
    crate::windows::appcontainer::set_traverse_acls(args)
}
