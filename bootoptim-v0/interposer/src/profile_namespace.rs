#[derive(Debug, Clone, PartialEq, Eq)]
enum AppCdsProfileNamespace {
    LegacyInstanceLocal,
    PersistentProfile(String),
}

impl AppCdsProfileNamespace {
    fn profile_uuid(&self) -> Option<&str> {
        match self {
            Self::LegacyInstanceLocal => None,
            Self::PersistentProfile(uuid) => Some(uuid.as_str()),
        }
    }
}

struct AppCdsProfileScope {
    namespace: AppCdsProfileNamespace,
    #[cfg(windows)]
    _profile_lock: Option<File>,
}

impl AppCdsProfileScope {
    fn namespace(&self) -> &AppCdsProfileNamespace {
        &self.namespace
    }
}

fn acquire_appcds_profile_scope(instance_dir: &Path) -> io::Result<AppCdsProfileScope> {
    let control = instance_dir.join(".pandora-layout-v1");
    let control_meta = match fs::symlink_metadata(&control) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(AppCdsProfileScope {
                namespace: AppCdsProfileNamespace::LegacyInstanceLocal,
                #[cfg(windows)]
                _profile_lock: None,
            });
        }
        Err(error) => return Err(error),
        Ok(meta) => meta,
    };
    ensure_plain_profile_directory(&control, &control_meta)?;

    let identity = read_plain_profile_text(&control.join("identity.json"))?;
    if unique_json_u64(&identity, "schema")? != 1 {
        return Err(invalid_profile_namespace("identity schema"));
    }
    let profile_uuid = unique_json_string(&identity, "profile_uuid")?;
    if !is_canonical_profile_uuid(&profile_uuid) {
        return Err(invalid_profile_namespace("profile uuid"));
    }

    #[cfg(windows)]
    let profile_lock = Some(acquire_existing_profile_lock_lease(&control, &profile_uuid)?);

    validate_ready_profile_control(&control, &profile_uuid)?;

    Ok(AppCdsProfileScope {
        namespace: AppCdsProfileNamespace::PersistentProfile(profile_uuid),
        #[cfg(windows)]
        _profile_lock: profile_lock,
    })
}

fn validate_ready_profile_control(control: &Path, profile_uuid: &str) -> io::Result<()> {
    for evidence in ["journal.json", "publishing.marker"] {
        match fs::symlink_metadata(control.join(evidence)) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
            Ok(_) => return Err(invalid_profile_namespace("active transaction evidence")),
        }
    }
    for directory in ["staging", "backup", "conflicts"] {
        let path = control.join(directory);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
            Ok(meta) => {
                ensure_plain_profile_directory(&path, &meta)?;
                if fs::read_dir(&path)?.next().transpose()?.is_some() {
                    return Err(invalid_profile_namespace("unresolved profile evidence"));
                }
            }
        }
    }

    let manifest = read_plain_profile_text(&control.join("manifest.json"))?;
    if unique_json_u64(&manifest, "schema")? != 1
        || unique_json_string(&manifest, "profile_uuid")? != profile_uuid
        || unique_json_string(&manifest, "state")? != "ready"
        || unique_json_u64(&manifest, "generation")? == 0
        || !unique_json_null(&manifest, "transaction_id")?
    {
        return Err(invalid_profile_namespace("profile manifest is not committed Ready"));
    }
    Ok(())
}

fn bind_appcds_cache_namespace(cache_dir: &Path, namespace: &AppCdsProfileNamespace) -> io::Result<()> {
    let Some(profile_uuid) = namespace.profile_uuid() else {
        fs::create_dir_all(cache_dir)?;
        return Ok(());
    };

    let parent = cache_dir
        .parent()
        .ok_or_else(|| invalid_profile_namespace("cache parent"))?;
    ensure_plain_profile_directory_path(parent)?;

    match fs::symlink_metadata(cache_dir) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
        Ok(meta) => {
            ensure_plain_profile_directory(cache_dir, &meta)?;
            let marker = cache_dir.join("profile.namespace");
            match fs::symlink_metadata(&marker) {
                Ok(_) => {
                    let text = read_plain_profile_text(&marker)?;
                    if text != format!("schema=1\nprofile_uuid={profile_uuid}\n") {
                        return Err(invalid_profile_namespace("AppCDS cache belongs to another profile"));
                    }
                    return Ok(());
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    if fs::read_dir(cache_dir)?.next().transpose()?.is_some() {
                        let quarantine = parent.join(format!("appcds-unbound-v0-{}", unique_suffix()));
                        fs::rename(cache_dir, quarantine)?;
                    }
                }
                Err(error) => return Err(error),
            }
        }
    }

    fs::create_dir_all(cache_dir)?;
    write_atomic_replace(
        &cache_dir.join("profile.namespace"),
        format!("schema=1\nprofile_uuid={profile_uuid}\n").as_bytes(),
    )
}

