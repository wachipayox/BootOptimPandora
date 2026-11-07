//! Migration from another launcher's folder (`--migrate-legacy`), run by the
//! Windows installer.
//!
//! The source folder is only ever read. The order matters:
//!
//! 1. Pack files the player already has (same path, size and SHA-256 as the
//!    profile's signed objects) are copied into the download cache, so they are
//!    not downloaded again.
//! 2. The instance is created from the profile exactly like the GUI does. Copying
//!    player files first would make the pack's paths "local" and stop them from
//!    receiving updates.
//! 3. Only an allow-list of player data is copied, never mods or other configs.
//! 4. The old `options.txt` becomes the launcher-wide Minecraft settings.
//!
//! Progress lines go to stdout as they happen, for the installer's log.
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use bridge::{
    message::CliStartOutcome,
    modal_action::{ModalAction, ProgressTrackerFinishType},
};

use crate::{
    BackendState,
    cli_instances::{PREFIX, done, failed, find, name_is_usable, refused, text},
    distribution::{DistributionClient, ManifestObjectRef, selected_revision, verified_cache_path, verify_file},
    profile_branch::GlobalRevisionPin,
};

/// Player folders copied whole. `saves` is handled per world.
const FOLDERS: &[&str] = &[
    "screenshots",
    "xaero",
    "journeymap",
    "schematics",
    "Sable-Schematics",
    "music_sheets",
    "replay_recordings",
    "config/litematica",
];
/// Player files. The first two replace the profile's first-run defaults.
const FILES: &[&str] = &["options.txt", "servers.dat", "hotbar.nbt", "command_history.txt"];
const REPLACES_DEFAULTS: &[&str] = &["options.txt", "servers.dat"];
/// Only what the player added on top of the old pack.
const ADDED_CONTENT: &[&str] = &["resourcepacks", "shaderpacks"];
const MODS_REPORT: &str = "mods-del-launcher-anterior.txt";

/// One progress line on stdout. The installer reads a pipe with `nsExec`, which
/// decodes the Windows ANSI code page, so a pipe gets that encoding; a console
/// gets UTF-8 as usual.
fn say(line: impl AsRef<str>) {
    use std::io::IsTerminal;
    let mut stdout = std::io::stdout().lock();
    let line = format!(
        "{}
",
        line.as_ref()
    );
    #[cfg(windows)]
    if !stdout.is_terminal() {
        let _ = stdout.write_all(&to_ansi(&line));
        let _ = stdout.flush();
        return;
    }
    let _ = stdout.write_all(line.as_bytes());
    let _ = stdout.flush();
}

#[cfg(windows)]
fn to_ansi(text: &str) -> Vec<u8> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn WideCharToMultiByte(
            code_page: u32,
            flags: u32,
            wide: *const u16,
            wide_len: i32,
            out: *mut u8,
            out_len: i32,
            default_char: *const u8,
            used_default: *mut i32,
        ) -> i32;
    }
    const CP_ACP: u32 = 0;
    let wide: Vec<u16> = text.encode_utf16().collect();
    let Ok(wide_len) = i32::try_from(wide.len()) else {
        return text.as_bytes().to_vec();
    };
    let null = std::ptr::null_mut();
    // SAFETY: lengths match the buffers; a null output buffer only asks for the size.
    let size =
        unsafe { WideCharToMultiByte(CP_ACP, 0, wide.as_ptr(), wide_len, null, 0, std::ptr::null(), null.cast()) };
    if size <= 0 {
        return text.as_bytes().to_vec();
    }
    let mut out = vec![0u8; size as usize];
    let written = unsafe {
        WideCharToMultiByte(CP_ACP, 0, wide.as_ptr(), wide_len, out.as_mut_ptr(), size, std::ptr::null(), null.cast())
    };
    out.truncate(written.max(0) as usize);
    out
}

fn megabytes(bytes: u64) -> String {
    let mb = bytes as f64 / 1_048_576.0;
    if mb >= 1024.0 {
        format!("{:.1} GB", mb / 1024.0).replace('.', ",")
    } else if mb >= 1.0 {
        format!("{mb:.0} MB")
    } else {
        "menos de 1 MB".into()
    }
}

fn native(relative: &str) -> PathBuf {
    relative.split('/').collect()
}

