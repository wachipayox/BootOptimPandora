#![deny(unused_must_use)]

use std::ffi::{OsStr, OsString};

mod backend;
pub use backend::*;

mod account;
mod arcfactory;
mod backend_filesystem;
mod backend_handler;
mod curseforge_manual_download;
mod directories;
mod duplicate;
mod export;
mod fs;
mod id_slab;
mod install_content;
mod instance;
mod java_manifest;
mod launch;
mod launch_wrapper;
mod launcher_import;
mod log_reader;
mod metadata;
mod mod_metadata;
mod persistent;
mod prelaunch_attribution;
#[cfg(test)]
mod prelaunch_mods_probe;
pub mod profile_layout_flow;
mod profile_layout_ownership;
mod profile_layout_service;
mod server_list_pinger;
mod shortcut;
mod skin_manager;
mod syncing;
mod update;

pub const KNOWN_SHADER_MODS: &[&'static str] = &["iris", "oculus", "optifine"];

pub fn join_windows_shell(args: &[&str]) -> String {
    let mut string = String::new();

    let mut first = true;
    for arg in args {
        let mut backslashes = 0;

        if first {
            first = false;
        } else {
            string.push(' ');
        }

        if arg.is_empty() {
            string.push_str("\"\"");
            continue;
        }

        let quoted = arg.contains(&[' ', '\t']);
        if quoted {
            string.push('"');
        }

        for char in arg.chars() {
            if char == '\\' {
                backslashes += 1;
            } else if char == '"' {
                for _ in 0..backslashes {
                    string.push_str("\\\\");
                }
                string.push_str("\\\"");
                backslashes = 0;
            } else {
                for _ in 0..backslashes {
                    string.push('\\');
                }
                backslashes = 0;
                string.push(char);
            }
        }

        if quoted {
            for _ in 0..backslashes {
                string.push_str("\\\\");
            }
            string.push('"');
        } else {
            for _ in 0..backslashes {
                string.push('\\');
            }
        }
    }

    string
}

pub fn join_linux_shell(args: &[&str]) -> String {
    let mut string = String::new();

    let mut first = true;
    for arg in args {
        if first {
            first = false;
        } else {
            string.push(' ');
        }

        if arg.is_empty() {
            string.push_str("''");
            continue;
        }

        let quoted = arg.contains(&[' ', '\t']);
        if quoted {
            string.push('\'');
        }

        for char in arg.chars() {
            if char == '\'' {
                string.push_str("'\\''");
            } else {
                string.push(char);
            }
        }

        if quoted {
            string.push('\'');
        }
    }

    string
}

pub fn split_shell(string: &str, mut handle: impl FnMut(&str)) {
    let mut word_start = 0;
    let mut in_quotes = false;
    let mut escape = false;

    for (index, char) in string.char_indices() {
        if escape {
            escape = false;
            continue;
        }

        if char == '\\' {
            escape = true;
            continue;
        }

        if char == '"' {
            in_quotes = !in_quotes;
            continue;
        }

        if char == ' ' && !in_quotes {
            if word_start < index {
                handle(&string[word_start..index]);
            }
            word_start = index + 1;
        }
    }

    if word_start < string.len() {
        handle(&string[word_start..]);
    }
}

pub fn osstr_starts_with(whole: &OsStr, prefix: &OsStr) -> bool {
    whole.as_encoded_bytes().starts_with(prefix.as_encoded_bytes())
}

pub fn osstr_split_once<'a>(whole: &'a OsStr, delimiter: &OsStr) -> Option<(&'a OsStr, &'a OsStr)> {
    let delimiter = delimiter.as_encoded_bytes();
    let whole = whole.as_encoded_bytes();

    let index = whole.windows(delimiter.len()).position(|window| window == delimiter)?;
    let (left, right) = whole.split_at(index);
    let right = &right[delimiter.len()..];

    unsafe { Some((OsStr::from_encoded_bytes_unchecked(left), OsStr::from_encoded_bytes_unchecked(right))) }
}

pub fn osstring_join<I>(strings: I, join: &OsStr) -> OsString
where
    I: IntoIterator,
    I::Item: AsRef<OsStr>,
{
    let mut output = OsString::new();
    let mut first = true;
    for string in strings {
        if first {
            first = false;
        } else {
            output.push(join);
        }
        output.push(string);
    }
    output
}
