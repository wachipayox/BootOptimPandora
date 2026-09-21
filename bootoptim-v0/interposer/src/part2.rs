fn build_launch_plan(parsed: &ParsedArgs) -> io::Result<LaunchPlan> {
    let mut identity_cache = IdentityDigestCache::stock();
    build_launch_plan_with_cache(parsed, &mut identity_cache)
}

fn build_launch_plan_with_cache(
    parsed: &ParsedArgs,
    identity_cache: &mut IdentityDigestCache,
) -> io::Result<LaunchPlan> {
    let java_path = absolute_path(&parsed.instance_dir, Path::new(&parsed.java_exe));
    let java_hash = identity_cache.resolve_raw("java", &java_path).ok().map(|(_, digest)| digest);
    let java_root = java_path.parent().and_then(Path::parent).map(Path::to_path_buf);
    let release_path = java_root.as_ref().map(|p| p.join("release"));
    let release_hash = release_path.as_ref().and_then(|p| hash_file(p).ok());
    let release_values = release_path.as_ref().and_then(|p| parse_release(p).ok()).unwrap_or_default();

    let classpath_raw = find_classpath(&parsed.java_args);
    let mut classpath = Vec::new();
    let mut classpath_valid = classpath_raw.is_some();
    if let Some(raw) = classpath_raw.as_ref() {
        for entry in env::split_paths(raw) {
            let resolved = absolute_path(&parsed.instance_dir, &entry);
            match artifact_from_path_with_cache("classpath", &resolved, identity_cache) {
                Ok(a) => classpath.push(a),
                Err(_) => classpath_valid = false,
            }
        }
        if classpath.is_empty() {
            classpath_valid = false;
        }
    }

    let mut module_path = Vec::new();
    let mut module_path_valid = true;
    match find_module_path(&parsed.java_args) {
        Ok(Some(raw)) => {
            for entry in env::split_paths(&raw) {
                let resolved = absolute_path(&parsed.instance_dir, &entry);
                match artifact_from_path_with_cache("module-path", &resolved, identity_cache) {
                    Ok(a) => module_path.push(a),
                    Err(_) => module_path_valid = false,
                }
            }
            if module_path.is_empty() {
                module_path_valid = false;
            }
        }
        Ok(None) => {}
        Err(()) => module_path_valid = false,
    }

    let mut mods = Vec::new();
    let mods_dir = parsed.instance_dir.join("mods");
    let mut mods_valid = mods_dir.is_dir();
    if mods_dir.is_dir() {
        let mut paths = Vec::new();
        match fs::read_dir(&mods_dir) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.extension().and_then(OsStr::to_str).map(|s| s.eq_ignore_ascii_case("jar")).unwrap_or(false) {
                        paths.push(p);
                    }
                }
            }
            Err(_) => mods_valid = false,
        }
        // Mods are fingerprinted as an unordered set only. This sorting is for
        // canonical identity bytes and never changes Pandora/FML launch order.
        paths.sort_by_key(|p| encode_os(p.as_os_str()).encoded_hex);
        for p in paths {
            match artifact_from_path_with_cache("mod", &p, identity_cache) {
                Ok(a) => mods.push(a),
                Err(_) => mods_valid = false,
            }
        }
        if mods.is_empty() {
            mods_valid = false;
        }
    }

    // A local dynamic archive must also be invalidated when launch-affecting
    // pack configuration changes between training and consumption. This is a
    // strong local snapshot, not a trusted distribution/repair manifest.
    let (pack_inputs, pack_inputs_valid) = collect_pack_inputs_with_cache(&parsed.instance_dir, identity_cache);
    let resource_pack_selection_sha256 = resource_pack_selection_fingerprint(&parsed.instance_dir).ok();
    let pack_manifest_sha256 = if mods_valid && pack_inputs_valid {
        resource_pack_selection_sha256.as_deref().map(|selection| {
            pack_manifest_digest(&mods, &pack_inputs, selection)
        })
    } else {
        None
    };

    let helper_path = env::current_exe().ok();
    let helper_artifact = helper_path.as_ref().and_then(|p| artifact_from_path_with_cache("helper", p, identity_cache).ok());
    let launcher_artifact = parsed.launcher_exe.as_ref().and_then(|p| artifact_from_path_with_cache("launcher", p, identity_cache).ok());

    let argv = parsed.java_args.iter().enumerate().map(|(index, arg)| {
        let sensitive = is_sensitive_arg(arg);
        ArgFingerprint {
            index,
            kind: classify_arg(arg),
            sha256: if sensitive { "REDACTED".to_string() } else { sha256_hex(&os_bytes(arg)) },
            safe_literal: if sensitive { None } else { safe_literal(arg) },
        }
    }).collect::<Vec<_>>();

    // Hidden JVM injection would make the final command unknowable. Record only
    // presence (never contents) and fail closed for AppCDS when any is present.
    let injected_env = ["JAVA_TOOL_OPTIONS", "_JAVA_OPTIONS", "JDK_JAVA_OPTIONS"]
        .into_iter()
        .map(|k| (k.to_string(), if env::var_os(k).is_some() { "present".to_string() } else { "unset".to_string() }))
        .collect::<BTreeMap<_, _>>();
    let injected_env_present = injected_env.values().any(|v| v == "present");

    let has_agent = has_agent_configuration(&parsed.java_args);
    let eligible = java_hash.is_some()
        && release_hash.is_some()
        && classpath_valid
        && module_path_valid
        && mods_valid
        && pack_inputs_valid
        && pack_manifest_sha256.is_some()
        && resource_pack_selection_sha256.is_some()
        && helper_artifact.is_some()
        && launcher_artifact.is_some()
        && !has_agent
        && !has_conflicting_cds_configuration(&parsed.java_args)
        && !has_unsupported_module_configuration(&parsed.java_args)
        && !injected_env_present;

    let mut out = String::new();
    out.push_str("{\n");
    push_json_num(&mut out, "schema", SCHEMA_VERSION as u64, true);
    push_json_str(&mut out, "helper_version", HELPER_VERSION, true);
    push_json_str(&mut out, "pandora_upstream_commit", &parsed.upstream_commit, true);
    push_json_str(&mut out, "os", env::consts::OS, true);
    push_json_str(&mut out, "arch", env::consts::ARCH, true);
    push_json_bool(&mut out, "activation_eligible", eligible, true);
    out.push_str("  \"java\": {\n");
    push_json_os(&mut out, "path", &encode_os(java_path.as_os_str()), 4, true);
    push_json_opt_str(&mut out, "sha256", java_hash.as_deref(), 4, true);
    push_json_opt_str(&mut out, "release_sha256", release_hash.as_deref(), 4, true);
    push_json_str_indent(&mut out, "vendor", release_values.get("IMPLEMENTOR").map(String::as_str).unwrap_or(""), 4, true);
    push_json_str_indent(&mut out, "version", release_values.get("JAVA_VERSION").map(String::as_str).unwrap_or(""), 4, false);
    out.push_str("  },\n");
    out.push_str("  \"argv\": [\n");
    for (n, arg) in argv.iter().enumerate() {
        out.push_str("    {");
        out.push_str(&format!("\"index\":{},\"kind\":\"{}\",\"sha256\":\"{}\",\"safe_literal\":", arg.index, arg.kind, arg.sha256));
        if let Some(lit) = &arg.safe_literal { push_json_string_value(&mut out, lit); } else { out.push_str("null"); }
        out.push('}');
        if n + 1 != argv.len() { out.push(','); }
        out.push('\n');
    }
    out.push_str("  ],\n");
    push_artifact_array(&mut out, "classpath", &classpath, true);
    push_artifact_array(&mut out, "module_path", &module_path, true);
    push_artifact_array(&mut out, "mods", &mods, true);
    push_artifact_array(&mut out, "pack_inputs", &pack_inputs, true);
    push_json_opt_str(&mut out, "pack_manifest_sha256", pack_manifest_sha256.as_deref(), 2, true);
    push_json_opt_str(&mut out, "resource_pack_selection_sha256", resource_pack_selection_sha256.as_deref(), 2, true);
    let mut components = Vec::new();
    if let Some(a) = launcher_artifact { components.push(a); }
    if let Some(a) = helper_artifact { components.push(a); }
    push_artifact_array(&mut out, "components", &components, true);
    out.push_str("  \"injected_jvm_env_presence\": {\n");
    for (idx, (k, v)) in injected_env.iter().enumerate() {
        out.push_str("    "); push_json_string_value(&mut out, k); out.push(':'); push_json_string_value(&mut out, v);
        if idx + 1 != injected_env.len() { out.push(','); }
        out.push('\n');
    }
    out.push_str("  }\n");
    out.push_str("}\n");

    let bytes = out.into_bytes();
    let sha256 = sha256_hex(&bytes);
    Ok(LaunchPlan { bytes, sha256, eligible })
}