/// Paths the old pack itself provided, from AutoModpack's own list.
fn automodpack_paths(source: &Path) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    let Ok(modpacks) = std::fs::read_dir(source.join("automodpack").join("modpacks")) else {
        return paths;
    };
    for modpack in modpacks.flatten() {
        let Ok(bytes) = std::fs::read(modpack.path().join("automodpack-content.json")) else {
            continue;
        };
        let Ok(json) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            continue;
        };
        for entry in json.get("list").and_then(|list| list.as_array()).into_iter().flatten() {
            if let Some(file) = entry.get("file").and_then(|file| file.as_str()) {
                paths.insert(file.trim_start_matches('/').to_string());
            }
        }
    }
    paths
}

fn is_link(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| {
        metadata.file_type().is_symlink() || {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                // Junctions are reparse points that Rust does not report as symlinks.
                metadata.file_attributes() & 0x400 != 0
            }
            #[cfg(not(windows))]
            {
                false
            }
        }
    })
}

fn tree_size(path: &Path) -> u64 {
    if is_link(path) {
        return 0;
    }
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => metadata.len(),
        Ok(metadata) if metadata.is_dir() => std::fs::read_dir(path)
            .map(|entries| entries.flatten().map(|entry| tree_size(&entry.path())).sum())
            .unwrap_or(0),
        _ => 0,
    }
}

/// Copies `from` into `to` without replacing anything already there, never
/// following links. Returns the bytes copied.
fn copy_missing(from: &Path, to: &Path) -> std::io::Result<u64> {
    if is_link(from) {
        return Ok(0);
    }
    let metadata = std::fs::metadata(from)?;
    if metadata.is_file() {
        if to.exists() {
            return Ok(0);
        }
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        return std::fs::copy(from, to);
    }
    std::fs::create_dir_all(to)?;
    let mut copied = 0;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        copied += copy_missing(&entry.path(), &to.join(entry.file_name()))?;
    }
    Ok(copied)
}

/// A free folder name next to `wanted`: `wanted`, then `wanted (migrado)`, `(migrado 2)`…
fn free_name(parent: &Path, wanted: &str) -> PathBuf {
    let first = parent.join(wanted);
    if !first.exists() {
        return first;
    }
    (1..)
        .map(|n| {
            parent.join(if n == 1 {
                format!("{wanted} (migrado)")
            } else {
                format!("{wanted} (migrado {n})")
            })
        })
        .find(|path| !path.exists())
        .unwrap()
}

/// Puts pack files the player already has into the verified download cache.
fn reuse_pack_files(source: &Path, objects: &BTreeMap<String, ManifestObjectRef>, cache_root: &Path) -> (usize, u64) {
    let (mut files, mut bytes) = (0, 0);
    for (path, object) in objects {
        let candidate = source.join(native(path));
        if is_link(&candidate) || std::fs::metadata(&candidate).map_or(true, |m| !m.is_file() || m.len() != object.size)
        {
            continue;
        }
        let Ok(None) = verified_cache_path(object, cache_root) else {
            continue; // already cached, or an invalid reference the download will report
        };
        if !verify_file(&candidate, &object.sha256, object.size).unwrap_or(false) {
            continue;
        }
        let destination = cache_root.join("sha256").join(&object.sha256[..2]).join(&object.sha256);
        let temp = destination.with_extension("migrate-tmp");
        let copied = std::fs::copy(&candidate, &temp)
            .is_ok_and(|_| verify_file(&temp, &object.sha256, object.size).unwrap_or(false))
            && std::fs::rename(&temp, &destination).is_ok();
        if copied {
            files += 1;
            bytes += object.size;
        } else {
            let _ = std::fs::remove_file(&temp);
        }
    }
    (files, bytes)
}

fn source_problem(source: &Path, launcher_dir: &Path) -> Option<&'static str> {
    if !source.is_dir() {
        return Some("source-not-a-folder");
    }
    let canonical = source.canonicalize().ok()?;
    if launcher_dir
        .canonicalize()
        .is_ok_and(|launcher| canonical.starts_with(&launcher) || launcher.starts_with(&canonical))
    {
        return Some("source-overlaps-launcher-data");
    }
    let looks_like_minecraft =
        ["options.txt", "saves", "mods", "config"].iter().any(|entry| source.join(entry).exists());
    (!looks_like_minecraft).then_some("source-is-not-a-minecraft-folder")
}

