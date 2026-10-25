//! Local replacement after Distribution's signed artifact checks have succeeded.
//! The previous installation is retained beside the target for recovery.
use anyhow::{Context, Result, ensure};
#[cfg(any(target_os = "macos", test))]
use std::{
    ffi::{OsStr, OsString},
    io::{Cursor, Read},
    path::Component,
};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

#[cfg(any(target_os = "macos", test))]
const MAX_EXPANDED: u64 = 2 * 1024 * 1024 * 1024;
#[cfg(any(target_os = "macos", test))]
const MAX_EXECUTABLE: u64 = 512 * 1024 * 1024;

pub(crate) fn install(bytes: &[u8]) -> Result<()> {
    let executable = std::env::current_exe()?;
    ensure!(executable.is_absolute(), "current executable path is not absolute");
    #[cfg(target_os = "macos")]
    if let Some(bundle) = app_path(&executable) {
        let parent = bundle.parent().context("application has no parent")?;
        let unpack = parent.join(format!(".wbombo-update-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&unpack)?;
        let result = (|| {
            let extracted = unpack_bundle(bytes, &unpack)?;
            let _ = bundle_executable(&extracted)?;
            let staged = unpack.with_added_extension("app");
            fs::rename(extracted, &staged)?;
            let result = replace_unix(bundle, &staged, true);
            if staged.exists() {
                let _ = fs::remove_dir_all(&staged);
            }
            result
        })();
        let _ = fs::remove_dir_all(&unpack);
        return result;
    }
    #[cfg(target_os = "macos")]
    let standalone = extract_macos_executable(bytes)?;
    #[cfg(target_os = "macos")]
    let bytes = standalone.as_slice();

    #[cfg(target_os = "linux")]
    let executable = std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .map(|path| path.canonicalize())
        .transpose()?
        .unwrap_or(executable);
    validate_executable(bytes, std::env::consts::OS)?;
    let staged = executable.with_added_extension(format!("wbombo-new-{}", uuid::Uuid::new_v4()));
    write_staged(&staged, bytes)?;
    #[cfg(windows)]
    let result = launch_windows_helper(&executable, &staged);
    #[cfg(unix)]
    let result = replace_unix(&executable, &staged, false);
    if result.is_err() {
        let _ = fs::remove_file(&staged);
    }
    result
}

fn write_staged(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o755))?;
        }
        Ok(())
    })();
    drop(file);
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result
}

fn validate_executable(bytes: &[u8], os: &str) -> Result<()> {
    let valid = match os {
        "windows" => bytes.starts_with(b"MZ"),
        "linux" => bytes.starts_with(b"\x7fELF"),
        "macos" => [
            b"\xcf\xfa\xed\xfe",
            b"\xfe\xed\xfa\xcf",
            b"\xca\xfe\xba\xbe",
            b"\xbe\xba\xfe\xca",
            b"\xca\xfe\xba\xbf",
            b"\xbf\xba\xfe\xca",
        ]
        .iter()
        .any(|magic| bytes.starts_with(*magic)),
        _ => false,
    };
    ensure!(valid, "artifact has an unexpected executable format");
    Ok(())
}

#[cfg(any(target_os = "macos", test))]
fn app_path(executable: &Path) -> Option<&Path> {
    let macos = executable.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    (macos.file_name()? == "MacOS" && contents.file_name()? == "Contents" && app.extension()? == "app").then_some(app)
}

#[cfg(any(target_os = "macos", test))]
fn archive_path(path: &Path) -> Result<Vec<OsString>> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {},
            Component::Normal(part) => parts.push(part.to_owned()),
            _ => anyhow::bail!("archive path escapes the bundle"),
        }
    }
    ensure!(
        !parts.is_empty() && Path::new(&parts[0]).extension() == Some(OsStr::new("app")),
        "archive must contain an application bundle"
    );
    Ok(parts)
}

#[cfg(any(target_os = "macos", test))]
fn checked_link(parts: &[OsString], link: &Path, hard: bool) -> Result<()> {
    let mut target = if hard {
        Vec::new()
    } else {
        parts[..parts.len() - 1].to_vec()
    };
    for component in link.components() {
        match component {
            Component::CurDir => {},
            Component::Normal(part) => target.push(part.to_owned()),
            Component::ParentDir if target.len() > 1 => {
                target.pop();
            },
            _ => anyhow::bail!("archive link escapes the bundle"),
        }
    }
    ensure!(target.first() == parts.first(), "archive link leaves the bundle");
    Ok(())
}