fn ensure_plain_profile_directory_path(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(path)?;
            let meta = fs::symlink_metadata(path)?;
            ensure_plain_profile_directory(path, &meta)
        }
        Err(error) => Err(error),
        Ok(meta) => ensure_plain_profile_directory(path, &meta),
    }
}

fn ensure_plain_profile_directory(path: &Path, meta: &fs::Metadata) -> io::Result<()> {
    if !meta.is_dir() || meta.file_type().is_symlink() || is_windows_reparse_point(path)? {
        return Err(invalid_profile_namespace("unsafe profile directory"));
    }
    Ok(())
}

fn read_plain_profile_text(path: &Path) -> io::Result<String> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.file_type().is_symlink() || is_windows_reparse_point(path)? {
        return Err(invalid_profile_namespace("unsafe profile file"));
    }
    fs::read_to_string(path)
}

fn unique_json_value_after_key<'a>(text: &'a str, key: &str) -> io::Result<&'a str> {
    let needle = format!("\"{}\"", key);
    let mut matches = text.match_indices(&needle);
    let (index, _) = matches
        .next()
        .ok_or_else(|| invalid_profile_namespace("missing profile json field"))?;
    if matches.next().is_some() {
        return Err(invalid_profile_namespace("duplicate profile json field"));
    }
    let tail = text[index + needle.len()..].trim_start();
    let tail = tail
        .strip_prefix(':')
        .ok_or_else(|| invalid_profile_namespace("malformed profile json field"))?;
    Ok(tail.trim_start())
}

fn unique_json_string(text: &str, key: &str) -> io::Result<String> {
    let value = unique_json_value_after_key(text, key)?;
    let rest = value
        .strip_prefix('"')
        .ok_or_else(|| invalid_profile_namespace("profile json string"))?;
    let end = rest
        .find('"')
        .ok_or_else(|| invalid_profile_namespace("unterminated profile json string"))?;
    let result = &rest[..end];
    if result.contains('\\') {
        return Err(invalid_profile_namespace("escaped profile json string"));
    }
    Ok(result.to_string())
}

fn unique_json_u64(text: &str, key: &str) -> io::Result<u64> {
    let value = unique_json_value_after_key(text, key)?;
    let digits = value
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        return Err(invalid_profile_namespace("profile json integer"));
    }
    digits
        .parse()
        .map_err(|_| invalid_profile_namespace("profile json integer overflow"))
}

fn unique_json_null(text: &str, key: &str) -> io::Result<bool> {
    Ok(unique_json_value_after_key(text, key)?.starts_with("null"))
}

fn is_canonical_profile_uuid(value: &str) -> bool {
    if value.len() != 36 {
        return false;
    }
    value.bytes().enumerate().all(|(index, byte)| {
        if [8, 13, 18, 23].contains(&index) {
            byte == b'-'
        } else {
            byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
        }
    })
}

fn invalid_profile_namespace(reason: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}

#[cfg(windows)]
fn acquire_existing_profile_lock_lease(control: &Path, profile_uuid: &str) -> io::Result<File> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};

    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;

    let locks_dir = control.join("locks");
    let locks_meta = fs::symlink_metadata(&locks_dir)?;
    ensure_plain_profile_directory(&locks_dir, &locks_meta)?;
    let lock_path = locks_dir.join(format!("{profile_uuid}.lock"));
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(&lock_path)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(invalid_profile_namespace("unsafe profile lock file"));
    }
    Ok(file)
}