fn find_module_path(args: &[OsString]) -> Result<Option<OsString>, ()> {
    let mut found: Option<OsString> = None;
    let mut i = 0usize;
    while i < args.len() {
        let arg = &args[i];
        if arg == OsStr::new("--module-path") || arg == OsStr::new("-p") {
            let value = args.get(i + 1).cloned().ok_or(())?;
            if found.replace(value).is_some() {
                return Err(());
            }
            i += 2;
            continue;
        }
        if let Some(text) = arg.to_str() {
            if let Some(rest) = text.strip_prefix("--module-path=") {
                if rest.is_empty() || found.replace(OsString::from(rest)).is_some() {
                    return Err(());
                }
            }
        }
        i += 1;
    }
    Ok(found)
}

fn has_unsupported_module_configuration(args: &[OsString]) -> bool {
    args.iter().any(|arg| {
        let text = arg.to_string_lossy();
        text == "--upgrade-module-path"
            || text.starts_with("--upgrade-module-path=")
            || text == "--patch-module"
            || text.starts_with("--patch-module=")
            || text == "--limit-modules"
            || text.starts_with("--limit-modules=")
    })
}

fn collect_pack_inputs(instance_dir: &Path) -> (Vec<Artifact>, bool) {
    let mut identity_cache = IdentityDigestCache::stock();
    collect_pack_inputs_with_cache(instance_dir, &mut identity_cache)
}