#[cfg(any(target_os = "macos", test))]
fn unpack_bundle(bytes: &[u8], destination: &Path) -> Result<PathBuf> {
    let decoder = flate2::bufread::GzDecoder::new(Cursor::new(bytes));
    let mut archive = tar::Archive::new(decoder);
    let mut bundle = None;
    let mut expanded = 0u64;
    let mut count = 0usize;
    for entry in archive.entries()? {
        let mut entry = entry?;
        count += 1;
        ensure!(count <= 50_000, "application archive has too many entries");
        expanded = expanded.checked_add(entry.size()).context("archive size overflow")?;
        ensure!(expanded <= MAX_EXPANDED, "application archive expands beyond the limit");
        let parts = archive_path(&entry.path()?)?;
        if let Some(root) = &bundle {
            ensure!(root == &parts[0], "archive contains multiple bundles");
        } else {
            bundle = Some(parts[0].clone());
        }
        let kind = entry.header().entry_type();
        ensure!(
            kind.is_file() || kind.is_dir() || kind.is_symlink() || kind.is_hard_link(),
            "unsupported archive entry"
        );
        if kind.is_symlink() || kind.is_hard_link() {
            let link = entry.link_name()?.context("archive link has no target")?;
            checked_link(&parts, &link, kind.is_hard_link())?;
        }
        ensure!(entry.unpack_in(destination)?, "archive entry was not contained");
    }
    Ok(destination.join(bundle.context("application archive is empty")?))
}

#[cfg(any(target_os = "macos", test))]
fn extract_macos_executable(bytes: &[u8]) -> Result<Vec<u8>> {
    let decoder = flate2::bufread::GzDecoder::new(Cursor::new(bytes));
    let mut archive = tar::Archive::new(decoder);
    let mut binary = None;
    let mut count = 0usize;
    let mut expanded = 0u64;
    for entry in archive.entries()? {
        let mut entry = entry?;
        count += 1;
        ensure!(count <= 50_000, "application archive has too many entries");
        expanded = expanded.checked_add(entry.size()).context("archive size overflow")?;
        ensure!(expanded <= MAX_EXPANDED, "application archive expands beyond the limit");
        let parts = archive_path(&entry.path()?)?;
        if parts.len() != 4 || parts[1] != "Contents" || parts[2] != "MacOS" || !entry.header().entry_type().is_file() {
            continue;
        }
        ensure!(
            binary.is_none() && entry.size() <= MAX_EXECUTABLE,
            "ambiguous or oversized application executable"
        );
        let mut bytes = Vec::new();
        entry.by_ref().take(MAX_EXECUTABLE + 1).read_to_end(&mut bytes)?;
        ensure!(bytes.len() as u64 <= MAX_EXECUTABLE, "application executable is too large");
        validate_executable(&bytes, "macos")?;
        binary = Some(bytes);
    }
    binary.context("application executable is missing")
}

#[cfg(any(target_os = "macos", test))]
fn bundle_executable(bundle: &Path) -> Result<PathBuf> {
    let mut found = None;
    for entry in fs::read_dir(bundle.join("Contents/MacOS"))? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            ensure!(found.is_none(), "application has multiple executables");
            ensure!(entry.metadata()?.len() <= MAX_EXECUTABLE, "application executable is too large");
            let bytes = fs::read(entry.path())?;
            validate_executable(&bytes, "macos")?;
            found = Some(entry.path());
        }
    }
    found.context("application executable is missing")
}

fn previous_path(target: &Path) -> PathBuf {
    target.with_added_extension(format!("wbombo-previous-{}", uuid::Uuid::new_v4()))
}

#[cfg(any(unix, test))]
fn replace_with_backup(target: &Path, staged: &Path) -> Result<PathBuf> {
    ensure!(
        target.is_absolute() && target.parent() == staged.parent(),
        "replacement must be a sibling of the target"
    );
    let backup = previous_path(target);
    ensure!(!backup.try_exists()?, "backup path already exists");
    fs::rename(target, &backup)?;
    if let Err(error) = fs::rename(staged, target) {
        fs::rename(&backup, target).context("replacement failed and backup restoration failed")?;
        return Err(error.into());
    }
    Ok(backup)
}

