const EXACT_CLONE_CANDIDATE_DIR: &str = "appcds-exact-clone-candidate";
const EXACT_CLONE_REJECTED_PREFIX: &str = "appcds-exact-clone-rejected-";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExactCloneAdoption {
    None,
    Adopted,
    Rejected,
}

fn try_adopt_exact_clone_candidate(
    instance_dir: &Path,
    cache_dir: &Path,
    namespace: &AppCdsProfileNamespace,
    plan: &LaunchPlan,
) -> io::Result<ExactCloneAdoption> {
    let Some(profile_uuid) = namespace.profile_uuid() else {
        return Ok(ExactCloneAdoption::None);
    };
    let bootoptim = instance_dir.join(".bootoptim");
    let candidate = bootoptim.join(EXACT_CLONE_CANDIDATE_DIR);
    match fs::symlink_metadata(&candidate) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(ExactCloneAdoption::None),
        Err(error) => return Err(error),
        Ok(meta) => {
            if ensure_plain_profile_directory(&candidate, &meta).is_err() {
                return reject_exact_clone_candidate(&bootoptim, &candidate);
            }
        }
    }

    let validation = validate_exact_clone_candidate(&candidate, cache_dir, profile_uuid, plan);
    let archive = match validation {
        Ok(path) => path,
        Err(_) => return reject_exact_clone_candidate(&bootoptim, &candidate),
    };

    // Never overwrite or reinterpret any local cache/training state. The normal
    // state machine remains authoritative once state exists.
    for name in [
        "ready.jsa",
        "ready.meta",
        "training.meta",
        "training.jsa",
        "training.complete",
        "training.invalid",
    ] {
        if cache_dir.join(name).exists() {
            return reject_exact_clone_candidate(&bootoptim, &candidate);
        }
    }
    if has_staging(cache_dir)? {
        return reject_exact_clone_candidate(&bootoptim, &candidate);
    }

    promote_archive(cache_dir, &archive, &plan.sha256)?;
    fs::remove_dir_all(&candidate)?;
    Ok(ExactCloneAdoption::Adopted)
}

fn validate_exact_clone_candidate(
    candidate: &Path,
    cache_dir: &Path,
    profile_uuid: &str,
    plan: &LaunchPlan,
) -> io::Result<PathBuf> {
    let complete = read_plain_profile_text(&candidate.join("candidate.complete"))?;
    if complete != "complete\n" {
        return Err(invalid_exact_clone_candidate("candidate is incomplete"));
    }

    let meta = read_plain_profile_text(&candidate.join("candidate.meta"))?;
    if exact_clone_meta_value(&meta, "schema")? != "1"
        || exact_clone_meta_value(&meta, "destination_profile_uuid")? != profile_uuid
        || exact_clone_meta_value(&meta, "expected_plan_sha256")? != plan.sha256
    {
        return Err(invalid_exact_clone_candidate("candidate identity mismatch"));
    }
    let archive_sha256 = exact_clone_meta_value(&meta, "archive_sha256")?;
    let archive_size: u64 = exact_clone_meta_value(&meta, "archive_size")?
        .parse()
        .map_err(|_| invalid_exact_clone_candidate("invalid candidate archive size"))?;

    let archive = candidate.join("archive.jsa");
    let archive_meta = fs::symlink_metadata(&archive)?;
    if !archive_meta.is_file()
        || archive_meta.file_type().is_symlink()
        || is_windows_reparse_point(&archive)?
        || archive_meta.len() != archive_size
    {
        return Err(invalid_exact_clone_candidate("unsafe candidate archive"));
    }
    let canonical_candidate = candidate.canonicalize()?;
    let canonical_archive = archive.canonicalize()?;
    if !canonical_archive.starts_with(&canonical_candidate) {
        return Err(invalid_exact_clone_candidate("candidate archive escapes namespace"));
    }
    if hash_file(&archive)? != archive_sha256 {
        return Err(invalid_exact_clone_candidate("candidate archive hash mismatch"));
    }

    // profile.namespace is written by PR #48 before this function runs. Recheck
    // it here so candidate promotion cannot bypass the cache/profile binding.
    let binding = read_plain_profile_text(&cache_dir.join("profile.namespace"))?;
    if binding != format!("schema=1\nprofile_uuid={profile_uuid}\n") {
        return Err(invalid_exact_clone_candidate("cache binding changed"));
    }
    Ok(archive)
}

fn exact_clone_meta_value(text: &str, key: &str) -> io::Result<String> {
    let prefix = format!("{key}=");
    let mut values = text.lines().filter_map(|line| line.strip_prefix(&prefix));
    let value = values
        .next()
        .ok_or_else(|| invalid_exact_clone_candidate("missing candidate metadata"))?;
    if values.next().is_some() || value.is_empty() {
        return Err(invalid_exact_clone_candidate("ambiguous candidate metadata"));
    }
    Ok(value.to_string())
}

fn reject_exact_clone_candidate(
    bootoptim: &Path,
    candidate: &Path,
) -> io::Result<ExactCloneAdoption> {
    let rejected = bootoptim.join(format!("{EXACT_CLONE_REJECTED_PREFIX}{}", unique_suffix()));
    match fs::rename(candidate, rejected) {
        Ok(()) => Ok(ExactCloneAdoption::Rejected),
        Err(error) => {
            // Persistence uncertainty is itself fail-closed. Propagate the error;
            // Pandora's existing helper-error path launches stock and preserves
            // the candidate evidence for inspection.
            Err(error)
        }
    }
}

fn invalid_exact_clone_candidate(reason: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}