pub async fn migrate(
    backend: &Arc<BackendState>,
    source: &Path,
    name: &str,
    profile_id: &str,
    include_voxy: bool,
) -> CliStartOutcome {
    if !name_is_usable(name) {
        return refused(name, "invalid-instance-name");
    }
    if let Some(reason) = source_problem(source, &backend.directories.root_launcher_dir) {
        return refused(name, reason);
    }
    let instance_name = {
        let mut candidate = name.to_string();
        let mut n = 1;
        while find(backend, &candidate).is_some() {
            n += 1;
            candidate = if n == 2 {
                format!("{name} (migrada)")
            } else {
                format!("{name} (migrada {n})")
            };
        }
        candidate
    };

    say("Buscando la última versión publicada del modpack…");
    let config = backend.config.lock().get().distribution.clone();
    let client = match DistributionClient::new(&config) {
        Ok(client) => client,
        Err(error) => return failed("MIGRATE-FAILED", name, &error.to_string()),
    };
    let catalog = match client.list_profiles_with_tests(false).await {
        Ok(catalog) => catalog,
        Err(error) => return failed("MIGRATE-FAILED", name, &format!("catalog: {error}")),
    };
    let Some(profile) = catalog.iter().find(|profile| profile.profile_id == profile_id) else {
        return failed("MIGRATE-FAILED", name, "profile-not-published");
    };
    // The revision the launcher itself installs and updates to.
    let target = selected_revision(profile).clone();
    let pin = GlobalRevisionPin {
        profile_id: profile_id.to_string(),
        revision_id: target.revision_id.clone(),
        manifest_sha256: target.manifest_sha256.clone(),
    };
    let metadata = match client.resolve_profile_metadata(&pin).await {
        Ok(metadata) => metadata,
        Err(error) => return failed("MIGRATE-FAILED", name, &format!("manifest: {error}")),
    };

    // Room for the player's data and the whole pack, with a margin.
    let pack_bytes: u64 = metadata.objects_by_path.values().map(|object| object.size).sum();
    let source_owned = source.to_path_buf();
    let player_bytes = tokio::task::spawn_blocking(move || planned_bytes(&source_owned, include_voxy))
        .await
        .unwrap_or(0);
    let needed = player_bytes + pack_bytes + 1024 * 1024 * 1024;
    if let Some(free) = crate::launch::aot_cache::free_disk_bytes(&backend.directories.instances_dir)
        && free < needed
    {
        say(format!(
            "No hay espacio suficiente: hacen falta unos {} y quedan {} libres.",
            megabytes(needed),
            megabytes(free)
        ));
        return failed("MIGRATE-FAILED", name, &format!("not-enough-space needed={needed} free={free}"));
    }

    say("Buscando archivos del modpack que ya tienes…");
    let cache_root = backend.directories.root_launcher_dir.join("distribution-objects");
    let objects = metadata.objects_by_path.clone();
    let source_owned = source.to_path_buf();
    let cache_owned = cache_root.clone();
    let (reused, reused_bytes) =
        tokio::task::spawn_blocking(move || reuse_pack_files(&source_owned, &objects, &cache_owned))
            .await
            .unwrap_or((0, 0));
    if reused > 0 {
        say(format!(
            "Se reutilizan {reused} archivos del modpack ({}); no hace falta descargarlos.",
            megabytes(reused_bytes)
        ));
    }

    say(format!("Descargando e instalando el modpack en la instancia «{instance_name}»…"));
    let modal_action = ModalAction::default();
    let progress = modal_action.push_tracker("Preparing modpack assets".into());
    progress.set_total(1);
    if let Err(error) = backend
        .create_global_profile_instance(&instance_name, profile_id, &target, &modal_action, &progress, None, false)
        .await
    {
        progress.set_finished(ProgressTrackerFinishType::Error);
        modal_action.set_finished_with_error(error.clone().into());
        // Nothing of the player's was copied yet; drop the half-made instance so a retry starts clean.
        if let Some(found) = find(backend, &instance_name)
            && found.root.parent() == Some(&*backend.directories.instances_dir)
            && std::fs::remove_dir_all(&found.root).is_ok()
        {
            backend.remove_instance(found.id);
        }
        return failed("MIGRATE-FAILED", name, &error);
    }
    progress.set_finished(ProgressTrackerFinishType::Normal);
    modal_action.set_finished();
    let Some(found) = find(backend, &instance_name) else {
        return failed("MIGRATE-FAILED", name, "instance-not-loaded");
    };

    let profile_paths: BTreeSet<String> = metadata.objects_by_path.keys().cloned().collect();
    let source_owned = source.to_path_buf();
    let dot_minecraft = found.dot_minecraft.to_path_buf();
    let copy = tokio::task::spawn_blocking(move || {
        copy_player_data(&source_owned, &dot_minecraft, &profile_paths, include_voxy)
    })
    .await;
    let copied = match copy {
        Ok(Ok(copied)) => copied,
        result => {
            let error = match result {
                Ok(Err(error)) => error.to_string(),
                Err(error) => error.to_string(),
                Ok(Ok(_)) => unreachable!(),
            };
            // The source is intact; leave no half-migrated instance behind. Links into shared
            // folders are removed, not followed.
            if found.root.parent() == Some(&*backend.directories.instances_dir)
                && std::fs::remove_dir_all(&found.root).is_ok()
            {
                backend.remove_instance(found.id);
            }
            return failed("MIGRATE-COPY-FAILED", &instance_name, &error);
        },
    };

    let old_options = source.join("options.txt");
    if old_options.is_file() {
        match crate::global_options::import(&backend.directories.root_launcher_dir, &old_options, &instance_name) {
            Ok(count) => {
                backend.config.lock().modify(|config| config.global_options = true);
                say(format!("Tus ajustes de Minecraft ({count}) son ahora los globales del launcher."));
            },
            Err(error) => log::warn!("Migration could not import options.txt: {error}"),
        }
    }

    done(vec![format!(
        "{PREFIX} MIGRATED instance={} profile={profile_id} rev {} reused_files={reused} copied_bytes={copied} source={}",
        text(&instance_name),
        target.revision_id,
        text(&source.to_string_lossy()),
    )])
}

