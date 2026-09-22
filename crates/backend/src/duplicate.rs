use sha1::Digest;
use sha2::Sha256;
use uuid::Uuid;
use std::{
    fs,
    io::{Error, ErrorKind, Read, Result, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use bridge::{
    instance::InstanceID,
    modal_action::{ModalAction, ProgressTrackerFinishType},
};

use crate::{
    BackendState,
    profile_layout_identity::{CONTROL_DIR_NAME, begin_profile_clone_destination, prepare_profile_clone_source},
};

const EXACT_APPCDS_CANDIDATE_DIR: &str = "appcds-exact-clone-candidate";
const EXACT_APPCDS_STAGING_PREFIX: &str = "appcds-exact-clone-staging-";

#[derive(Debug)]
struct ExactAppCdsCloneSource {
    source_root: PathBuf,
    source_plan: Vec<u8>,
    source_plan_sha256: String,
    archive_path: PathBuf,
    archive_sha256: String,
    archive_size: u64,
    source_profile_uuid: Uuid,
}

fn find_content_library_path(content_library_dir: &Path, hash: [u8; 20], path: &Path) -> Option<PathBuf> {
    let extension = path.extension().and_then(|s| s.to_str());
    let lib_path = crate::fs::create_content_library_path(content_library_dir, hash, extension);
    if lib_path.exists() {
        return Some(lib_path);
    }

    let disabled_extension = path
        .file_name()
        .and_then(|s| s.to_str())
        .and_then(|filename| filename.strip_suffix(".disabled"))
        .and_then(|base| Path::new(base).extension())
        .and_then(|s| s.to_str());
    let lib_path = crate::fs::create_content_library_path(content_library_dir, hash, disabled_extension);
    lib_path.exists().then_some(lib_path)
}

fn hash_file(path: &Path, buf: &mut [u8], check_cancel: &dyn Fn() -> Result<()>) -> Result<[u8; 20]> {
    let mut file = fs::File::open(path)?;
    let mut hasher = sha1::Sha1::default();
    loop {
        check_cancel()?;
        let read = file.read(buf)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(hasher.finalize().into())
}

fn copy_file(from: &Path, to: &Path, buf: &mut [u8], check_cancel: &dyn Fn() -> Result<()>) -> Result<u64> {
    let mut src = fs::File::open(from)?;
    let mut dst = fs::File::create(to)?;
    let mut total = 0_u64;
    loop {
        check_cancel()?;
        let read = src.read(buf)?;
        if read == 0 {
            break;
        }
        dst.write_all(&buf[..read])?;
        total += read as u64;
    }

    let metadata = fs::metadata(from)?;
    fs::set_permissions(to, metadata.permissions())?;
    if let Ok(modified) = metadata.modified() {
        let _ = dst.set_times(fs::FileTimes::new().set_modified(modified));
    }

    Ok(total)
}

fn duplicate_with_content_library(
    from: &Path,
    to: &Path,
    content_library_dir: &Path,
    progress: &dyn Fn(u64, u64),
    check_cancel: &dyn Fn() -> Result<()>,
) -> Result<()> {
    let from = from.canonicalize()?;
    if !from.is_dir() {
        return Err(ErrorKind::NotADirectory.into());
    }
    if !to.is_dir() {
        return Err(ErrorKind::AlreadyExists.into());
    }

    let mut directories = Vec::new();
    let mut files = Vec::new();
    let mut internal_symlinks = Vec::new();
    let mut external_symlinks = Vec::new();
    #[cfg(windows)]
    let mut internal_junctions = Vec::new();
    #[cfg(windows)]
    let mut external_junctions = Vec::new();

    let mut directories_to_visit = Vec::new();
    directories_to_visit.push((from.to_path_buf(), 0));

    while let Some((directory, depth)) = directories_to_visit.pop() {
        check_cancel()?;
        let read_dir = fs::read_dir(directory)?;
        for entry in read_dir {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            let Ok(relative) = path.strip_prefix(&from) else {
                return Err(Error::new(ErrorKind::Other, format!("{path:?} is not a child of {from:?}")));
            };
            // Persistent profile control state is identity/transaction state, not instance
            // payload. The destination namespace is created separately with a fresh UUID.
            let bootoptim_appcds_state = relative.parent() == Some(Path::new(".bootoptim"))
                && relative
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| {
                        name == "appcds"
                            || name.starts_with("appcds-unbound-v0-")
                            || name.starts_with("appcds-exact-clone-")
                    })
                    .unwrap_or(false);
            if relative == Path::new(CONTROL_DIR_NAME) || bootoptim_appcds_state {
                continue;
            }
            #[cfg(windows)]
            if let Ok(target) = junction::get_target(&path) {
                if let Ok(internal) = target.strip_prefix(&from) {
                    internal_junctions.push((relative.to_path_buf(), internal.to_path_buf()));
                } else {
                    external_junctions.push((relative.to_path_buf(), target));
                }
                continue;
            }
            if file_type.is_symlink() {
                let target = fs::read_link(&path)?;
                if let Ok(internal) = target.strip_prefix(&from) {
                    internal_symlinks.push((relative.to_path_buf(), internal.to_path_buf()));
                } else {
                    external_symlinks.push((relative.to_path_buf(), target));
                }
            } else if file_type.is_file() {
                files.push((relative.to_path_buf(), path));
            } else if file_type.is_dir() {
                if depth >= 256 {
                    return Err(ErrorKind::QuotaExceeded.into());
                }

                directories.push(relative.to_path_buf());
                directories_to_visit.push((path, depth + 1));
            }
        }
    }

    let total_files = files.len() as u64;
    progress(0, total_files);

    for directory in directories {
        check_cancel()?;
        _ = fs::create_dir(to.join(directory));
    }

    let mut files_done = 0_u64;
    let mut buf = vec![0_u8; 128 * 1024];
    for (relative, source_path) in &files {
        check_cancel()?;
        let dest = to.join(relative);

        if reflink_copy::reflink(source_path, &dest).is_ok() {
            files_done += 1;
            progress(files_done, total_files);
            continue;
        }

        // If the source_path was hard linked from the content library
        // We will make the duplicated file also hard linked
        if let Ok(source_metadata) = crate::fs::FileMetadata::new(source_path)
            && source_metadata.number_of_links() > 1
        {
            if let Ok(hash) = hash_file(source_path, &mut buf, check_cancel) {
                if let Some(lib_path) = find_content_library_path(content_library_dir, hash, source_path) {
                    if let Ok(lib_metadata) = crate::fs::FileMetadata::new(&lib_path)
                        && source_metadata.is_same(&lib_metadata)
                    {
                        if crate::fs::fastcopy(&lib_path, &dest, false, true).is_ok() {
                            files_done += 1;
                            progress(files_done, total_files);
                            continue;
                        }
                    }
                }
            }
        }

        copy_file(source_path, &dest, &mut buf, check_cancel)?;
        files_done += 1;
        progress(files_done, total_files);
    }

    for (relative, internal) in &internal_symlinks {
        let dest = to.join(relative);
        let target = to.join(internal);
        if let Err(err) = crate::fs::symlink_dir_or_file(&target, &dest) {
            return Err(err);
        }
    }
    for (relative, target) in &external_symlinks {
        let dest = to.join(relative);
        if let Err(err) = crate::fs::symlink_dir_or_file(&target, &dest) {
            return Err(err);
        }
    }
    #[cfg(windows)]
    for (relative, internal) in &internal_junctions {
        let dest = to.join(relative);
        let target = to.join(internal);
        if let Err(err) = junction::create(&target, &dest) {
            return Err(err);
        }
    }
    #[cfg(windows)]
    for (relative, target) in &external_junctions {
        let dest = to.join(relative);
        if let Err(err) = junction::create(&target, &dest) {
            return Err(err);
        }
    }

    Ok(())
}

pub async fn duplicate_instance(
    backend: Arc<BackendState>,
    id: InstanceID,
    name: &str,
    exact_clone: bool,
    modal_action: ModalAction,
) {
    if !crate::fs::is_single_component_path_str(name) {
        modal_action
            .set_finished_with_error(format!("Unable to duplicate instance, name must not be a path: {name}").into());
        return;
    }
    if !sanitize_filename::is_sanitized_with_options(
        name,
        sanitize_filename::OptionsForCheck {
            windows: true,
            ..Default::default()
        },
    ) {
        modal_action.set_finished_with_error(format!("Unable to duplicate instance, name is invalid: {name}").into());
        return;
    }
    if backend.instance_state.read().instances.iter().any(|i| i.name == name) {
        modal_action.set_finished_with_error("Unable to duplicate instance, name is already used".to_string().into());
        return;
    }

    let source = {
        let state = backend.instance_state.read();
        let Some(instance) = state.instances.get(id) else {
            modal_action.set_finished_with_error("Unable to duplicate instance, unknown id".to_string().into());
            return;
        };
        instance.root_path.clone()
    };

    // Persistent sources are cloned only from a hash-proven Ready snapshot. The returned source
    // guard stays alive through the copy, so a cooperating second Pandora process cannot enter
    // reconcile/Publishing while the snapshot is being duplicated. Legacy has no control state;
    // ambiguous/non-Ready persistent state is rejected rather than recovered or guessed here.
    let clone_source = match prepare_profile_clone_source(&source) {
        Ok(source) => source,
        Err(error) => {
            modal_action.set_finished_with_error(format!("Unable to duplicate instance safely: {error}").into());
            return;
        },
    };

    // Exact clone is deliberately stricter than normal duplication. The persistent-profile
    // source lock above is also the AppCDS namespace lease introduced by PR #48, so no
    // cooperating helper can mutate READY state while this validation/copy is in progress.
    let exact_appcds = if exact_clone {
        match prepare_exact_appcds_clone(&source, clone_source.source_uuid()) {
            Ok(candidate) => Some(candidate),
            Err(error) => {
                modal_action
                    .set_finished_with_error(format!("Unable to create exact clone from AppCDS state: {error}").into());
                return;
            },
        }
    } else {
        None
    };

    let dest = backend.directories.instances_dir.join(name);

    if let Err(err) = fs::create_dir(&dest) {
        modal_action.set_finished_with_error(format!("Unable to create instance directory: {err}").into());
        return;
    }

    // Mint the destination identity before copying any instance payload. The destination guard
    // remains held until the clone is verified and its reminted Ready manifest is committed.
    let clone_destination = match begin_profile_clone_destination(&dest, &clone_source) {
        Ok(destination) => destination,
        Err(error) => {
            let _ = fs::remove_dir_all(&dest);
            modal_action
                .set_finished_with_error(format!("Unable to initialize duplicated profile identity: {error}").into());
            return;
        },
    };

    let tracker = modal_action.push_tracker("Copying instance files...".into());

    let result = duplicate_with_content_library(
        &source,
        &dest,
        &backend.directories.content_library_dir,
        &|current, total| {
            tracker.set_count(current as usize);
            tracker.set_total(total as usize);
        },
        &|| {
            if modal_action.has_requested_cancel() {
                tracker.set_title("Cancelling...".into());
                Err(Error::new(ErrorKind::Interrupted, "Operation cancelled"))
            } else {
                Ok(())
            }
        },
    );

    let result = match result {
        Ok(()) => match clone_destination.finish() {
            Ok(destination_uuid) => {
                if let Some(candidate) = exact_appcds.as_ref() {
                    publish_exact_appcds_candidate(candidate, &dest, destination_uuid, &|| {
                        if modal_action.has_requested_cancel() {
                            Err(Error::new(ErrorKind::Interrupted, "Operation cancelled"))
                        } else {
                            Ok(())
                        }
                    })
                } else {
                    Ok(())
                }
            },
            Err(error) => Err(Error::new(ErrorKind::Other, error.to_string())),
        },
        Err(error) => Err(error),
    };

    match result {
        Ok(()) => {
            tracker.set_finished(ProgressTrackerFinishType::Normal);
        },
        Err(error) => {
            let _ = fs::remove_dir_all(&dest);
            if modal_action.has_requested_cancel() {
                tracker.set_finished(ProgressTrackerFinishType::Fast);
            } else {
                tracker.set_finished(ProgressTrackerFinishType::Error);
                modal_action.set_finished_with_error(error.to_string().into());
            }
        },
    }

    modal_action.set_finished();
}


fn prepare_exact_appcds_clone(
    source_root: &Path,
    source_uuid: Option<Uuid>,
) -> Result<ExactAppCdsCloneSource> {
    let source_profile_uuid = source_uuid.ok_or_else(|| {
        Error::new(ErrorKind::InvalidData, "exact AppCDS clone requires persistent profile identity")
    })?;
    let source_root = source_root.canonicalize()?;
    let cache_dir = source_root.join(".bootoptim").join("appcds");
    ensure_exact_clone_regular_dir(&cache_dir, &source_root)?;

    let namespace = read_exact_clone_regular_file(&cache_dir.join("profile.namespace"), &cache_dir)?;
    let expected_namespace = format!("schema=1\nprofile_uuid={source_profile_uuid}\n");
    if namespace != expected_namespace.as_bytes() {
        return Err(Error::new(ErrorKind::InvalidData, "AppCDS namespace does not match source profile"));
    }

    for name in ["training.meta", "training.jsa", "training.complete", "training.invalid"] {
        if cache_dir.join(name).exists() {
            return Err(Error::new(ErrorKind::InvalidData, "AppCDS training state is not cloneable READY"));
        }
    }
    for entry in fs::read_dir(&cache_dir)? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with("staging-") {
            return Err(Error::new(ErrorKind::InvalidData, "AppCDS staging state is not cloneable READY"));
        }
    }

    let plan_path = cache_dir.join("launch-plan.json");
    let source_plan = read_exact_clone_regular_file(&plan_path, &cache_dir)?;
    let source_plan_sha256 = sha256_bytes(&source_plan);
    let plan_sha_file = String::from_utf8(read_exact_clone_regular_file(
        &cache_dir.join("launch-plan.sha256"),
        &cache_dir,
    )?)
    .map_err(|_| Error::new(ErrorKind::InvalidData, "AppCDS plan hash is not UTF-8"))?;
    if plan_sha_file.trim() != source_plan_sha256 {
        return Err(Error::new(ErrorKind::InvalidData, "AppCDS launch plan hash mismatch"));
    }

    let ready_meta = String::from_utf8(read_exact_clone_regular_file(&cache_dir.join("ready.meta"), &cache_dir)?)
        .map_err(|_| Error::new(ErrorKind::InvalidData, "AppCDS ready metadata is not UTF-8"))?;
    let ready_plan = exact_meta_value(&ready_meta, "plan_sha256")?;
    let archive_sha256 = exact_meta_value(&ready_meta, "archive_sha256")?;
    let archive_size: u64 = exact_meta_value(&ready_meta, "archive_size")?
        .parse()
        .map_err(|_| Error::new(ErrorKind::InvalidData, "invalid AppCDS archive size"))?;
    if ready_plan != source_plan_sha256 {
        return Err(Error::new(ErrorKind::InvalidData, "AppCDS READY plan does not match source plan"));
    }

    let archive_path = cache_dir.join("ready.jsa");
    let archive_bytes = read_exact_clone_regular_file(&archive_path, &cache_dir)?;
    if archive_bytes.len() as u64 != archive_size || sha256_bytes(&archive_bytes) != archive_sha256 {
        return Err(Error::new(ErrorKind::InvalidData, "AppCDS READY archive hash/size mismatch"));
    }

    Ok(ExactAppCdsCloneSource {
        source_root,
        source_plan,
        source_plan_sha256,
        archive_path,
        archive_sha256,
        archive_size,
        source_profile_uuid,
    })
}

fn publish_exact_appcds_candidate(
    source: &ExactAppCdsCloneSource,
    destination_root: &Path,
    destination_uuid: Uuid,
    check_cancel: &dyn Fn() -> Result<()>,
) -> Result<()> {
    check_cancel()?;
    let destination_root = destination_root.canonicalize()?;
    let bootoptim = destination_root.join(".bootoptim");
    if !bootoptim.exists() {
        fs::create_dir(&bootoptim)?;
    }
    ensure_exact_clone_regular_dir(&bootoptim, &destination_root)?;

    let final_dir = bootoptim.join(EXACT_APPCDS_CANDIDATE_DIR);
    if final_dir.exists() {
        return Err(Error::new(ErrorKind::AlreadyExists, "exact AppCDS clone candidate already exists"));
    }
    let staging = bootoptim.join(format!("{EXACT_APPCDS_STAGING_PREFIX}{destination_uuid}"));
    fs::create_dir(&staging)?;

    let expected_plan = remap_exact_clone_plan(
        &source.source_plan,
        &source.source_root,
        &destination_root,
        source.source_profile_uuid,
        destination_uuid,
    )?;
    let expected_plan_sha256 = sha256_bytes(&expected_plan);

    let result = (|| {
        check_cancel()?;
        let archive_dest = staging.join("archive.jsa");
        fs::copy(&source.archive_path, &archive_dest)?;
        fs::OpenOptions::new().read(true).write(true).open(&archive_dest)?.sync_all()?;
        let copied = fs::read(&archive_dest)?;
        if copied.len() as u64 != source.archive_size || sha256_bytes(&copied) != source.archive_sha256 {
            return Err(Error::new(ErrorKind::InvalidData, "copied exact-clone archive failed verification"));
        }

        let meta = format!(
            "schema=1\nsource_profile_uuid={}\ndestination_profile_uuid={}\nsource_plan_sha256={}\nexpected_plan_sha256={}\narchive_sha256={}\narchive_size={}\n",
            source.source_profile_uuid,
            destination_uuid,
            source.source_plan_sha256,
            expected_plan_sha256,
            source.archive_sha256,
            source.archive_size,
        );
        write_new_synced_local(&staging.join("candidate.meta"), meta.as_bytes())?;
        write_new_synced_local(&staging.join("candidate.complete"), b"complete\n")?;
        fs::rename(&staging, &final_dir)?;
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

fn remap_exact_clone_plan(
    source_plan: &[u8],
    source_root: &Path,
    destination_root: &Path,
    source_uuid: Uuid,
    destination_uuid: Uuid,
) -> Result<Vec<u8>> {
    let mut text = String::from_utf8(source_plan.to_vec())
        .map_err(|_| Error::new(ErrorKind::InvalidData, "AppCDS launch plan is not UTF-8"))?;

    // UUID ownership is the only non-path identity field deliberately changed by
    // exact cloning. It must occur exactly once in the PR #48 plan.
    let source_uuid_field = format!("\"appcds_profile_uuid\": \"{source_uuid}\"");
    if text.matches(&source_uuid_field).count() != 1 {
        return Err(Error::new(ErrorKind::InvalidData, "ambiguous AppCDS profile UUID in source plan"));
    }
    text = text.replacen(
        &source_uuid_field,
        &format!("\"appcds_profile_uuid\": \"{destination_uuid}\""),
        1,
    );

    // Relocate only serialized artifact path objects that are actually inside
    // the cloned instance root. JVM argv fingerprints/safe literals are not
    // rewritten: if an effective argument changes because it embeds the old
    // root, the independently rebuilt destination plan will differ and the
    // inherited archive is rejected.
    let source_display = source_root.to_string_lossy();
    let destination_display = destination_root.to_string_lossy();
    let source_display_json = serde_json::to_string(source_display.as_ref())
        .map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;
    let destination_display_json = serde_json::to_string(destination_display.as_ref())
        .map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;
    let source_display_json = &source_display_json[1..source_display_json.len() - 1];
    let destination_display_json = &destination_display_json[1..destination_display_json.len() - 1];
    let source_hex = os_path_hex(source_root.as_os_str());
    let destination_hex = os_path_hex(destination_root.as_os_str());

    let mut replacements = Vec::<(usize, usize, String)>::new();
    let mut cursor = 0usize;
    while let Some(relative_key) = text[cursor..].find("\"path\"") {
        let key = cursor + relative_key;
        let after_key = key + "\"path\"".len();
        let Some(relative_colon) = text[after_key..].find(':') else {
            return Err(Error::new(ErrorKind::InvalidData, "malformed AppCDS path field"));
        };
        let after_colon = after_key + relative_colon + 1;
        let Some(relative_brace) = text[after_colon..].find('{') else {
            return Err(Error::new(ErrorKind::InvalidData, "malformed AppCDS path object"));
        };
        let object_start = after_colon + relative_brace;
        let object_end = find_json_object_end(&text, object_start)?;
        let raw = &text[object_start..object_end];
        let parsed: serde_json::Value = serde_json::from_str(raw)
            .map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;
        let display = parsed
            .get("display")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| Error::new(ErrorKind::InvalidData, "AppCDS path display missing"))?;
        let encoded = parsed
            .get("encoded_hex")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| Error::new(ErrorKind::InvalidData, "AppCDS encoded path missing"))?;

        if path_display_is_within(display, source_display.as_ref()) {
            if !encoded.starts_with(&source_hex) {
                return Err(Error::new(ErrorKind::InvalidData, "AppCDS path encodings disagree"));
            }
            let old_display = format!("\"display\":\"{source_display_json}");
            let new_display = format!("\"display\":\"{destination_display_json}");
            let old_encoded = format!("\"encoded_hex\":\"{source_hex}");
            let new_encoded = format!("\"encoded_hex\":\"{destination_hex}");
            let updated = raw
                .replacen(&old_display, &new_display, 1)
                .replacen(&old_encoded, &new_encoded, 1);
            if updated == raw {
                return Err(Error::new(ErrorKind::InvalidData, "AppCDS path relocation failed"));
            }
            replacements.push((object_start, object_end, updated));
        }
        cursor = object_end;
    }

    for (start, end, updated) in replacements.into_iter().rev() {
        text.replace_range(start..end, &updated);
    }
    Ok(text.into_bytes())
}

fn path_display_is_within(display: &str, root: &str) -> bool {
    if display == root {
        return true;
    }
    display
        .strip_prefix(root)
        .map(|suffix| suffix.starts_with('/') || suffix.starts_with('\\'))
        .unwrap_or(false)
}

fn find_json_object_end(text: &str, object_start: usize) -> Result<usize> {
    let bytes = text.as_bytes();
    if bytes.get(object_start) != Some(&b'{') {
        return Err(Error::new(ErrorKind::InvalidData, "AppCDS path object does not start with brace"));
    }
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for index in object_start..bytes.len() {
        let byte = bytes[index];
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(index + 1);
                }
            },
            _ => {}
        }
    }
    Err(Error::new(ErrorKind::InvalidData, "unterminated AppCDS path object"))
}

