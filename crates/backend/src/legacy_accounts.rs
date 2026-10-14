//! One-time import of Minecraft accounts from the data folder used before the Wachiland rebrand.
//!
//! The rebrand moved the launcher folder from `PandoraLauncher` to `WachilandLauncher` (a sibling
//! in the same data directory). Only `accounts.json` is imported: instances, libraries and every
//! other file stay where they are. Account secrets are kept by the system credential store under
//! names that never included the folder (`PandoraLauncher_*` on Windows, service
//! `pandora-launcher` on Linux), keyed by account UUID, so an imported account keeps working
//! without signing in again. The old folder is only read, never modified.

use std::path::{Path, PathBuf};

use crate::account::BackendAccountInfo;

const LEGACY_FOLDER: &str = "PandoraLauncher";
const ACCOUNTS_FILE: &str = "accounts.json";
/// Written after a successful import so accounts the player later removes are not re-imported.
const MARKER_FILE: &str = "pandora-accounts-imported";

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ImportOutcome {
    AlreadyImported,
    NoLegacyAccounts,
    Imported {
        added: usize,
        already_present: usize,
    },
}

/// Imports legacy accounts before the account list is loaded. Failures are logged and leave
/// both folders untouched so the next start can retry.
pub(crate) fn import_legacy_accounts(launcher_dir: &Path) {
    match try_import(launcher_dir) {
        Ok(ImportOutcome::Imported { added, already_present }) => log::info!(
            "[migration] Imported {added} account(s) from the previous {LEGACY_FOLDER} folder ({already_present} already present). \
             Credentials stay in the system store under the same names; the old folder was not modified."
        ),
        Ok(ImportOutcome::AlreadyImported | ImportOutcome::NoLegacyAccounts) => {},
        Err(error) => log::warn!(
            "[migration] Could not import accounts from the previous {LEGACY_FOLDER} folder; nothing was changed and it will be retried on the next start: {error}"
        ),
    }
}

fn legacy_accounts_path(launcher_dir: &Path) -> Option<PathBuf> {
    if launcher_dir.file_name()? == LEGACY_FOLDER {
        return None;
    }
    Some(launcher_dir.parent()?.join(LEGACY_FOLDER).join(ACCOUNTS_FILE))
}

