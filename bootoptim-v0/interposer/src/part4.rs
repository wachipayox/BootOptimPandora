#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let p = env::temp_dir().join(format!("bootoptim-interposer-test-{label}-{}", unique_suffix()));
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn write_ready_profile_control(root: &Path, uuid: &str) {
        let control = root.join(".pandora-layout-v1");
        fs::create_dir_all(control.join("locks")).unwrap();
        fs::write(
            control.join("identity.json"),
            format!("{{\n  \"schema\": 1,\n  \"profile_uuid\": \"{uuid}\"\n}}\n"),
        )
        .unwrap();
        fs::write(
            control.join("manifest.json"),
            format!(
                "{{\n  \"schema\": 1,\n  \"profile_uuid\": \"{uuid}\",\n  \"generation\": 1,\n  \"state\": \"ready\",\n  \"managed_input_fingerprint\": \"test\",\n  \"sync_identity\": \"\",\n  \"sandbox_policy\": \"\",\n  \"managed_entries\": {{}},\n  \"transaction_id\": null\n}}\n"
            ),
        )
        .unwrap();
        fs::write(control.join("locks").join(format!("{uuid}.lock")), b"").unwrap();
    }

    #[test]
    fn persistent_profile_namespace_is_uuid_bound_and_rename_stable() {
        let root = temp_dir("profile-namespace");
        let uuid = "01234567-89ab-cdef-8123-456789abcdef";
        write_ready_profile_control(&root, uuid);

        let scope = acquire_appcds_profile_scope(&root).unwrap();
        assert_eq!(scope.namespace().profile_uuid(), Some(uuid));
        drop(scope);

        let cache = root.join(".bootoptim/appcds");
        fs::create_dir_all(&cache).unwrap();
        bind_appcds_cache_namespace(&cache, &AppCdsProfileNamespace::PersistentProfile(uuid.to_string())).unwrap();
        assert_eq!(
            fs::read_to_string(cache.join("profile.namespace")).unwrap(),
            format!("schema=1\nprofile_uuid={uuid}\n")
        );

        let renamed = root.with_extension("renamed");
        fs::rename(&root, &renamed).unwrap();
        let renamed_scope = acquire_appcds_profile_scope(&renamed).unwrap();
        assert_eq!(renamed_scope.namespace().profile_uuid(), Some(uuid));
        drop(renamed_scope);
        let _ = fs::remove_dir_all(renamed);
    }

    #[test]
    fn persistent_profile_namespace_rejects_recovery_and_conflicts() {
        let root = temp_dir("profile-recovery");
        let uuid = "11234567-89ab-cdef-8123-456789abcdef";
        write_ready_profile_control(&root, uuid);
        let control = root.join(".pandora-layout-v1");

        fs::write(control.join("journal.json"), b"{}").unwrap();
        assert!(acquire_appcds_profile_scope(&root).is_err());
        fs::remove_file(control.join("journal.json")).unwrap();

        fs::create_dir_all(control.join("conflicts")).unwrap();
        fs::write(control.join("conflicts").join("pending"), b"evidence").unwrap();
        assert!(acquire_appcds_profile_scope(&root).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn persistent_cache_binding_quarantines_unbound_cache_and_rejects_wrong_uuid() {
        let root = temp_dir("profile-cache-binding");
        let cache = root.join(".bootoptim/appcds");
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join("ready.jsa"), b"legacy-unbound").unwrap();

        let uuid = "21234567-89ab-cdef-8123-456789abcdef";
        let namespace = AppCdsProfileNamespace::PersistentProfile(uuid.to_string());
        bind_appcds_cache_namespace(&cache, &namespace).unwrap();
        assert!(!cache.join("ready.jsa").exists());
        assert_eq!(
            fs::read_to_string(cache.join("profile.namespace")).unwrap(),
            format!("schema=1\nprofile_uuid={uuid}\n")
        );
        assert!(
            fs::read_dir(root.join(".bootoptim"))
                .unwrap()
                .flatten()
                .any(|entry| entry.file_name().to_string_lossy().starts_with("appcds-unbound-v0-"))
        );

        let other = AppCdsProfileNamespace::PersistentProfile("31234567-89ab-cdef-8123-456789abcdef".to_string());
        assert!(bind_appcds_cache_namespace(&cache, &other).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn persistent_profile_scope_lock_excludes_second_process() {
        const ROOT_ENV: &str = "BOOTOPTIM_TEST_PROFILE_SCOPE_ROOT";
        const READY_ENV: &str = "BOOTOPTIM_TEST_PROFILE_SCOPE_READY";
        const ROLE_ENV: &str = "BOOTOPTIM_TEST_PROFILE_SCOPE_ROLE";

        if let Some(root) = env::var_os(ROOT_ENV) {
            let root = PathBuf::from(root);
            let role = env::var(ROLE_ENV).unwrap_or_default();
            let scope = acquire_appcds_profile_scope(&root);
            if role == "holder" {
                let _scope = scope.expect("holder must acquire persistent profile lease");
                fs::write(PathBuf::from(env::var_os(READY_ENV).unwrap()), b"ready").unwrap();
                std::thread::sleep(std::time::Duration::from_millis(1500));
            } else {
                assert!(scope.is_err(), "contender must fail closed while profile lease is held");
            }
            return;
        }

        let root = temp_dir("profile-process-lock");
        let ready = root.join("holder.ready");
        let uuid = "41234567-89ab-cdef-8123-456789abcdef";
        write_ready_profile_control(&root, uuid);
        let exe = env::current_exe().unwrap();
        let test_name = "tests::persistent_profile_scope_lock_excludes_second_process";

        let mut holder = Command::new(&exe)
            .arg("--exact")
            .arg(test_name)
            .arg("--nocapture")
            .env(ROOT_ENV, &root)
            .env(READY_ENV, &ready)
            .env(ROLE_ENV, "holder")
            .spawn()
            .unwrap();

        for _ in 0..100 {
            if ready.is_file() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(ready.is_file(), "profile lease holder did not acquire in time");

        let contender = Command::new(&exe)
            .arg("--exact")
            .arg(test_name)
            .arg("--nocapture")
            .env(ROOT_ENV, &root)
            .env(READY_ENV, &ready)
            .env(ROLE_ENV, "contender")
            .status()
            .unwrap();
        assert!(contender.success());
        assert!(holder.wait().unwrap().success());

        assert!(acquire_appcds_profile_scope(&root).is_ok());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn deleting_profile_root_deletes_bound_appcds_cache() {
        let root = temp_dir("profile-delete");
        let uuid = "51234567-89ab-cdef-8123-456789abcdef";
        write_ready_profile_control(&root, uuid);
        let cache = root.join(".bootoptim/appcds");
        bind_appcds_cache_namespace(&cache, &AppCdsProfileNamespace::PersistentProfile(uuid.to_string())).unwrap();
        fs::write(cache.join("ready.jsa"), b"profile-owned").unwrap();
        assert!(cache.join("ready.jsa").is_file());

        fs::remove_dir_all(&root).unwrap();
        assert!(!root.exists());
        assert!(!cache.exists());
    }

    #[test]
    fn legacy_instance_cache_namespace_remains_local_and_compatible() {
        let root = temp_dir("legacy-profile");
        let scope = acquire_appcds_profile_scope(&root).unwrap();
        assert_eq!(scope.namespace(), &AppCdsProfileNamespace::LegacyInstanceLocal);
        let cache = root.join(".bootoptim/appcds");
        bind_appcds_cache_namespace(&cache, scope.namespace()).unwrap();
        assert!(cache.is_dir());
        assert!(!cache.join("profile.namespace").exists());
        let _ = fs::remove_dir_all(root);
    }

    fn write_exact_clone_candidate(root: &Path, profile_uuid: &str, expected_plan_sha256: &str, archive: &[u8]) {
        let candidate = root.join(".bootoptim/appcds-exact-clone-candidate");
        fs::create_dir_all(&candidate).unwrap();
        let archive_sha256 = hash_file_bytes_for_test(archive);
        fs::write(candidate.join("archive.jsa"), archive).unwrap();
        fs::write(
            candidate.join("candidate.meta"),
            format!(
                "schema=1\nsource_profile_uuid=61234567-89ab-cdef-8123-456789abcdef\ndestination_profile_uuid={profile_uuid}\nsource_plan_sha256=source\nexpected_plan_sha256={expected_plan_sha256}\narchive_sha256={archive_sha256}\narchive_size={}\n",
                archive.len()
            ),
        )
        .unwrap();
        fs::write(candidate.join("candidate.complete"), b"complete\n").unwrap();
    }

    fn hash_file_bytes_for_test(bytes: &[u8]) -> String {
        sha256_hex(bytes)
    }

    #[test]
    fn exact_clone_candidate_promotes_only_after_destination_plan_matches() {
        let root = temp_dir("exact-clone-adopt");
        let uuid = "71234567-89ab-cdef-8123-456789abcdef";
        write_ready_profile_control(&root, uuid);
        let scope = acquire_appcds_profile_scope(&root).unwrap();
        let cache = root.join(".bootoptim/appcds");
        bind_appcds_cache_namespace(&cache, scope.namespace()).unwrap();

        let plan = LaunchPlan {
            bytes: b"{\"destination\":true}\n".to_vec(),
            sha256: sha256_hex(b"{\"destination\":true}\n"),
            eligible: true,
        };
        write_exact_clone_candidate(&root, uuid, &plan.sha256, b"inherited-ready");
        assert_eq!(
            try_adopt_exact_clone_candidate(&root, &cache, scope.namespace(), &plan).unwrap(),
            ExactCloneAdoption::Adopted
        );
        assert_eq!(classify_cache(&cache, &plan.sha256).unwrap().0, CacheState::Ready);
        assert!(!root.join(".bootoptim/appcds-exact-clone-candidate").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn exact_clone_input_change_rejects_candidate_and_never_creates_ready() {
        let root = temp_dir("exact-clone-input-change");
        let uuid = "81234567-89ab-cdef-8123-456789abcdef";
        write_ready_profile_control(&root, uuid);
        let scope = acquire_appcds_profile_scope(&root).unwrap();
        let cache = root.join(".bootoptim/appcds");
        bind_appcds_cache_namespace(&cache, scope.namespace()).unwrap();

        let inherited = sha256_hex(b"source-effective-plan");
        write_exact_clone_candidate(&root, uuid, &inherited, b"inherited-ready");
        let changed = LaunchPlan {
            bytes: b"changed-mod-config-java-loader-or-args".to_vec(),
            sha256: sha256_hex(b"changed-mod-config-java-loader-or-args"),
            eligible: true,
        };
        assert_eq!(
            try_adopt_exact_clone_candidate(&root, &cache, scope.namespace(), &changed).unwrap(),
            ExactCloneAdoption::Rejected
        );
        assert!(!cache.join("ready.jsa").exists());
        assert!(
            fs::read_dir(root.join(".bootoptim"))
                .unwrap()
                .flatten()
                .any(|entry| entry.file_name().to_string_lossy().starts_with("appcds-exact-clone-rejected-"))
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn exact_clone_partial_or_corrupt_candidate_never_adopts_ready() {
        let root = temp_dir("exact-clone-corrupt");
        let uuid = "91234567-89ab-cdef-8123-456789abcdef";
        write_ready_profile_control(&root, uuid);
        let scope = acquire_appcds_profile_scope(&root).unwrap();
        let cache = root.join(".bootoptim/appcds");
        bind_appcds_cache_namespace(&cache, scope.namespace()).unwrap();
        let plan = LaunchPlan {
            bytes: b"plan".to_vec(),
            sha256: sha256_hex(b"plan"),
            eligible: true,
        };
        write_exact_clone_candidate(&root, uuid, &plan.sha256, b"archive");
        fs::remove_file(root.join(".bootoptim/appcds-exact-clone-candidate/candidate.complete")).unwrap();

        assert_eq!(
            try_adopt_exact_clone_candidate(&root, &cache, scope.namespace(), &plan).unwrap(),
            ExactCloneAdoption::Rejected
        );
        assert!(!cache.join("ready.jsa").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sha256_known_vector() {
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn parses_control_args_and_preserves_unicode_spaces() {
        let args = vec![
            OsString::from("--instance-dir"),
            OsString::from("C:\\Pack Ünicode\\Instance One"),
            OsString::from("--launcher-exe"),
            OsString::from("C:\\Pandora Folder\\Pandora.exe"),
            OsString::from("--"),
            OsString::from("C:\\Java 25\\bin\\javaw.exe"),
            OsString::from("-cp"),
            OsString::from("C:\\Lib A\\a.jar;C:\\Lïb B\\b.jar"),
        ];
        let p = parse_args(args).unwrap();
        assert_eq!(p.java_args.len(), 2);
        assert!(p.instance_dir.to_string_lossy().contains("Ünicode"));
        assert!(p.java_exe.to_string_lossy().contains("Java 25"));
    }

    #[test]
    fn classpath_parser_keeps_original_string() {
        let args = vec![
            OsString::from("-Xmx6G"),
            OsString::from("-cp"),
            OsString::from("A B.jar;Ü.jar"),
            OsString::from("Main"),
        ];
        assert_eq!(find_classpath(&args).unwrap(), OsString::from("A B.jar;Ü.jar"));
    }

    #[test]
    fn module_path_parser_keeps_original_order_and_rejects_ambiguity() {
        let raw = OsString::from("A Module.jar;Ü Module.jar");
        let args = vec![OsString::from("-p"), raw.clone(), OsString::from("Main")];
        assert_eq!(find_module_path(&args).unwrap(), Some(raw));

        let long = vec![
            OsString::from("--module-path=first.jar;second.jar"),
            OsString::from("Main"),
        ];
        assert_eq!(find_module_path(&long).unwrap(), Some(OsString::from("first.jar;second.jar")));

        let duplicate = vec![
            OsString::from("-p"),
            OsString::from("first.jar"),
            OsString::from("--module-path"),
            OsString::from("second.jar"),
        ];
        assert!(find_module_path(&duplicate).is_err());
    }

    #[test]
    fn sensitive_literals_are_never_exposed() {
        assert!(is_sensitive_arg(OsStr::new("-DauthToken=VERY_SECRET")));
        assert_eq!(safe_literal(OsStr::new("-DauthToken=VERY_SECRET")), None);
        assert_eq!(safe_literal(OsStr::new("-Xmx6G")), Some("-Xmx6G".to_string()));
        let status = "BOOTOPTIM_INTERPOSER status=fail-open reason=helper-error";
        assert!(!status.contains("VERY_SECRET"));
    }

    #[test]
    fn lock_excludes_second_owner() {
        let d = temp_dir("lock");
        let p = d.join("cache.lock");
        let g1 = try_lock(&p).unwrap().unwrap();
        assert!(try_lock(&p).unwrap().is_none());
        drop(g1);
        assert!(try_lock(&p).unwrap().is_some());
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn lock_excludes_a_second_process() {
        const PATH_ENV: &str = "BOOTOPTIM_TEST_LOCK_PATH";
        const READY_ENV: &str = "BOOTOPTIM_TEST_LOCK_READY";
        const ROLE_ENV: &str = "BOOTOPTIM_TEST_LOCK_ROLE";

        if let Some(lock_path) = env::var_os(PATH_ENV) {
            let role = env::var(ROLE_ENV).unwrap_or_default();
            let acquired = try_lock(Path::new(&lock_path)).unwrap();
            if role == "holder" {
                let _guard = acquired.expect("holder must acquire OS lock");
                let ready = PathBuf::from(env::var_os(READY_ENV).unwrap());
                fs::write(ready, b"ready").unwrap();
                std::thread::sleep(std::time::Duration::from_millis(1500));
            } else {
                assert!(acquired.is_none(), "contender must fail open while holder owns lock");
            }
            return;
        }

        let d = temp_dir("process-lock");
        let lock = d.join("cache.lock");
        let ready = d.join("holder.ready");
        let exe = env::current_exe().unwrap();
        let test_name = "tests::lock_excludes_a_second_process";

        let mut holder = Command::new(&exe)
            .arg("--exact")
            .arg(test_name)
            .arg("--nocapture")
            .env(PATH_ENV, &lock)
            .env(READY_ENV, &ready)
            .env(ROLE_ENV, "holder")
            .spawn()
            .unwrap();

        for _ in 0..100 {
            if ready.is_file() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(ready.is_file(), "holder did not acquire lock in time");

        let contender = Command::new(&exe)
            .arg("--exact")
            .arg(test_name)
            .arg("--nocapture")
            .env(PATH_ENV, &lock)
            .env(READY_ENV, &ready)
            .env(ROLE_ENV, "contender")
            .status()
            .unwrap();
        assert!(contender.success());
        assert!(holder.wait().unwrap().success());
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn incomplete_staging_is_never_ready() {
        let d = temp_dir("incomplete");
        fs::write(d.join("staging-dead.jsa"), b"partial").unwrap();
        let (state, _) = classify_cache(&d, "plan").unwrap();
        assert_eq!(state, CacheState::Failed);
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn ready_metadata_supports_legacy_and_round_trips_usn_identity() {
        let d = temp_dir("ready-metadata-usn");
        let path = d.join("ready.meta");
        fs::write(
            &path,
            "schema=1\nplan_sha256=plan\narchive_sha256=hash\narchive_size=7\nhelper_version=test\n",
        )
        .unwrap();
        assert_eq!(read_metadata(&path).unwrap().archive_usn_identity, None);

        let expected = ReadyMetadata {
            plan_sha256: "plan".to_string(),
            archive_sha256: "hash".to_string(),
            archive_size: 7,
            helper_version: "test".to_string(),
            archive_usn_identity: Some(ArchiveUsnIdentity {
                volume_guid: r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\".to_string(),
                volume_serial: 42,
                file_id: [0xabu8; 16],
                journal_id: 43,
                snapshot_first_usn: 10,
                snapshot_lowest_valid_usn: 12,
                snapshot_next_usn: 20,
                file_usn: 18,
            }),
        };
        fs::write(&path, metadata_bytes(&expected)).unwrap();
        assert_eq!(read_metadata(&path).unwrap().archive_usn_identity, expected.archive_usn_identity);
        let _ = fs::remove_dir_all(d);
    }

    #[cfg(windows)]
    #[test]
    fn ready_archive_usn_reuse_skips_hash_until_file_changes() {
        let d = temp_dir("ready-archive-usn");
        let ready = d.join("ready.jsa");
        fs::write(&ready, b"archive").unwrap();
        let archive_usn_identity = read_ready_archive_usn_identity(&ready).unwrap();
        let md = ReadyMetadata {
            plan_sha256: "plan".to_string(),
            archive_sha256: hash_file(&ready).unwrap(),
            archive_size: fs::metadata(&ready).unwrap().len(),
            helper_version: HELPER_VERSION.to_string(),
            archive_usn_identity: Some(archive_usn_identity),
        };
        fs::write(d.join("ready.meta"), metadata_bytes(&md)).unwrap();

        assert_eq!(classify_cache(&d, "plan").unwrap().0, CacheState::Ready);
        fs::write(&ready, b"tamper!").unwrap();
        assert_eq!(classify_cache(&d, "plan").unwrap().0, CacheState::Stale);
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn promotion_and_stale_detection_are_exact() {
        let d = temp_dir("promote");
        let staging = d.join("staging-ok.jsa");
        fs::write(&staging, b"archive bytes").unwrap();
        promote_archive(&d, &staging, "plan-a").unwrap();
        assert_eq!(classify_cache(&d, "plan-a").unwrap().0, CacheState::Ready);
        assert_eq!(classify_cache(&d, "plan-b").unwrap().0, CacheState::Stale);
        fs::write(d.join("ready.jsa"), b"tampered").unwrap();
        assert_eq!(classify_cache(&d, "plan-a").unwrap().0, CacheState::Stale);
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn plan_byte_comparison_is_deterministic() {
        let d = temp_dir("plan");
        let bytes = b"{\"stable\":true}\n";
        assert!(!persist_plan_and_compare(&d, bytes, &sha256_hex(bytes)).unwrap());
        assert!(persist_plan_and_compare(&d, bytes, &sha256_hex(bytes)).unwrap());
        let changed = b"{\"stable\":false}\n";
        assert!(!persist_plan_and_compare(&d, changed, &sha256_hex(changed)).unwrap());
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn launch_identity_changes_invalidate_plan_and_stale_never_consumes_archive() {
        let d = temp_dir("identity-change");
        let java_root = d.join("runtime");
        fs::create_dir_all(java_root.join("bin")).unwrap();
        let java = java_root.join(if cfg!(windows) { "bin/javaw.exe" } else { "bin/java" });
        fs::write(&java, b"fake-java-v1").unwrap();
        fs::write(
            java_root.join("release"),
            b"JAVA_VERSION=\"25.0.4\"\nIMPLEMENTOR=\"Oracle Corporation\"\n",
        )
        .unwrap();
        let lib = d.join("library one.jar");
        fs::write(&lib, b"jar-v1").unwrap();
        let module_a = d.join("module one.jar");
        let module_b = d.join("módulo two.jar");
        fs::write(&module_a, b"module-a-v1").unwrap();
        fs::write(&module_b, b"module-b-v1").unwrap();
        fs::create_dir_all(d.join("mods")).unwrap();
        fs::write(d.join("mods/mod.jar"), b"mod-v1").unwrap();
        fs::create_dir_all(d.join("config")).unwrap();
        fs::write(d.join("config/fml.toml"), b"earlyWindowControl=true\n").unwrap();
        fs::write(d.join("options.txt"), b"resourcePacks:[\"vanilla\"]\nincompatibleResourcePacks:[]\n").unwrap();
        let launcher = d.join("Pandora Launcher.exe");
        fs::write(&launcher, b"launcher").unwrap();
        let cp = env::join_paths([lib.clone()]).unwrap();
        let module_path_raw = env::join_paths([module_a.clone(), module_b.clone()]).unwrap();
        let parsed = ParsedArgs {
            instance_dir: d.clone(),
            launcher_exe: Some(launcher),
            upstream_commit: UPSTREAM_DEFAULT.to_string(),
            appcds_identity_normal_gui: false,
            java_exe: java.clone().into_os_string(),
            java_args: vec![
                OsString::from("-DauthToken=VERY_SECRET"),
                OsString::from("-cp"),
                cp,
                OsString::from("--module-path"),
                module_path_raw.clone(),
                OsString::from("com.moulberry.pandora.LaunchWrapper"),
            ],
        };
        let p1 = build_launch_plan(&parsed).unwrap();
        let p2 = build_launch_plan(&parsed).unwrap();
        assert_eq!(p1.bytes, p2.bytes);
        assert!(p1.eligible);
        assert_eq!(find_module_path(&parsed.java_args).unwrap(), Some(module_path_raw));
        let plan_text = String::from_utf8_lossy(&p1.bytes);
        assert!(!plan_text.contains("VERY_SECRET"));
        let module_section = plan_text.split("\"module_path\"").nth(1).unwrap();
        let module_a_hash = hash_file(&module_a).unwrap();
        let module_b_hash = hash_file(&module_b).unwrap();
        let a_pos = module_section.find(&module_a_hash).unwrap();
        let b_pos = module_section.find(&module_b_hash).unwrap();
        assert!(a_pos < b_pos, "module path fingerprint must preserve JVM path order");

        fs::write(&lib, b"jar-v2").unwrap();
        let p3 = build_launch_plan(&parsed).unwrap();
        assert_ne!(p1.sha256, p3.sha256);
        fs::write(&lib, b"jar-v1").unwrap();

        fs::write(&module_b, b"module-b-v2").unwrap();
        let module_changed = build_launch_plan(&parsed).unwrap();
        assert_ne!(p1.sha256, module_changed.sha256);
        fs::write(&module_b, b"module-b-v1").unwrap();

        fs::write(d.join("config/fml.toml"), b"earlyWindowControl=false\n").unwrap();
        let config_changed = build_launch_plan(&parsed).unwrap();
        assert_ne!(p1.sha256, config_changed.sha256);
        fs::write(d.join("config/fml.toml"), b"earlyWindowControl=true\n").unwrap();

        fs::write(
            d.join("options.txt"),
            b"resourcePacks:[\"vanilla\",\"file/Test Pack.zip\"]\nincompatibleResourcePacks:[]\n",
        )
        .unwrap();
        let resource_selection_changed = build_launch_plan(&parsed).unwrap();
        assert_ne!(p1.sha256, resource_selection_changed.sha256);
        fs::write(d.join("options.txt"), b"resourcePacks:[\"vanilla\"]\nincompatibleResourcePacks:[]\n").unwrap();

        let java2_root = d.join("runtime copy");
        fs::create_dir_all(java2_root.join("bin")).unwrap();
        let java2 = java2_root.join(if cfg!(windows) { "bin/javaw.exe" } else { "bin/java" });
        fs::write(&java2, b"fake-java-v1").unwrap();
        fs::write(
            java2_root.join("release"),
            b"JAVA_VERSION=\"25.0.4\"\nIMPLEMENTOR=\"Oracle Corporation\"\n",
        )
        .unwrap();
        let moved = ParsedArgs {
            instance_dir: parsed.instance_dir.clone(),
            launcher_exe: parsed.launcher_exe.clone(),
            upstream_commit: parsed.upstream_commit.clone(),
            appcds_identity_normal_gui: false,
            java_exe: java2.clone().into_os_string(),
            java_args: parsed.java_args.clone(),
        };
        let p4 = build_launch_plan(&moved).unwrap();
        assert_ne!(p1.sha256, p4.sha256);
        fs::write(&java2, b"fake-java-v2").unwrap();
        let p5 = build_launch_plan(&moved).unwrap();
        assert_ne!(p4.sha256, p5.sha256);

        assert!(shared_archive_flags(CacheState::Stale, &d.join("ready.jsa")).is_empty());
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn agent_and_conflicting_cds_flags_block_activation() {
        assert!(has_agent_configuration(&[OsString::from("-javaagent:x.jar")]));
        assert!(has_agent_text("-XX:+AllowArchivingWithJavaAgent"));
        assert!(!has_agent_configuration(&[OsString::from("-Xmx6G")]));
        assert!(has_conflicting_cds_configuration(&[OsString::from("-XX:+AutoCreateSharedArchive")]));
        assert!(has_conflicting_cds_configuration(&[OsString::from("-Xshare:on")]));
    }

    #[test]
    fn upgrade_module_path_is_fail_closed() {
        assert!(has_unsupported_module_configuration(&[
            OsString::from("--upgrade-module-path"),
            OsString::from("upgrade.jar")
        ]));
        assert!(has_unsupported_module_configuration(&[OsString::from(
            "--upgrade-module-path=upgrade.jar"
        )]));
    }

    #[test]
    fn patch_module_is_fail_closed() {
        assert!(has_unsupported_module_configuration(&[
            OsString::from("--patch-module"),
            OsString::from("example=patch.jar")
        ]));
        assert!(has_unsupported_module_configuration(&[OsString::from(
            "--patch-module=example=patch.jar"
        )]));
    }

    #[test]
    fn limit_modules_is_fail_closed() {
        assert!(has_unsupported_module_configuration(&[
            OsString::from("--limit-modules"),
            OsString::from("java.base")
        ]));
        assert!(has_unsupported_module_configuration(&[OsString::from("--limit-modules=java.base")]));
        assert!(!has_unsupported_module_configuration(&[OsString::from("--add-modules=ALL-MODULE-PATH")]));
    }

    #[test]
    fn first_eligible_identity_trains_without_a_proof_only_launch() {
        let d = temp_dir("immediate-train");
        let plan_bytes = b"{\"identity\":\"first\"}\n";
        let plan_sha = sha256_hex(plan_bytes);

        let stable = persist_plan_and_compare(&d, plan_bytes, &plan_sha).unwrap();
        assert!(!stable, "first observation must still be FIRST_OR_MISMATCH");
        assert_eq!(prepare_eligible_cache(&d, &plan_sha).unwrap(), PrepareDecision::Train);
        assert_eq!(read_training_plan(&d.join("training.meta")).unwrap(), plan_sha);
        assert!(!d.join("ready.jsa").exists());

        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn training_needs_clean_exit_and_a_later_matching_identity_before_ready() {
        let d = temp_dir("train-confirm");
        let plan_bytes = b"{\"identity\":\"same\"}\n";
        let plan_sha = sha256_hex(plan_bytes);

        assert!(!persist_plan_and_compare(&d, plan_bytes, &plan_sha).unwrap());
        assert_eq!(prepare_eligible_cache(&d, &plan_sha).unwrap(), PrepareDecision::Train);

        // A concurrent launch, crash/kill, or archive that appears before Pandora
        // has observed a clean Java exit must not consume or promote anything.
        assert_eq!(reconcile_training(&d, &plan_sha).unwrap(), TrainingReconcile::Matching);
        assert_eq!(prepare_eligible_cache(&d, &plan_sha).unwrap(), PrepareDecision::Stock);
        fs::write(d.join("training.jsa"), b"partial-or-final-looking").unwrap();
        assert_eq!(prepare_eligible_cache(&d, &plan_sha).unwrap(), PrepareDecision::Stock);
        assert!(!d.join("ready.jsa").exists());

        // Only a later independently rebuilt exact identity plus the clean-exit
        // marker may promote and be consumed by that later launch.
        fs::write(d.join("training.complete"), b"complete\n").unwrap();
        assert!(persist_plan_and_compare(&d, plan_bytes, &plan_sha).unwrap());
        assert_eq!(reconcile_training(&d, &plan_sha).unwrap(), TrainingReconcile::Matching);
        assert_eq!(prepare_eligible_cache(&d, &plan_sha).unwrap(), PrepareDecision::Ready);
        assert!(d.join("ready.jsa").is_file());
        assert!(d.join("ready.meta").is_file());
        assert!(!d.join("training.meta").exists());
        assert!(!d.join("training.complete").exists());

        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn intervening_identity_mismatch_discards_training_and_falls_back() {
        let d = temp_dir("training-mismatch");
        let plan_a = "a".repeat(64);
        let plan_b = "b".repeat(64);

        assert_eq!(prepare_eligible_cache(&d, &plan_a).unwrap(), PrepareDecision::Train);
        fs::write(d.join("training.jsa"), b"archive-a").unwrap();
        fs::write(d.join("training.complete"), b"complete\n").unwrap();

        assert_eq!(
            reconcile_training(&d, &plan_b).unwrap(),
            TrainingReconcile::Discarded("training-plan-mismatch")
        );
        assert!(!d.join("training.meta").exists());
        assert!(!d.join("training.jsa").exists());
        assert!(!d.join("training.complete").exists());
        assert!(d.join("training.invalid").is_file());
        assert!(!d.join("ready.jsa").exists());
        assert_eq!(
            prepare_eligible_cache(&d, &plan_b).unwrap(),
            PrepareDecision::Stock,
            "invalidated campaign must block a replacement writer until explicit reset"
        );

        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn corrupt_training_metadata_or_completion_never_promotes() {
        let d = temp_dir("training-corrupt-meta");
        let plan = "c".repeat(64);

        fs::write(
            d.join("training.meta"),
            format!("schema={}\nplan_sha256={}\nunexpected=true\n", SCHEMA_VERSION, plan),
        )
        .unwrap();
        fs::write(d.join("training.jsa"), b"archive").unwrap();
        fs::write(d.join("training.complete"), b"complete\n").unwrap();
        assert_eq!(
            reconcile_training(&d, &plan).unwrap(),
            TrainingReconcile::Discarded("training-metadata-corrupt")
        );
        assert!(d.join("training.invalid").is_file());
        assert!(!d.join("ready.jsa").exists());
        assert_eq!(prepare_eligible_cache(&d, &plan).unwrap(), PrepareDecision::Stock);
        let _ = fs::remove_dir_all(d);

        let d = temp_dir("training-corrupt-complete");
        assert_eq!(prepare_eligible_cache(&d, &plan).unwrap(), PrepareDecision::Train);
        fs::write(d.join("training.jsa"), b"archive").unwrap();
        fs::write(d.join("training.complete"), b"not-complete\n").unwrap();
        assert_eq!(
            reconcile_training(&d, &plan).unwrap(),
            TrainingReconcile::Discarded("training-completion-corrupt")
        );
        assert!(d.join("training.invalid").is_file());
        assert!(!d.join("ready.jsa").exists());
        assert!(!d.join("training.meta").exists());
        assert_eq!(prepare_eligible_cache(&d, &plan).unwrap(), PrepareDecision::Stock);

        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn verified_ready_archive_reuses_exact_identity_without_plan_history_gate() {
        let d = temp_dir("ready-reuse");
        let plan = "d".repeat(64);
        let staging = d.join("staging-ready.jsa");
        fs::write(&staging, b"verified-ready").unwrap();
        promote_archive(&d, &staging, &plan).unwrap();

        assert_eq!(prepare_eligible_cache(&d, &plan).unwrap(), PrepareDecision::Ready);
        assert_eq!(prepare_eligible_cache(&d, &"e".repeat(64)).unwrap(), PrepareDecision::Stock);
        assert_eq!(prepare_eligible_cache(&d, &plan).unwrap(), PrepareDecision::Ready);

        let _ = fs::remove_dir_all(d);
    }
    #[cfg(windows)]
    #[test]
    fn composed_first_run_strong_train_second_run_usn_reuse_promotes_ready() {
        let root = temp_dir("composed-two-run");
        let uuid = "a1234567-89ab-cdef-8123-456789abcdef";
        write_ready_profile_control(&root, uuid);
        fs::create_dir_all(root.join("runtime/bin")).unwrap();
        fs::create_dir_all(root.join("mods")).unwrap();
        fs::create_dir_all(root.join("config")).unwrap();
        let java = root.join("runtime/bin/javaw.exe");
        let lib = root.join("library.jar");
        fs::write(&java, b"fake-java-v1").unwrap();
        fs::write(root.join("runtime/release"), b"JAVA_VERSION=\"25.0.4\"\n").unwrap();
        fs::write(&lib, b"lib-v1-large-placeholder").unwrap();
        fs::write(root.join("mods/mod.jar"), b"mod-v1-large-placeholder").unwrap();
        fs::write(root.join("config/raw.cfg"), b"raw-config-v1").unwrap();
        fs::write(root.join("options.txt"), b"resourcePacks:[\"vanilla\"]\nincompatibleResourcePacks:[]\n").unwrap();
        let launcher = root.join("Pandora.exe");
        fs::write(&launcher, b"launcher-v1").unwrap();

        let parsed = ParsedArgs {
            instance_dir: root.clone(),
            launcher_exe: Some(launcher),
            upstream_commit: UPSTREAM_DEFAULT.to_string(),
            appcds_identity_normal_gui: true,
            java_exe: java.into_os_string(),
            java_args: vec![
                OsString::from("-cp"),
                env::join_paths([lib]).unwrap(),
                OsString::from("com.moulberry.pandora.LaunchWrapper"),
            ],
        };

        let scope = acquire_appcds_profile_scope(&root).unwrap();
        let cache = root.join(".bootoptim/appcds");
        bind_appcds_cache_namespace(&cache, scope.namespace()).unwrap();

        let mut seed = IdentityDigestCache::begin(&cache, true);
        let first = build_launch_plan_for_namespace_with_cache(&parsed, scope.namespace(), &mut seed).unwrap();
        assert_eq!(seed.stats().0, 0);
        assert!(seed.stats().1 > 0);
        seed.finish().unwrap();
        assert!(cache.join(APPCDS_IDENTITY_MANIFEST).is_file());
        assert!(!persist_plan_and_compare(&cache, &first.bytes, &first.sha256).unwrap());
        assert_eq!(prepare_eligible_cache(&cache, &first.sha256).unwrap(), PrepareDecision::Train);

        fs::write(cache.join("training.jsa"), b"trained-archive").unwrap();
        fs::write(cache.join("training.complete"), b"complete\n").unwrap();

        let mut reuse = IdentityDigestCache::begin(&cache, true);
        let second = build_launch_plan_for_namespace_with_cache(&parsed, scope.namespace(), &mut reuse).unwrap();
        assert_eq!(first.bytes, second.bytes);
        assert!(reuse.stats().0 > 0);
        assert_eq!(reuse.stats().1, 0);
        reuse.finish().unwrap();

        assert!(persist_plan_and_compare(&cache, &second.bytes, &second.sha256).unwrap());
        assert_eq!(reconcile_training(&cache, &second.sha256).unwrap(), TrainingReconcile::Matching);
        assert_eq!(prepare_eligible_cache(&cache, &second.sha256).unwrap(), PrepareDecision::Ready);
        assert!(cache.join("ready.jsa").is_file());
        assert!(!cache.join("training.meta").exists());
        assert!(!cache.join("training.complete").exists());
        let _ = fs::remove_dir_all(root);
    }
}