#[cfg(unix)]
fn replace_unix(target: &Path, staged: &Path, app: bool) -> Result<()> {
    let backup = replace_with_backup(target, staged)?;
    // Pass paths as positional arguments, never interpolate them into shell code.
    // Retain the previous installation for recovery; never delete an existing
    // backup. Restore it immediately if helper creation fails.
    let script = if app {
        "while kill -0 \"$1\" 2>/dev/null; do sleep 0.2; done; exec /usr/bin/open -n \"$2\""
    } else {
        "while kill -0 \"$1\" 2>/dev/null; do sleep 0.2; done; exec \"$2\""
    };
    let result = Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .arg("wbombo-update")
        .arg(std::process::id().to_string())
        .arg(target)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    if let Err(error) = result {
        let failed = target.with_added_extension(format!("wbombo-failed-{}", uuid::Uuid::new_v4()));
        fs::rename(target, &failed)?;
        fs::rename(backup, target).context("could not restore previous installation")?;
        if failed.is_dir() {
            let _ = fs::remove_dir_all(failed);
        } else {
            let _ = fs::remove_file(failed);
        }
        return Err(error.into());
    }
    Ok(())
}

#[cfg(windows)]
fn windows_script(target: &Path, staged: &Path, pid: u32) -> Result<String> {
    ensure!(
        target.is_absolute() && target.parent() == staged.parent(),
        "replacement must be a sibling of the target"
    );
    let literal = |path: &Path| -> Result<String> {
        let value = path.to_str().context("update path is not Unicode")?;
        Ok(format!("'{}'", value.replace('\'', "''")))
    };
    Ok(format!(
        r#"$ErrorActionPreference = 'Stop'
$target = {target}
$staged = {staged}
$backup = {backup}
$backedUp = $false
Wait-Process -Id {pid} -ErrorAction SilentlyContinue
try {{
    if (Test-Path -LiteralPath $backup) {{ throw 'Update backup already exists' }}
    Move-Item -LiteralPath $target -Destination $backup
    $backedUp = $true
    Move-Item -LiteralPath $staged -Destination $target
    Start-Process -FilePath $target -WorkingDirectory (Split-Path -Parent $target)
}} catch {{
    if ($backedUp) {{
        if (Test-Path -LiteralPath $target) {{ Move-Item -LiteralPath $target -Destination $staged -Force }}
        Move-Item -LiteralPath $backup -Destination $target
    }}
    Start-Process -FilePath $target -WorkingDirectory (Split-Path -Parent $target)
}} finally {{
    if (Test-Path -LiteralPath $staged) {{ Remove-Item -LiteralPath $staged -Force }}
}}
"#,
        target = literal(target)?,
        staged = literal(staged)?,
        backup = literal(&previous_path(target))?
    ))
}