fn ensure_exact_clone_regular_dir(path: &Path, root: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err(Error::new(ErrorKind::InvalidData, "unsafe AppCDS clone directory"));
    }
    let canonical = path.canonicalize()?;
    let canonical_root = root.canonicalize()?;
    if !canonical.starts_with(&canonical_root) {
        return Err(Error::new(ErrorKind::InvalidData, "AppCDS clone directory escapes profile root"));
    }
    Ok(())
}

fn read_exact_clone_regular_file(path: &Path, root: &Path) -> Result<Vec<u8>> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err(Error::new(ErrorKind::InvalidData, "unsafe AppCDS clone file"));
    }
    let canonical = path.canonicalize()?;
    let canonical_root = root.canonicalize()?;
    if !canonical.starts_with(&canonical_root) {
        return Err(Error::new(ErrorKind::InvalidData, "AppCDS clone file escapes cache root"));
    }
    fs::read(path)
}

fn exact_meta_value(text: &str, key: &str) -> Result<String> {
    let prefix = format!("{key}=");
    let mut values = text.lines().filter_map(|line| line.strip_prefix(&prefix));
    let value = values
        .next()
        .ok_or_else(|| Error::new(ErrorKind::InvalidData, format!("missing AppCDS metadata field {key}")))?;
    if values.next().is_some() || value.is_empty() {
        return Err(Error::new(ErrorKind::InvalidData, format!("ambiguous AppCDS metadata field {key}")));
    }
    Ok(value.to_string())
}