pub(crate) fn try_import(launcher_dir: &Path) -> Result<ImportOutcome, String> {
    let marker = launcher_dir.join(MARKER_FILE);
    if marker.exists() {
        return Ok(ImportOutcome::AlreadyImported);
    }
    let Some(legacy_path) = legacy_accounts_path(launcher_dir).filter(|path| path.is_file()) else {
        return Ok(ImportOutcome::NoLegacyAccounts);
    };
    let legacy: BackendAccountInfo = crate::fs::read_json(&legacy_path)
        .map_err(|error| format!("the old accounts.json is unreadable ({error:?})"))?;

    let current_path = launcher_dir.join(ACCOUNTS_FILE);
    let mut current: BackendAccountInfo = if current_path.exists() {
        // Never overwrite an accounts file this launcher cannot read.
        crate::fs::read_json(&current_path)
            .map_err(|error| format!("the current accounts.json is unreadable ({error:?})"))?
    } else {
        BackendAccountInfo::default()
    };

    let total = legacy.accounts.len();
    let mut added = 0;
    for (uuid, account) in legacy.accounts {
        if !current.accounts.contains_key(&uuid) {
            current.accounts.insert(uuid, account);
            added += 1;
        }
    }
    let mut changed = added > 0;
    if current.selected_account.is_none()
        && let Some(selected) = legacy.selected_account.filter(|uuid| current.accounts.contains_key(uuid))
    {
        current.selected_account = Some(selected);
        changed = true;
    }

    if changed {
        let bytes = serde_json::to_vec(&current).map_err(|error| error.to_string())?;
        crate::fs::write_safe(&current_path, &bytes)
            .map_err(|error| format!("could not write accounts.json ({error})"))?;
    }
    crate::fs::write_safe(
        &marker,
        format!("Imported {added} of {total} account(s) from ../{LEGACY_FOLDER}/{ACCOUNTS_FILE}\n").as_bytes(),
    )
    .map_err(|error| format!("could not write the import marker ({error})"))?;
    Ok(ImportOutcome::Imported {
        added,
        already_present: total - added,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn data_dir(label: &str) -> PathBuf {
        let root = std::env::temp_dir()
            .join(format!("wachiland-legacy-accounts-{label}-{}", Uuid::from_bytes(rand::random())));
        std::fs::create_dir_all(root.join(LEGACY_FOLDER)).unwrap();
        std::fs::create_dir_all(root.join("WachilandLauncher")).unwrap();
        root
    }

    fn accounts(entries: &[(Uuid, &str)], selected: Option<Uuid>) -> String {
        let accounts = entries
            .iter()
            .map(|(uuid, name)| format!(r#""{uuid}":{{"username":"{name}","offline":false,"head":null}}"#))
            .collect::<Vec<_>>()
            .join(",");
        let selected = selected.map_or_else(|| "null".to_owned(), |uuid| format!("\"{uuid}\""));
        format!(r#"{{"accounts":{{{accounts}}},"selected_account":{selected}}}"#)
    }

    fn read(path: &Path) -> BackendAccountInfo {
        crate::fs::read_json(path).unwrap()
    }

    #[test]
    fn imports_into_an_empty_launcher_and_leaves_the_old_folder_untouched() {
        let root = data_dir("fresh");
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let legacy = accounts(&[(a, "Alpha"), (b, "Beta")], Some(b));
        std::fs::write(root.join(LEGACY_FOLDER).join(ACCOUNTS_FILE), &legacy).unwrap();
        let launcher = root.join("WachilandLauncher");

        assert_eq!(
            try_import(&launcher).unwrap(),
            ImportOutcome::Imported {
                added: 2,
                already_present: 0
            }
        );
        let imported = read(&launcher.join(ACCOUNTS_FILE));
        assert_eq!(imported.accounts.keys().copied().collect::<Vec<_>>(), vec![a, b]);
        assert_eq!(imported.selected_account, Some(b));
        assert_eq!(std::fs::read_to_string(root.join(LEGACY_FOLDER).join(ACCOUNTS_FILE)).unwrap(), legacy);

        // A later removal in the new launcher is not undone on the next start.
        std::fs::write(launcher.join(ACCOUNTS_FILE), accounts(&[(a, "Alpha")], Some(a))).unwrap();
        assert_eq!(try_import(&launcher).unwrap(), ImportOutcome::AlreadyImported);
        assert_eq!(read(&launcher.join(ACCOUNTS_FILE)).accounts.len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn merges_without_replacing_existing_accounts_or_selection() {
        let root = data_dir("merge");
        let (a, b, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        std::fs::write(
            root.join(LEGACY_FOLDER).join(ACCOUNTS_FILE),
            accounts(&[(a, "OldName"), (b, "Beta")], Some(a)),
        )
        .unwrap();
        let launcher = root.join("WachilandLauncher");
        std::fs::write(launcher.join(ACCOUNTS_FILE), accounts(&[(a, "NewName"), (c, "Gamma")], Some(c))).unwrap();

        assert_eq!(
            try_import(&launcher).unwrap(),
            ImportOutcome::Imported {
                added: 1,
                already_present: 1
            }
        );
        let merged = read(&launcher.join(ACCOUNTS_FILE));
        assert_eq!(merged.accounts.len(), 3);
        assert_eq!(&*merged.accounts[&a].username, "NewName");
        assert_eq!(merged.selected_account, Some(c));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unreadable_files_change_nothing_and_retry_later() {
        let root = data_dir("corrupt");
        let launcher = root.join("WachilandLauncher");
        std::fs::write(root.join(LEGACY_FOLDER).join(ACCOUNTS_FILE), accounts(&[(Uuid::new_v4(), "Alpha")], None))
            .unwrap();
        std::fs::write(launcher.join(ACCOUNTS_FILE), b"{not json").unwrap();
        assert!(try_import(&launcher).is_err());
        assert_eq!(std::fs::read(launcher.join(ACCOUNTS_FILE)).unwrap(), b"{not json");
        assert!(!launcher.join(MARKER_FILE).exists());

        std::fs::remove_file(launcher.join(ACCOUNTS_FILE)).unwrap();
        std::fs::write(root.join(LEGACY_FOLDER).join(ACCOUNTS_FILE), b"[]").unwrap();
        assert!(try_import(&launcher).is_err());
        assert!(!launcher.join(ACCOUNTS_FILE).exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn nothing_to_do_without_a_legacy_folder() {
        let root =
            std::env::temp_dir().join(format!("wachiland-legacy-accounts-none-{}", Uuid::from_bytes(rand::random())));
        let launcher = root.join("WachilandLauncher");
        std::fs::create_dir_all(&launcher).unwrap();
        assert_eq!(try_import(&launcher).unwrap(), ImportOutcome::NoLegacyAccounts);
        assert!(!launcher.join(MARKER_FILE).exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