#[cfg(windows)]
fn launch_windows_helper(target: &Path, staged: &Path) -> Result<()> {
    use base64::Engine;
    use std::os::windows::process::CommandExt;
    let script = windows_script(target, staged, std::process::id())?;
    let encoded: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-EncodedCommand",
        ])
        .arg(base64::engine::general_purpose::STANDARD.encode(encoded))
        .creation_flags(0x08000000)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("could not start launcher replacement helper")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let parent = std::env::temp_dir().canonicalize().unwrap();
            let path = parent.join(format!("wbombo-updater-test-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            assert_eq!(self.0.parent(), Some(std::env::temp_dir().canonicalize().unwrap().as_path()));
            assert!(self.0.file_name().unwrap().to_string_lossy().starts_with("wbombo-updater-test-"));
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn archive(files: &[(&str, &[u8])]) -> Vec<u8> {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        let mut archive = tar::Builder::new(encoder);
        for (name, bytes) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            archive.append_data(&mut header, name, *bytes).unwrap();
        }
        archive.into_inner().unwrap().finish().unwrap()
    }

    const MACHO: &[u8] = b"\xcf\xfa\xed\xfe-synthetic-fixture-never-executed";

    #[test]
    fn macos_archive_supports_bundle_and_standalone() {
        let bytes = archive(&[
            ("Example.app/Contents/MacOS/launcher", MACHO),
            ("Example.app/Contents/Resources/data", b"resource"),
        ]);
        assert_eq!(extract_macos_executable(&bytes).unwrap(), MACHO);
        let scratch = Scratch::new();
        let app = unpack_bundle(&bytes, &scratch.0).unwrap();
        assert_eq!(app, scratch.0.join("Example.app"));
        assert_eq!(bundle_executable(&app).unwrap(), app.join("Contents/MacOS/launcher"));
        assert_eq!(app_path(&app.join("Contents/MacOS/launcher")), Some(app.as_path()));
        assert!(app_path(&app.join("Contents/Resources/launcher")).is_none());
        assert!(app_path(&scratch.0.join("Example/Contents/MacOS/launcher")).is_none());
    }

    #[test]
    fn macos_archive_rejects_ambiguous_or_missing_executables() {
        let multiple = archive(&[
            ("Example.app/Contents/MacOS/first", MACHO),
            ("Example.app/Contents/MacOS/second", MACHO),
        ]);
        assert!(extract_macos_executable(&multiple).is_err());
        let missing = archive(&[("Example.app/Contents/Resources/data", b"data")]);
        assert!(extract_macos_executable(&missing).is_err());
        let wrong = archive(&[("Example.app/Contents/MacOS/launcher", b"MZ-wrong-platform")]);
        assert!(extract_macos_executable(&wrong).is_err());
        let multiple_bundles = archive(&[
            ("First.app/Contents/MacOS/launcher", MACHO),
            ("Second.app/Contents/MacOS/launcher", MACHO),
        ]);
        let scratch = Scratch::new();
        assert!(unpack_bundle(&multiple_bundles, &scratch.0).is_err());
    }

    #[test]
    fn archive_paths_and_links_cannot_escape_the_bundle() {
        assert!(archive_path(Path::new("../escape")).is_err());
        assert!(archive_path(Path::new("Example.app/../../escape")).is_err());
        assert!(archive_path(Path::new("/Example.app/Contents/file")).is_err());
        assert!(archive_path(Path::new("unexpected/Contents/file")).is_err());
        let parts = archive_path(Path::new("Example.app/Contents/Frameworks/Library")).unwrap();
        assert!(checked_link(&parts, Path::new("../Library"), false).is_ok());
        assert!(checked_link(&parts, Path::new("../../../escape"), false).is_err());
        assert!(checked_link(&parts, Path::new("Other.app/file"), true).is_err());
        assert!(checked_link(&parts, Path::new("Example.app/Contents/file"), true).is_ok());
    }

    #[test]
    fn replacement_retains_previous_and_restores_on_failure() {
        let scratch = Scratch::new();
        let target = scratch.0.join("launcher");
        let staged = scratch.0.join("next");
        fs::write(&target, b"previous").unwrap();
        fs::write(&staged, b"candidate").unwrap();
        let backup = replace_with_backup(&target, &staged).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"candidate");
        assert_eq!(fs::read(&backup).unwrap(), b"previous");
        assert!(!staged.exists());
        assert!(replace_with_backup(&target, &staged).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"candidate");
        assert_eq!(fs::read(&backup).unwrap(), b"previous");
    }

    #[test]
    fn staging_does_not_overwrite_and_format_matches_platform() {
        let scratch = Scratch::new();
        let staged = scratch.0.join("next");
        write_staged(&staged, b"MZ-fixture").unwrap();
        assert!(write_staged(&staged, b"overwrite").is_err());
        assert_eq!(fs::read(&staged).unwrap(), b"MZ-fixture");
        assert!(validate_executable(b"MZ-fixture", "windows").is_ok());
        assert!(validate_executable(b"\x7fELF-fixture", "linux").is_ok());
        assert!(validate_executable(MACHO, "macos").is_ok());
        assert!(validate_executable(b"MZ-fixture", "linux").is_err());
    }

    #[cfg(windows)]
    #[test]
    fn windows_helper_quotes_literal_paths_without_replaying_cli_arguments() {
        let target = Path::new("C:\\app $data\\O'Brien portable.exe");
        let staged = Path::new("C:\\app $data\\next.exe");
        let script = windows_script(target, staged, 12345).unwrap();
        assert!(script.contains("$target = 'C:\\app $data\\O''Brien portable.exe'"));
        assert!(script.contains("Wait-Process -Id 12345"));
        assert!(script.contains("Move-Item -LiteralPath $staged"));
        assert!(!script.contains("--run-instance"));
        assert!(windows_script(target, Path::new("D:\\different\\next.exe"), 12345).is_err());
    }
}
