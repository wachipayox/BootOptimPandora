use std::time::Instant;

const PREFLIGHT_PROBE_SCHEMA: &str = "bootoptim.appcds_preflight_probe.v1";
const PREFLIGHT_PROBE_ENV: &str = "BOOTOPTIM_APPCDS_PREFLIGHT_PROBE";

#[derive(Debug, Default, Clone, Copy)]
struct ProbeInventory {
    classpath_files: usize,
    classpath_bytes: u64,
    module_path_files: usize,
    module_path_bytes: u64,
    mod_files: usize,
    mod_bytes: u64,
    pack_input_files: usize,
    pack_input_bytes: u64,
    component_files: usize,
    component_bytes: u64,
}

#[derive(Debug)]
struct PreflightProbe {
    path: Option<PathBuf>,
    started: Option<Instant>,
    mode: &'static str,
    top_stages: Vec<(&'static str, u128)>,
    build_stages: Vec<(&'static str, u128)>,
    inventory: ProbeInventory,
}

impl PreflightProbe {
    fn from_env() -> Self {
        let path = env::var_os(PREFLIGHT_PROBE_ENV)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        let started = path.as_ref().map(|_| Instant::now());
        Self {
            path,
            started,
            mode: "unknown",
            top_stages: Vec::new(),
            build_stages: Vec::new(),
            inventory: ProbeInventory::default(),
        }
    }

    #[cfg(test)]
    fn for_test(path: PathBuf) -> Self {
        Self {
            path: Some(path),
            started: Some(Instant::now()),
            mode: "plan",
            top_stages: Vec::new(),
            build_stages: Vec::new(),
            inventory: ProbeInventory::default(),
        }
    }

    fn enabled(&self) -> bool {
        self.path.is_some()
    }

    fn set_mode(&mut self, mode: Mode) {
        self.mode = match mode {
            Mode::Plan => "plan",
            Mode::Auto => "auto",
        };
    }

    fn stage_start(&self) -> Option<Instant> {
        if self.enabled() { Some(Instant::now()) } else { None }
    }

    fn record_top(&mut self, name: &'static str, started: Option<Instant>) {
        if let Some(started) = started {
            self.top_stages.push((name, started.elapsed().as_nanos()));
        }
    }

    fn record_build(&mut self, name: &'static str, started: Option<Instant>) {
        if let Some(started) = started {
            self.build_stages.push((name, started.elapsed().as_nanos()));
        }
    }

    fn finish(&mut self, result: &io::Result<PrepareDecision>) {
        let Some(path) = self.path.as_ref() else { return; };
        let Some(started) = self.started else { return; };
        let decision = match result {
            Ok(PrepareDecision::Stock) => "STOCK",
            Ok(PrepareDecision::Train) => "TRAIN",
            Ok(PrepareDecision::Ready) => "READY",
            Err(_) => "ERROR",
        };

        let mut out = String::new();
        out.push_str("{\"schema\":\"");
        out.push_str(PREFLIGHT_PROBE_SCHEMA);
        out.push_str("\",\"mode\":\"");
        out.push_str(self.mode);
        out.push_str("\",\"decision\":\"");
        out.push_str(decision);
        out.push_str("\",\"total_ns\":");
        out.push_str(&started.elapsed().as_nanos().to_string());
        out.push_str(",\"top_ns\":{");
        push_probe_stages(&mut out, &self.top_stages);
        out.push_str("},\"build_plan_ns\":{");
        push_probe_stages(&mut out, &self.build_stages);
        out.push_str("},\"inventory\":{");
        push_probe_inventory(&mut out, self.inventory);
        out.push_str("}}\n");

        // Diagnostic output is best-effort and must never affect STOCK/TRAIN/READY.
        // create_new also makes an accidentally reused sidecar path fail closed for
        // measurement without overwriting evidence from another launch.
        if let Ok(mut file) = OpenOptions::new().create_new(true).write(true).open(path) {
            let _ = file.write_all(out.as_bytes());
            let _ = file.flush();
        }
    }
}

fn push_probe_stages(out: &mut String, stages: &[(&'static str, u128)]) {
    for (index, (name, duration_ns)) in stages.iter().enumerate() {
        if index != 0 { out.push(','); }
        out.push('"');
        out.push_str(name);
        out.push_str("\":");
        out.push_str(&duration_ns.to_string());
    }
}

fn push_probe_inventory(out: &mut String, inventory: ProbeInventory) {
    out.push_str("\"classpath\":{\"files\":");
    out.push_str(&inventory.classpath_files.to_string());
    out.push_str(",\"bytes\":");
    out.push_str(&inventory.classpath_bytes.to_string());
    out.push_str("},\"module_path\":{\"files\":");
    out.push_str(&inventory.module_path_files.to_string());
    out.push_str(",\"bytes\":");
    out.push_str(&inventory.module_path_bytes.to_string());
    out.push_str("},\"mods\":{\"files\":");
    out.push_str(&inventory.mod_files.to_string());
    out.push_str(",\"bytes\":");
    out.push_str(&inventory.mod_bytes.to_string());
    out.push_str("},\"pack_inputs\":{\"files\":");
    out.push_str(&inventory.pack_input_files.to_string());
    out.push_str(",\"bytes\":");
    out.push_str(&inventory.pack_input_bytes.to_string());
    out.push_str("},\"components\":{\"files\":");
    out.push_str(&inventory.component_files.to_string());
    out.push_str(",\"bytes\":");
    out.push_str(&inventory.component_bytes.to_string());
    out.push('}');
}

fn probe_artifact_totals(artifacts: &[Artifact]) -> (usize, u64) {
    (artifacts.len(), artifacts.iter().map(|artifact| artifact.size).sum())
}

// This is intentionally a probe-only mirror of build_launch_plan. Disabled
// launches call the original function. Tests require the profiled and stock
// plan bytes to remain identical, so instrumentation cannot silently become a
// second identity contract.
fn build_launch_plan_profiled(parsed: &ParsedArgs, probe: &mut PreflightProbe) -> io::Result<LaunchPlan> {
    let started = probe.stage_start();
    let java_path = absolute_path(&parsed.instance_dir, Path::new(&parsed.java_exe));
    let java_hash = hash_file(&java_path).ok();
    let java_root = java_path.parent().and_then(Path::parent).map(Path::to_path_buf);
    let release_path = java_root.as_ref().map(|p| p.join("release"));
    let release_hash = release_path.as_ref().and_then(|p| hash_file(p).ok());
    let release_values = release_path.as_ref().and_then(|p| parse_release(p).ok()).unwrap_or_default();
    probe.record_build("java_identity", started);

    let started = probe.stage_start();
    let classpath_raw = find_classpath(&parsed.java_args);
    let mut classpath = Vec::new();
    let mut classpath_valid = classpath_raw.is_some();
    if let Some(raw) = classpath_raw.as_ref() {
        for entry in env::split_paths(raw) {
            let resolved = absolute_path(&parsed.instance_dir, &entry);
            match artifact_from_path("classpath", &resolved) {
                Ok(a) => classpath.push(a),
                Err(_) => classpath_valid = false,
            }
        }
        if classpath.is_empty() {
            classpath_valid = false;
        }
    }
    probe.record_build("classpath", started);

    let started = probe.stage_start();
    let mut module_path = Vec::new();
    let mut module_path_valid = true;
    match find_module_path(&parsed.java_args) {
        Ok(Some(raw)) => {
            for entry in env::split_paths(&raw) {
                let resolved = absolute_path(&parsed.instance_dir, &entry);
                match artifact_from_path("module-path", &resolved) {
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
    probe.record_build("module_path", started);

    let started = probe.stage_start();
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
        paths.sort_by_key(|p| encode_os(p.as_os_str()).encoded_hex);
        for p in paths {
            match artifact_from_path("mod", &p) {
                Ok(a) => mods.push(a),
                Err(_) => mods_valid = false,
            }
        }
        if mods.is_empty() {
            mods_valid = false;
        }
    }
    probe.record_build("mods", started);

    let started = probe.stage_start();
    let (pack_inputs, pack_inputs_valid) = collect_pack_inputs(&parsed.instance_dir);
    probe.record_build("pack_inputs", started);

    let started = probe.stage_start();
    let resource_pack_selection_sha256 = resource_pack_selection_fingerprint(&parsed.instance_dir).ok();
    let pack_manifest_sha256 = if mods_valid && pack_inputs_valid {
        resource_pack_selection_sha256.as_deref().map(|selection| {
            pack_manifest_digest(&mods, &pack_inputs, selection)
        })
    } else {
        None
    };
    probe.record_build("resource_pack_manifest", started);

    let started = probe.stage_start();
    let helper_path = env::current_exe().ok();
    let helper_artifact = helper_path.as_ref().and_then(|p| artifact_from_path("helper", p).ok());
    let launcher_artifact = parsed.launcher_exe.as_ref().and_then(|p| artifact_from_path("launcher", p).ok());
    probe.record_build("components", started);

    let started = probe.stage_start();
    let argv = parsed.java_args.iter().enumerate().map(|(index, arg)| {
        let sensitive = is_sensitive_arg(arg);
        ArgFingerprint {
            index,
            kind: classify_arg(arg),
            sha256: if sensitive { "REDACTED".to_string() } else { sha256_hex(&os_bytes(arg)) },
            safe_literal: if sensitive { None } else { safe_literal(arg) },
        }
    }).collect::<Vec<_>>();

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
    probe.record_build("argv_and_eligibility", started);

    let (classpath_files, classpath_bytes) = probe_artifact_totals(&classpath);
    let (module_path_files, module_path_bytes) = probe_artifact_totals(&module_path);
    let (mod_files, mod_bytes) = probe_artifact_totals(&mods);
    let (pack_input_files, pack_input_bytes) = probe_artifact_totals(&pack_inputs);
    let mut component_files = 0usize;
    let mut component_bytes = 0u64;
    if let Some(artifact) = helper_artifact.as_ref() {
        component_files += 1;
        component_bytes = component_bytes.saturating_add(artifact.size);
    }
    if let Some(artifact) = launcher_artifact.as_ref() {
        component_files += 1;
        component_bytes = component_bytes.saturating_add(artifact.size);
    }
    probe.inventory = ProbeInventory {
        classpath_files,
        classpath_bytes,
        module_path_files,
        module_path_bytes,
        mod_files,
        mod_bytes,
        pack_input_files,
        pack_input_bytes,
        component_files,
        component_bytes,
    };

    let started = probe.stage_start();
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
    probe.record_build("serialize_plan", started);
    Ok(LaunchPlan { bytes, sha256, eligible })
}

#[cfg(test)]
mod preflight_probe_tests {
    use super::*;

    fn probe_temp_dir(label: &str) -> PathBuf {
        let path = env::temp_dir().join(format!("bootoptim-preflight-probe-{label}-{}", unique_suffix()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn probe_fixture() -> (PathBuf, ParsedArgs) {
        let d = probe_temp_dir("plan-equivalence");
        let java_root = d.join("runtime");
        fs::create_dir_all(java_root.join("bin")).unwrap();
        let java = java_root.join(if cfg!(windows) { "bin/javaw.exe" } else { "bin/java" });
        fs::write(&java, b"fake-java-v1").unwrap();
        fs::write(java_root.join("release"), b"JAVA_VERSION=\"25.0.4\"\nIMPLEMENTOR=\"Oracle Corporation\"\n").unwrap();
        let lib = d.join("library.jar");
        fs::write(&lib, b"library-v1").unwrap();
        let module = d.join("module.jar");
        fs::write(&module, b"module-v1").unwrap();
        fs::create_dir_all(d.join("mods")).unwrap();
        fs::write(d.join("mods/mod.jar"), b"mod-v1").unwrap();
        fs::create_dir_all(d.join("config")).unwrap();
        fs::write(d.join("config/fml.toml"), b"earlyWindowControl=true\n").unwrap();
        fs::write(d.join("options.txt"), b"resourcePacks:[\"vanilla\"]\nincompatibleResourcePacks:[]\n").unwrap();
        let launcher = d.join("Pandora.exe");
        fs::write(&launcher, b"launcher").unwrap();
        let parsed = ParsedArgs {
            instance_dir: d.clone(),
            launcher_exe: Some(launcher),
            upstream_commit: UPSTREAM_DEFAULT.to_string(),
            java_exe: java.into_os_string(),
            java_args: vec![
                OsString::from("-cp"),
                env::join_paths([lib]).unwrap(),
                OsString::from("--module-path"),
                env::join_paths([module]).unwrap(),
                OsString::from("com.moulberry.pandora.LaunchWrapper"),
            ],
        };
        (d, parsed)
    }

    #[test]
    fn profiled_plan_is_byte_identical_to_stock_plan() {
        let (d, parsed) = probe_fixture();
        let stock = build_launch_plan(&parsed).unwrap();
        let mut probe = PreflightProbe::for_test(d.join("probe.json"));
        let profiled = build_launch_plan_profiled(&parsed, &mut probe).unwrap();
        assert_eq!(stock.bytes, profiled.bytes);
        assert_eq!(stock.sha256, profiled.sha256);
        assert_eq!(stock.eligible, profiled.eligible);
        assert!(probe.inventory.classpath_files > 0);
        assert!(probe.inventory.mod_files > 0);
        assert!(probe.build_stages.iter().any(|(name, _)| *name == "pack_inputs"));
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn sidecar_is_aggregate_and_create_new() {
        let d = probe_temp_dir("sidecar");
        let path = d.join("probe.json");
        let mut probe = PreflightProbe::for_test(path.clone());
        probe.inventory.classpath_files = 2;
        probe.inventory.classpath_bytes = 123;
        probe.finish(&Ok(PrepareDecision::Stock));
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains(PREFLIGHT_PROBE_SCHEMA));
        assert!(text.contains("\"decision\":\"STOCK\""));
        assert!(text.contains("\"classpath\":{\"files\":2,\"bytes\":123}"));

        fs::write(&path, b"sentinel\n").unwrap();
        let mut second = PreflightProbe::for_test(path.clone());
        second.finish(&Ok(PrepareDecision::Ready));
        assert_eq!(fs::read(&path).unwrap(), b"sentinel\n");
        let _ = fs::remove_dir_all(d);
    }
}