fn collect_pack_inputs_with_cache(
    instance_dir: &Path,
    identity_cache: &mut IdentityDigestCache,
) -> (Vec<Artifact>, bool) {
    const ROOTS: &[&str] = &["config", "defaultconfigs", "kubejs", "scripts"];
    let mut paths = Vec::new();
    let mut valid = true;

    for root_name in ROOTS {
        let root = instance_dir.join(root_name);
        if !root.exists() {
            continue;
        }
        if !root.is_dir() {
            valid = false;
            continue;
        }
        if collect_regular_tree(&root, &mut paths).is_err() {
            valid = false;
        }
    }

    paths.sort_by_key(|p| encode_os(p.as_os_str()).encoded_hex);
    let mut artifacts = Vec::with_capacity(paths.len());
    for path in paths {
        match pack_input_artifact_with_cache(instance_dir, &path, identity_cache) {
            Ok(artifact) => artifacts.push(artifact),
            Err(_) => valid = false,
        }
    }
    (artifacts, valid)
}

fn collect_regular_tree(root: &Path, paths: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_symlink() || is_windows_reparse_point(&path)? {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "pack input symlink/reparse point"));
        }
        if file_type.is_dir() {
            collect_regular_tree(&path, paths)?;
        } else if file_type.is_file() {
            paths.push(path);
        } else {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "unsupported pack input type"));
        }
    }
    Ok(())
}

fn is_windows_reparse_point(path: &Path) -> io::Result<bool> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
        Ok(fs::symlink_metadata(path)?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Ok(false)
    }
}

fn resource_pack_selection_fingerprint(instance_dir: &Path) -> io::Result<String> {
    let options = fs::read_to_string(instance_dir.join("options.txt"))?;
    let mut values = BTreeMap::new();
    for line in options.lines() {
        if let Some((key, value)) = line.split_once(':') {
            if key == "resourcePacks" || key == "incompatibleResourcePacks" {
                values.insert(key.to_string(), value.to_string());
            }
        }
    }
    if !values.contains_key("resourcePacks") {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "options missing resourcePacks"));
    }
    let mut canonical = String::new();
    for key in ["resourcePacks", "incompatibleResourcePacks"] {
        if let Some(value) = values.get(key) {
            canonical.push_str(key);
            canonical.push('=');
            canonical.push_str(value);
            canonical.push('\n');
        }
    }
    Ok(sha256_hex(canonical.as_bytes()))
}

fn pack_manifest_digest(mods: &[Artifact], pack_inputs: &[Artifact], resource_selection_sha256: &str) -> String {
    let mut canonical = String::new();
    canonical.push_str("schema=1\n");
    for artifact in mods.iter().chain(pack_inputs.iter()) {
        canonical.push_str(artifact.role);
        canonical.push('|');
        canonical.push_str(&artifact.path.encoded_hex);
        canonical.push('|');
        canonical.push_str(&artifact.size.to_string());
        canonical.push('|');
        canonical.push_str(&artifact.sha256);
        canonical.push('\n');
    }
    canonical.push_str("resource_pack_selection_sha256=");
    canonical.push_str(resource_selection_sha256);
    canonical.push('\n');
    sha256_hex(canonical.as_bytes())
}

fn persist_plan_and_compare(cache_dir: &Path, bytes: &[u8], sha: &str) -> io::Result<bool> {
    let plan_path = cache_dir.join("launch-plan.json");
    let stable = fs::read(&plan_path).ok().map(|old| old == bytes).unwrap_or(false);
    write_atomic_replace(&plan_path, bytes)?;
    write_atomic_replace(&cache_dir.join("launch-plan.sha256"), format!("{}\n", sha).as_bytes())?;
    write_atomic_replace(&cache_dir.join("launch-plan.match"), if stable { b"MATCH\n" } else { b"FIRST_OR_MISMATCH\n" })?;
    Ok(stable)
}

