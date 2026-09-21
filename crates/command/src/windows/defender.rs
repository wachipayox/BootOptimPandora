use std::{
    ffi::{OsStr, OsString},
    fs,
    io,
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::Command,
};

use windows::Win32::{
    Foundation::CloseHandle,
    System::{
        SystemInformation::GetSystemDirectoryW,
        Threading::{GetExitCodeProcess, INFINITE, WaitForSingleObject},
    },
    UI::{
        Shell::{SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW},
        WindowsAndMessaging::SW_HIDE,
    },
};

const OWNER_FILE: &str = ".pandora-defender-process-owner-v1";
const OWNER_MAGIC: &[u8; 8] = b"PDPXOWN1";

const QUERY_SCRIPT: &str = r#"try { $p=$env:PANDORA_DEFENDER_TARGET; $x=(Get-MpPreference -ErrorAction Stop).ExclusionProcess; if ($null -ne $x -and $x -contains $p) { exit 0 }; exit 3 } catch { exit 20 }"#;
const ADD_SCRIPT: &str = r#"try { Add-MpPreference -ExclusionProcess $env:PANDORA_DEFENDER_TARGET -ErrorAction Stop; exit 0 } catch { exit 21 }"#;
const REMOVE_SCRIPT: &str = r#"try { Remove-MpPreference -ExclusionProcess $env:PANDORA_DEFENDER_TARGET -ErrorAction Stop; exit 0 } catch { exit 22 }"#;

const EXIT_CHANGED: u32 = 0;
const EXIT_ALREADY_OWNED: u32 = 10;
const EXIT_PRESENT_UNOWNED: u32 = 11;
const EXIT_ABSENT_CLEARED: u32 = 12;
const EXIT_NO_OWNERSHIP: u32 = 13;
const EXIT_INVALID_OWNERSHIP: u32 = 14;
const EXIT_BLOCKED: u32 = 20;
const EXIT_FAILED: u32 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefenderProcessAction {
    Enable,
    Remove,
}