/// Upper bound of what [`copy_player_data`] copies.
fn planned_bytes(source: &Path, include_voxy: bool) -> u64 {
    let folders = FOLDERS
        .iter()
        .chain(include_voxy.then_some(&".voxy"))
        .chain(ADDED_CONTENT)
        .chain(&["saves"]);
    folders.map(|folder| tree_size(&source.join(native(folder)))).sum::<u64>()
        + FILES.iter().map(|file| tree_size(&source.join(file))).sum::<u64>()
}

fn copy_player_data(
    source: &Path,
    dot_minecraft: &Path,
    profile_paths: &BTreeSet<String>,
    include_voxy: bool,
) -> std::io::Result<u64> {
    let mut copied = 0;

    // Worlds one by one: two worlds with one name must never be merged.
    let saves = source.join("saves");
    if saves.is_dir() && !is_link(&saves) {
        say(format!("Copiando tus mundos ({})…", megabytes(tree_size(&saves))));
        for world in std::fs::read_dir(&saves)?.flatten() {
            let name = world.file_name().to_string_lossy().into_owned();
            let destination = free_name(&dot_minecraft.join("saves"), &name);
            copied += copy_missing(&world.path(), &destination)?;
        }
    }

    let mut folders: Vec<&str> = FOLDERS.to_vec();
    if include_voxy {
        folders.push(".voxy");
    }
    for folder in folders {
        let from = source.join(native(folder));
        if from.is_dir() && !is_link(&from) {
            say(format!("Copiando {folder} ({})…", megabytes(tree_size(&from))));
            copied += copy_missing(&from, &dot_minecraft.join(native(folder)))?;
        }
    }

    for file in FILES {
        let from = source.join(file);
        if !from.is_file() || is_link(&from) {
            continue;
        }
        let to = dot_minecraft.join(file);
        if REPLACES_DEFAULTS.contains(file) {
            copied += std::fs::copy(&from, &to)?;
        } else {
            copied += copy_missing(&from, &to)?;
        }
    }

    // Resource and shader packs the player added: not the old pack's, not the new one's.
    let old_pack = automodpack_paths(source);
    for folder in ADDED_CONTENT {
        let Ok(entries) = std::fs::read_dir(source.join(folder)) else {
            continue;
        };
        let mut count = 0;
        for entry in entries.flatten() {
            let relative = format!("{folder}/{}", entry.file_name().to_string_lossy());
            let from_pack = old_pack.contains(&relative)
                || profile_paths.contains(&relative)
                || profile_paths.iter().any(|path| path.starts_with(&format!("{relative}/")));
            if !from_pack {
                copied += copy_missing(&entry.path(), &dot_minecraft.join(native(&relative)))?;
                count += 1;
            }
        }
        if count > 0 {
            say(format!("Copiados {count} elementos tuyos de {folder}."));
        }
    }

    // Mods are never copied; list the ones the player had added, for reference.
    if let Ok(entries) = std::fs::read_dir(source.join("mods")) {
        let mut added: Vec<String> = entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|file| {
                let lower = file.to_ascii_lowercase();
                lower.ends_with(".jar") || lower.ends_with(".jar.disabled")
            })
            .filter(|file| {
                let relative = format!("mods/{file}");
                !old_pack.contains(&relative) && !profile_paths.contains(&relative)
            })
            .collect();
        added.sort_by_key(|file| file.to_lowercase());
        if !added.is_empty() {
            let mut report = String::from(
                "Mods que tenías en tu launcher anterior y que no forman parte del modpack.\n\
                 No se han copiado: el modpack instala los suyos. Si quieres alguno, añádelo desde el launcher.\n\n",
            );
            for file in &added {
                report.push_str(file);
                report.push('\n');
            }
            std::fs::write(dot_minecraft.join(MODS_REPORT), report)?;
            say(format!("Tenías {} mods añadidos por ti; la lista está en {MODS_REPORT}.", added.len()));
        }
    }
    Ok(copied)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn copies_only_player_data() {
        let temp = TempDir(std::env::temp_dir().join(format!("pandora-migration-{}", uuid::Uuid::new_v4())));
        let source = temp.0.join("old");
        let dot = temp.0.join("new");
        write(&source.join("saves/Mundo/level.dat"), "old world");
        write(&source.join("config/litematica/x.json"), "mine");
        write(&source.join("config/jei/jei-client.ini"), "not copied");
        write(&source.join("mods/pack.jar"), "pack");
        write(&source.join("mods/extra.jar"), "mine");
        write(&source.join("mods/.connector/cache.bin"), "not a mod");
        write(&source.join("resourcepacks/pack.zip"), "pack");
        write(&source.join("resourcepacks/mine.zip"), "mine");
        write(&source.join("shaderpacks/Shader/a.txt"), "mine");
        write(&source.join("options.txt"), "version:3955\nfov:0.5\n");
        write(&source.join(".voxy/a"), "lod");
        write(
            &source.join("automodpack/modpacks/W/automodpack-content.json"),
            r#"{"list":[{"file":"/mods/pack.jar"},{"file":"/resourcepacks/pack.zip"}]}"#,
        );
        // The profile already created these.
        write(&dot.join("saves/Mundo/level.dat"), "group world");
        write(&dot.join("options.txt"), "version:3955\nfov:0.0\n");
        write(&dot.join("mods/pack.jar"), "pack");

        let profile: BTreeSet<String> = ["mods/pack.jar".to_string(), "options.txt".to_string()].into();
        copy_player_data(&source, &dot, &profile, false).unwrap();

        assert_eq!(std::fs::read_to_string(dot.join("saves/Mundo/level.dat")).unwrap(), "group world");
        assert_eq!(std::fs::read_to_string(dot.join("saves/Mundo (migrado)/level.dat")).unwrap(), "old world");
        assert!(dot.join("config/litematica/x.json").is_file());
        assert!(!dot.join("config/jei").exists());
        assert!(!dot.join("mods/extra.jar").exists());
        assert!(dot.join("resourcepacks/mine.zip").is_file());
        assert!(!dot.join("resourcepacks/pack.zip").exists());
        assert!(dot.join("shaderpacks/Shader/a.txt").is_file());
        assert!(!dot.join(".voxy").exists());
        assert_eq!(std::fs::read_to_string(dot.join("options.txt")).unwrap(), "version:3955\nfov:0.5\n");
        let report = std::fs::read_to_string(dot.join(MODS_REPORT)).unwrap();
        assert!(report.contains("extra.jar") && !report.contains("pack.jar") && !report.contains(".connector"));
        // The source is untouched.
        assert!(source.join("mods/extra.jar").is_file());
    }
}