fn classify_cache(cache_dir: &Path, plan_sha: &str) -> io::Result<(CacheState, Option<ReadyMetadata>)> {
    let ready = cache_dir.join("ready.jsa");
    let meta = cache_dir.join("ready.meta");
    if ready.is_file() || meta.is_file() {
        if !(ready.is_file() && meta.is_file()) {
            return Ok((CacheState::Stale, None));
        }
        let parsed = match read_metadata(&meta) {
            Ok(m) => m,
            Err(_) => return Ok((CacheState::Stale, None)),
        };
        if parsed.plan_sha256 != plan_sha || parsed.helper_version != HELPER_VERSION {
            return Ok((CacheState::Stale, Some(parsed)));
        }
        let size = fs::metadata(&ready)?.len();
        if size != parsed.archive_size {
            return Ok((CacheState::Stale, Some(parsed)));
        }
        let actual_hash = hash_file(&ready)?;
        if actual_hash != parsed.archive_sha256 {
            return Ok((CacheState::Stale, Some(parsed)));
        }
        return Ok((CacheState::Ready, Some(parsed)));
    }
    if has_staging(cache_dir)? {
        return Ok((CacheState::Failed, None));
    }
    Ok((CacheState::Absent, None))
}

fn promote_archive(cache_dir: &Path, staging: &Path, plan_sha: &str) -> io::Result<()> {
    let ready = cache_dir.join("ready.jsa");
    let meta = cache_dir.join("ready.meta");
    if ready.exists() || meta.exists() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, "ready target exists"));
    }
    let archive_size = fs::metadata(staging)?.len();
    let archive_sha256 = hash_file(staging)?;
    let md = ReadyMetadata {
        plan_sha256: plan_sha.to_string(),
        archive_sha256,
        archive_size,
        helper_version: HELPER_VERSION.to_string(),
    };
    let meta_staging = cache_dir.join(format!("staging-{}.meta", unique_suffix()));
    {
        let mut f = OpenOptions::new().create_new(true).write(true).open(&meta_staging)?;
        f.write_all(metadata_bytes(&md).as_bytes())?;
        f.sync_all()?;
    }
    fs::rename(staging, &ready)?;
    if let Err(e) = fs::rename(&meta_staging, &meta) {
        let _ = fs::remove_file(&ready);
        let _ = fs::remove_file(&meta_staging);
        return Err(e);
    }
    Ok(())
}

fn cleanup_orphan_staging(cache_dir: &Path) -> io::Result<()> {
    for entry in fs::read_dir(cache_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("staging-") && (name.ends_with(".jsa") || name.ends_with(".meta")) {
            let _ = fs::remove_file(entry.path());
        }
    }
    Ok(())
}

fn has_staging(cache_dir: &Path) -> io::Result<bool> {
    for entry in fs::read_dir(cache_dir)? {
        let name = entry?.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("staging-") && (name.ends_with(".jsa") || name.ends_with(".meta")) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn write_state(cache_dir: &Path, state: CacheState, reason: &str) -> io::Result<()> {
    let text = format!("schema={}\nstate={}\nreason={}\nhelper_version={}\n", SCHEMA_VERSION, state.as_str(), reason, HELPER_VERSION);
    write_atomic_replace(&cache_dir.join("state.meta"), text.as_bytes())
}

fn metadata_bytes(md: &ReadyMetadata) -> String {
    format!(
        "schema={}\nplan_sha256={}\narchive_sha256={}\narchive_size={}\nhelper_version={}\n",
        SCHEMA_VERSION, md.plan_sha256, md.archive_sha256, md.archive_size, md.helper_version
    )
}

fn read_metadata(path: &Path) -> io::Result<ReadyMetadata> {
    let text = fs::read_to_string(path)?;
    let mut map = BTreeMap::new();
    for line in text.lines() {
        if let Some((k, v)) = line.split_once('=') {
            map.insert(k, v);
        }
    }
    if map.get("schema") != Some(&"1") {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "schema"));
    }
    Ok(ReadyMetadata {
        plan_sha256: map.get("plan_sha256").ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "plan"))?.to_string(),
        archive_sha256: map.get("archive_sha256").ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "archive"))?.to_string(),
        archive_size: map.get("archive_size").ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "size"))?.parse().map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "size"))?,
        helper_version: map.get("helper_version").ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "helper"))?.to_string(),
    })
}