impl DefenderProcessAction {
    pub fn cli_value(self) -> &'static str {
        match self {
            Self::Enable => "enable",
            Self::Remove => "remove",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "enable" => Some(Self::Enable),
            "remove" => Some(Self::Remove),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefenderProcessLocalState {
    NotManaged,
    ManagedCurrent,
    ManagedPrevious,
    InvalidOwnershipRecord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefenderProcessResult {
    Enabled,
    Removed,
    AlreadyEnabledByPandora,
    PresentButNotOwned,
    PreviousEntryAlreadyAbsent,
    NothingOwned,
    InvalidOwnershipRecord,
    UacDenied,
    DefenderBlockedOrUnavailable,
    Failed,
}

pub fn local_state() -> DefenderProcessLocalState {
    let Ok(current) = canonical_launcher_path() else {
        return DefenderProcessLocalState::InvalidOwnershipRecord;
    };
    let marker = owner_file(&current);
    let owner = match read_owner(&marker) {
        Ok(owner) => owner,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return DefenderProcessLocalState::NotManaged;
        }
        Err(_) => return DefenderProcessLocalState::InvalidOwnershipRecord,
    };

    if owner == current {
        DefenderProcessLocalState::ManagedCurrent
    } else if validate_owned_target(&current, &owner) {
        DefenderProcessLocalState::ManagedPrevious
    } else {
        DefenderProcessLocalState::InvalidOwnershipRecord
    }
}

pub fn request(action: DefenderProcessAction) -> DefenderProcessResult {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(_) => return DefenderProcessResult::Failed,
    };

    let exe_wide = wide_null(exe.as_os_str());
    let args = format!("--internal-defender-process-exclusion {}", action.cli_value());
    let args_wide = wide_null(OsStr::new(&args));

    let mut sei = SHELLEXECUTEINFOW::default();
    sei.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    sei.fMask = SEE_MASK_NOASYNC | SEE_MASK_NOCLOSEPROCESS;
    sei.lpVerb = windows::core::w!("runas");
    sei.lpFile = windows::core::PCWSTR(exe_wide.as_ptr());
    sei.lpParameters = windows::core::PCWSTR(args_wide.as_ptr());
    sei.nShow = SW_HIDE.0;

    if let Err(error) = unsafe { ShellExecuteExW(&mut sei) } {
        // HRESULT_FROM_WIN32(ERROR_CANCELLED)
        if error.code().0 == 0x8007_04c7u32 as i32 {
            return DefenderProcessResult::UacDenied;
        }
        return DefenderProcessResult::Failed;
    }

    if sei.hProcess.is_invalid() {
        return DefenderProcessResult::Failed;
    }

    unsafe {
        _ = WaitForSingleObject(sei.hProcess, INFINITE);
    }

    let mut exit_code = EXIT_FAILED;
    let exit_result = unsafe { GetExitCodeProcess(sei.hProcess, &mut exit_code) };
    _ = unsafe { CloseHandle(sei.hProcess) };
    if exit_result.is_err() {
        return DefenderProcessResult::Failed;
    }

    match (action, exit_code) {
        (DefenderProcessAction::Enable, EXIT_CHANGED) => DefenderProcessResult::Enabled,
        (DefenderProcessAction::Remove, EXIT_CHANGED) => DefenderProcessResult::Removed,
        (_, EXIT_ALREADY_OWNED) => DefenderProcessResult::AlreadyEnabledByPandora,
        (_, EXIT_PRESENT_UNOWNED) => DefenderProcessResult::PresentButNotOwned,
        (_, EXIT_ABSENT_CLEARED) => DefenderProcessResult::PreviousEntryAlreadyAbsent,
        (_, EXIT_NO_OWNERSHIP) => DefenderProcessResult::NothingOwned,
        (_, EXIT_INVALID_OWNERSHIP) => DefenderProcessResult::InvalidOwnershipRecord,
        (_, EXIT_BLOCKED) => DefenderProcessResult::DefenderBlockedOrUnavailable,
        _ => DefenderProcessResult::Failed,
    }
}

pub fn run_elevated(action: DefenderProcessAction) -> u32 {
    match action {
        DefenderProcessAction::Enable => elevated_enable(),
        DefenderProcessAction::Remove => elevated_remove(),
    }
}

fn elevated_enable() -> u32 {
    let Ok(target) = canonical_launcher_path() else {
        return EXIT_FAILED;
    };
    let marker = owner_file(&target);

    let existing_owner = match read_owner(&marker) {
        Ok(owner) => Some(owner),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(_) => return EXIT_INVALID_OWNERSHIP,
    };
    if let Some(owner) = existing_owner.as_ref() {
        if owner != &target && validate_owned_target(&target, owner) {
            return EXIT_INVALID_OWNERSHIP;
        }
        if owner != &target {
            return EXIT_INVALID_OWNERSHIP;
        }
    }

    match query_present(&target) {
        Ok(true) if existing_owner.as_ref() == Some(&target) => return EXIT_ALREADY_OWNED,
        Ok(true) => return EXIT_PRESENT_UNOWNED,
        Ok(false) => {
            if existing_owner.as_ref() == Some(&target) {
                let _ = fs::remove_file(&marker);
            }
        }
        Err(_) => return EXIT_BLOCKED,
    }

    if powershell(ADD_SCRIPT, &target).ok() != Some(0) {
        return EXIT_BLOCKED;
    }
    if query_present(&target).ok() != Some(true) {
        return EXIT_FAILED;
    }

    if write_owner(&marker, &target).is_err() {
        let _ = powershell(REMOVE_SCRIPT, &target);
        if query_present(&target).ok() == Some(false) {
            let _ = fs::remove_file(&marker);
        }
        return EXIT_FAILED;
    }

    EXIT_CHANGED
}

fn elevated_remove() -> u32 {
    let Ok(current) = canonical_launcher_path() else {
        return EXIT_FAILED;
    };
    let marker = owner_file(&current);
    let target = match read_owner(&marker) {
        Ok(target) => target,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return EXIT_NO_OWNERSHIP,
        Err(_) => return EXIT_INVALID_OWNERSHIP,
    };

    if removal_plan(&current, Some(&target)) != RemovalPlan::RemoveOwnedTarget {
        return EXIT_INVALID_OWNERSHIP;
    }

    match query_present(&target) {
        Ok(false) => {
            let _ = fs::remove_file(&marker);
            return EXIT_ABSENT_CLEARED;
        }
        Ok(true) => {}
        Err(_) => return EXIT_BLOCKED,
    }

    if powershell(REMOVE_SCRIPT, &target).ok() != Some(0) {
        return EXIT_BLOCKED;
    }
    if query_present(&target).ok() != Some(false) {
        return EXIT_FAILED;
    }
    if fs::remove_file(&marker).is_err() {
        return EXIT_FAILED;
    }

    EXIT_CHANGED
}

fn query_present(target: &Path) -> io::Result<bool> {
    match powershell(QUERY_SCRIPT, target)? {
        0 => Ok(true),
        3 => Ok(false),
        _ => Err(io::Error::other("Defender query failed")),
    }
}

fn powershell(script: &str, target: &Path) -> io::Result<u32> {
    let powershell = system_powershell_path()?;
    let status = Command::new(powershell)
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("PANDORA_DEFENDER_TARGET", target.as_os_str())
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .status()?;

    Ok(status.code().unwrap_or(EXIT_FAILED as i32) as u32)
}

fn system_powershell_path() -> io::Result<PathBuf> {
    let mut buffer = [0u16; 32_768];
    let len = unsafe { GetSystemDirectoryW(Some(&mut buffer)) } as usize;
    if len == 0 || len >= buffer.len() {
        return Err(io::Error::other("unable to resolve the Windows system directory"));
    }

    let mut path = PathBuf::from(OsString::from_wide(&buffer[..len]));
    path.push(r"WindowsPowerShell\v1.0\powershell.exe");
    if !fs::metadata(&path)?.is_file() {
        return Err(io::Error::other("system PowerShell executable is unavailable"));
    }
    Ok(path)
}

fn canonical_launcher_path() -> io::Result<PathBuf> {
    let canonical = fs::canonicalize(std::env::current_exe()?)?;
    let wide: Vec<u16> = canonical.as_os_str().encode_wide().collect();

    let normalized = if wide.starts_with(&[b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16]) {
        if wide.get(4..8).is_some_and(|p| {
            matches!(p[0], 0x55 | 0x75)
                && matches!(p[1], 0x4e | 0x6e)
                && matches!(p[2], 0x43 | 0x63)
                && p[3] == b'\\' as u16
        }) {
            return Err(io::Error::other("network executable paths are not eligible"));
        }
        PathBuf::from(OsString::from_wide(&wide[4..]))
    } else {
        canonical
    };

    if !normalized.is_absolute() || has_wildcard(&normalized) {
        return Err(io::Error::other("launcher path is not an exact absolute path"));
    }
    if !normalized
        .extension()
        .and_then(|v| v.to_str())
        .is_some_and(|v| v.eq_ignore_ascii_case("exe"))
    {
        return Err(io::Error::other("launcher target is not an exe"));
    }
    if !fs::metadata(&normalized)?.is_file() {
        return Err(io::Error::other("launcher target is not a file"));
    }

    Ok(normalized)
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemovalPlan {
    NothingOwned,
    RemoveOwnedTarget,
    RefuseInvalidOwnership,
}

fn removal_plan(current: &Path, owned: Option<&Path>) -> RemovalPlan {
    match owned {
        None => RemovalPlan::NothingOwned,
        Some(owned) if validate_owned_target(current, owned) => RemovalPlan::RemoveOwnedTarget,
        Some(_) => RemovalPlan::RefuseInvalidOwnership,
    }
}

fn validate_owned_target(current: &Path, owned: &Path) -> bool {
    owned.is_absolute()
        && !has_wildcard(owned)
        && owned
            .extension()
            .and_then(|v| v.to_str())
            .is_some_and(|v| v.eq_ignore_ascii_case("exe"))
        && current.parent().is_some()
        && current.parent() == owned.parent()
}

fn has_wildcard(path: &Path) -> bool {
    path.as_os_str()
        .encode_wide()
        .any(|c| c == b'*' as u16 || c == b'?' as u16)
}

fn owner_file(current: &Path) -> PathBuf {
    current
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(OWNER_FILE)
}

fn write_owner(path: &Path, target: &Path) -> io::Result<()> {
    let mut bytes = Vec::with_capacity(OWNER_MAGIC.len() + 256);
    bytes.extend_from_slice(OWNER_MAGIC);
    for unit in target.as_os_str().encode_wide() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }

    let mut temp = path.to_path_buf();
    temp.set_extension("owner-new");
    fs::write(&temp, bytes)?;
    fs::rename(&temp, path)?;
    Ok(())
}

fn read_owner(path: &Path) -> io::Result<PathBuf> {
    let bytes = fs::read(path)?;
    if bytes.len() < OWNER_MAGIC.len()
        || &bytes[..OWNER_MAGIC.len()] != OWNER_MAGIC
        || (bytes.len() - OWNER_MAGIC.len()) % 2 != 0
    {
        return Err(io::Error::other("invalid Defender ownership record"));
    }

    let mut wide = Vec::with_capacity((bytes.len() - OWNER_MAGIC.len()) / 2);
    for pair in bytes[OWNER_MAGIC.len()..].chunks_exact(2) {
        wide.push(u16::from_le_bytes([pair[0], pair[1]]));
    }
    if wide.is_empty() || wide.contains(&0) {
        return Err(io::Error::other("invalid Defender ownership path"));
    }

    Ok(PathBuf::from(OsString::from_wide(&wide)))
}

fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_action_parser_accepts_only_fixed_operations() {
        assert_eq!(DefenderProcessAction::parse("enable"), Some(DefenderProcessAction::Enable));
        assert_eq!(DefenderProcessAction::parse("remove"), Some(DefenderProcessAction::Remove));
        for rejected in ["", "query", "add-path", r"C:\Games", "--remove"] {
            assert_eq!(DefenderProcessAction::parse(rejected), None);
        }
    }