fn sha256_bytes(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn write_new_synced_local(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = fs::OpenOptions::new().create_new(true).write(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn os_path_hex(value: &std::ffi::OsStr) -> String {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        return hex::encode(value.encode_wide().flat_map(u16::to_le_bytes).collect::<Vec<_>>());
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        return hex::encode(value.as_bytes());
    }
    #[cfg(not(any(windows, unix)))]
    {
        hex::encode(value.to_string_lossy().as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use sha2::Digest as Sha2Digest;

    use super::*;
    use crate::{
        profile_layout_flow::{
            DesiredManagedFile, PersistentProfileLayout, ProfileLayoutFlowError, ProfileLayoutState,
        },
        profile_layout_identity::ProfileCloneSource,
    };

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new(label: &str, minecraft: bool) -> Self {
            let root =
                std::env::temp_dir().join(format!("pandora-duplicate-{label}-{}", Uuid::from_bytes(rand::random())));
            fs::create_dir(&root).unwrap();
            if minecraft {
                fs::create_dir_all(root.join(".minecraft/mods")).unwrap();
            }
            Self(root)
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn desired(root: &Path, name: &str, bytes: &[u8]) -> DesiredManagedFile {
        let source = root.join(format!("source-{name}"));
        fs::write(&source, bytes).unwrap();
        DesiredManagedFile::new(
            format!("mods/{name}"),
            source,
            format!("identity-{name}"),
            hex::encode(Sha256::digest(bytes)),
        )
    }

    #[test]
    fn profile_layout_ready_duplicate_remints_namespace_and_cannot_use_original_journal() {
        let source = TestRoot::new("ready-source", true);
        let destination = TestRoot::new("ready-destination", false);
        let content_library = TestRoot::new("content-library", false);

        let mut original_layout = PersistentProfileLayout::open(&source.0).unwrap();
        original_layout.reconcile(&[desired(&source.0, "managed.jar", b"source-v1")]).unwrap();
        let original_uuid = original_layout.profile_uuid();
        drop(original_layout);

        let clone_source = prepare_profile_clone_source(&source.0).unwrap();
        assert!(matches!(clone_source, ProfileCloneSource::Ready { .. }));
        assert_eq!(clone_source.source_uuid(), Some(original_uuid));
        let clone_destination = begin_profile_clone_destination(&destination.0, &clone_source).unwrap();
        let clone_uuid = clone_destination.profile_uuid();
        assert_ne!(clone_uuid, original_uuid);

        duplicate_with_content_library(&source.0, &destination.0, &content_library.0, &|_, _| {}, &|| Ok(())).unwrap();
        clone_destination.finish().unwrap();
        drop(clone_source);

        assert!(!destination.0.join(CONTROL_DIR_NAME).join("journal.json").exists());
        assert!(!destination.0.join(CONTROL_DIR_NAME).join("backup").exists());
        assert!(!destination.0.join(CONTROL_DIR_NAME).join("staging").exists());
        let source_identity = fs::read(source.0.join(CONTROL_DIR_NAME).join("identity.json")).unwrap();
        let clone_identity = fs::read(destination.0.join(CONTROL_DIR_NAME).join("identity.json")).unwrap();
        assert_ne!(source_identity, clone_identity);

        let mut clone_layout = PersistentProfileLayout::open(&destination.0).unwrap();
        assert_eq!(clone_layout.profile_uuid(), clone_uuid);
        assert_eq!(clone_layout.status().state, ProfileLayoutState::Ready);
        clone_layout.reconcile(&[desired(&destination.0, "managed.jar", b"clone-v2")]).unwrap();
        drop(clone_layout);
        assert_eq!(fs::read(source.0.join(".minecraft/mods/managed.jar")).unwrap(), b"source-v1");
        assert_eq!(fs::read(destination.0.join(".minecraft/mods/managed.jar")).unwrap(), b"clone-v2");

        let forged_original_journal = serde_json::json!({
            "schema": 1,
            "profile_uuid": original_uuid,
            "transaction_id": Uuid::from_bytes(rand::random::<[u8; 16]>()),
            "from_generation": 2,
            "target_generation": 3,
            "previous_manifest_sha256": null,
            "target_manifest_sha256": "00",
            "state": "prepared",
            "operations": []
        });
        fs::write(
            destination.0.join(CONTROL_DIR_NAME).join("journal.json"),
            serde_json::to_vec_pretty(&forged_original_journal).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            PersistentProfileLayout::open(&destination.0),
            Err(ProfileLayoutFlowError::IdentityMismatch)
        ));
        assert_eq!(fs::read(source.0.join(".minecraft/mods/managed.jar")).unwrap(), b"source-v1");

        fs::remove_dir_all(&destination.0).unwrap();
        assert!(source.0.join(CONTROL_DIR_NAME).join("identity.json").is_file());
        assert_eq!(fs::read(source.0.join(".minecraft/mods/managed.jar")).unwrap(), b"source-v1");
    }
    #[test]
    fn duplicate_skips_appcds_cache_but_preserves_other_bootoptim_state() {
        let source = TestRoot::new("appcds-source", true);
        let destination = TestRoot::new("appcds-destination", false);
        let content_library = TestRoot::new("appcds-content-library", false);

        fs::create_dir_all(source.0.join(".bootoptim/appcds")).unwrap();
        fs::write(source.0.join(".bootoptim/appcds/ready.jsa"), b"source-only-appcds").unwrap();
        fs::create_dir_all(source.0.join(".bootoptim/appcds-unbound-v0-old")).unwrap();
        fs::write(
            source.0.join(".bootoptim/appcds-unbound-v0-old/ready.jsa"),
            b"quarantined-source-only-appcds",
        )
        .unwrap();
        fs::write(source.0.join(".bootoptim/keep.txt"), b"keep").unwrap();

        duplicate_with_content_library(
            &source.0,
            &destination.0,
            &content_library.0,
            &|_, _| {},
            &|| Ok(()),
        )
        .unwrap();

        assert!(!destination.0.join(".bootoptim/appcds").exists());
        assert!(!destination.0.join(".bootoptim/appcds-unbound-v0-old").exists());
        assert_eq!(fs::read(destination.0.join(".bootoptim/keep.txt")).unwrap(), b"keep");
        assert!(source.0.join(".bootoptim/appcds/ready.jsa").is_file());
    }

    fn write_ready_appcds_fixture(root: &Path, profile_uuid: Uuid, plan_suffix: &str) {
        let cache = root.join(".bootoptim/appcds");
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join("profile.namespace"), format!("schema=1\nprofile_uuid={profile_uuid}\n")).unwrap();
        let plan = format!(
            "{{\n  \"schema\": 1,\n  \"appcds_profile_uuid\": \"{profile_uuid}\",\n  \"mods\": [{{\"path\":{{\"display\":\"{}{}mods\\\\a.jar\",\"encoded_hex\":\"{}\"}}}}],\n  \"marker\": \"{plan_suffix}\"\n}}\n",
            root.to_string_lossy().replace('\\', "\\\\"),
            if cfg!(windows) { "\\\\" } else { "/" },
            os_path_hex(root.as_os_str()),
        );
        let plan_sha = sha256_bytes(plan.as_bytes());
        fs::write(cache.join("launch-plan.json"), plan.as_bytes()).unwrap();
        fs::write(cache.join("launch-plan.sha256"), format!("{plan_sha}\n")).unwrap();
        let archive = b"validated-ready-archive";
        let archive_sha = sha256_bytes(archive);
        fs::write(cache.join("ready.jsa"), archive).unwrap();
        fs::write(
            cache.join("ready.meta"),
            format!(
                "schema=1\nplan_sha256={plan_sha}\narchive_sha256={archive_sha}\narchive_size={}\nhelper_version=test\n",
                archive.len()
            ),
        )
        .unwrap();
    }

    #[test]
    fn exact_clone_candidate_remints_profile_uuid_and_requires_transformed_exact_plan() {
        let source = TestRoot::new("exact-source", false);
        let destination = TestRoot::new("exact-destination", false);
        let source_uuid = Uuid::from_bytes([1; 16]);
        let destination_uuid = Uuid::from_bytes([2; 16]);
        write_ready_appcds_fixture(&source.0, source_uuid, "unchanged");

        let candidate = prepare_exact_appcds_clone(&source.0, Some(source_uuid)).unwrap();
        publish_exact_appcds_candidate(&candidate, &destination.0, destination_uuid, &|| Ok(())).unwrap();

        let final_dir = destination.0.join(".bootoptim").join(EXACT_APPCDS_CANDIDATE_DIR);
        let meta = fs::read_to_string(final_dir.join("candidate.meta")).unwrap();
        assert!(meta.contains(&format!("source_profile_uuid={source_uuid}")));
        assert!(meta.contains(&format!("destination_profile_uuid={destination_uuid}")));
        assert!(!destination.0.join(".bootoptim/appcds/ready.jsa").exists());

        let expected_plan = remap_exact_clone_plan(
            &candidate.source_plan,
            &candidate.source_root,
            &destination.0.canonicalize().unwrap(),
            source_uuid,
            destination_uuid,
        )
        .unwrap();
        assert_eq!(
            exact_meta_value(&meta, "expected_plan_sha256").unwrap(),
            sha256_bytes(&expected_plan)
        );
    }

    #[test]
    fn exact_clone_rejects_corrupt_or_training_source_state() {
        let source = TestRoot::new("exact-corrupt", false);
        let source_uuid = Uuid::from_bytes([3; 16]);
        write_ready_appcds_fixture(&source.0, source_uuid, "unchanged");
        fs::write(source.0.join(".bootoptim/appcds/training.meta"), b"pending").unwrap();
        assert!(prepare_exact_appcds_clone(&source.0, Some(source_uuid)).is_err());
        fs::remove_file(source.0.join(".bootoptim/appcds/training.meta")).unwrap();
        fs::write(source.0.join(".bootoptim/appcds/ready.jsa"), b"tampered").unwrap();
        assert!(prepare_exact_appcds_clone(&source.0, Some(source_uuid)).is_err());
    }


}