    #[test]
    fn owned_target_must_stay_in_launcher_directory_and_be_exact_exe() {
        let current = Path::new(r"C:\Program Files\Pandora\PandoraLauncher.exe");
        assert!(validate_owned_target(
            current,
            Path::new(r"C:\Program Files\Pandora\PandoraLauncher-old.exe")
        ));
        assert!(!validate_owned_target(current, Path::new(r"C:\Games\Minecraft.exe")));
        assert!(!validate_owned_target(
            current,
            Path::new(r"C:\Program Files\Pandora\*.exe")
        ));
        assert!(!validate_owned_target(
            current,
            Path::new(r"C:\Program Files\Pandora\mods")
        ));
    }

    #[test]
    fn removal_requires_owned_target_in_same_launcher_directory() {
        let current = Path::new(r"C:\Program Files\Pandora\PandoraLauncher.exe");
        assert_eq!(removal_plan(current, None), RemovalPlan::NothingOwned);
        assert_eq!(
            removal_plan(current, Some(Path::new(r"C:\Program Files\Pandora\PandoraLauncher-old.exe"))),
            RemovalPlan::RemoveOwnedTarget
        );
        assert_eq!(
            removal_plan(current, Some(Path::new(r"C:\Games\Minecraft.exe"))),
            RemovalPlan::RefuseInvalidOwnership
        );
    }

    #[test]
    fn defender_commands_are_fixed_process_exclusion_only() {
        assert!(ADD_SCRIPT.contains("Add-MpPreference -ExclusionProcess"));
        assert!(REMOVE_SCRIPT.contains("Remove-MpPreference -ExclusionProcess"));
        assert!(QUERY_SCRIPT.contains(".ExclusionProcess"));
        for script in [ADD_SCRIPT, REMOVE_SCRIPT, QUERY_SCRIPT] {
            assert!(!script.contains("ExclusionPath"));
            assert!(!script.contains("Set-MpPreference"));
            assert!(!script.contains("Disable"));
            assert!(!script.contains("EncodedCommand"));
        }
    }
}
