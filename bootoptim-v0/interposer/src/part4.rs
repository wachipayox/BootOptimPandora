#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let p = env::temp_dir().join(format!("bootoptim-interposer-test-{label}-{}", unique_suffix()));
        fs::create_dir_all(&p).unwrap(); p
    }

    #[test]
    fn sha256_known_vector() {
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn parses_control_args_and_preserves_unicode_spaces() {
        let args = vec![OsString::from("--instance-dir"), OsString::from("C:\\Pack Ünicode\\Instance One"), OsString::from("--launcher-exe"), OsString::from("C:\\Pandora Folder\\Pandora.exe"), OsString::from("--"), OsString::from("C:\\Java 25\\bin\\javaw.exe"), OsString::from("-cp"), OsString::from("C:\\Lib A\\a.jar;C:\\Lïb B\\b.jar")];
        let p = parse_args(args).unwrap();
        assert_eq!(p.java_args.len(), 2);
        assert!(p.instance_dir.to_string_lossy().contains("Ünicode"));
        assert!(p.java_exe.to_string_lossy().contains("Java 25"));
    }

    #[test]
    fn classpath_parser_keeps_original_string() {
        let args = vec![OsString::from("-Xmx6G"), OsString::from("-cp"), OsString::from("A B.jar;Ü.jar"), OsString::from("Main")];
        assert_eq!(find_classpath(&args).unwrap(), OsString::from("A B.jar;Ü.jar"));
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
        let d = temp_dir("lock"); let p = d.join("cache.lock");
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
            if ready.is_file() { break; }
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
    fn promotion_and_stale_detection_are_exact() {
        let d = temp_dir("promote"); let staging = d.join("staging-ok.jsa");
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
    fn jar_java_and_path_changes_change_plan_and_stale_never_consumes_archive() {
        let d = temp_dir("identity-change");
        let java_root = d.join("runtime");
        fs::create_dir_all(java_root.join("bin")).unwrap();
        let java = java_root.join(if cfg!(windows) { "bin/javaw.exe" } else { "bin/java" });
        fs::write(&java, b"fake-java-v1").unwrap();
        fs::write(java_root.join("release"), b"JAVA_VERSION=\"25.0.4\"\nIMPLEMENTOR=\"Oracle Corporation\"\n").unwrap();
        let lib = d.join("library one.jar");
        fs::write(&lib, b"jar-v1").unwrap();
        fs::create_dir_all(d.join("mods")).unwrap();
        fs::write(d.join("mods/mod.jar"), b"mod-v1").unwrap();
        let launcher = d.join("Pandora Launcher.exe");
        fs::write(&launcher, b"launcher").unwrap();
        let cp = env::join_paths([lib.clone()]).unwrap();
        let parsed = ParsedArgs {
            instance_dir: d.clone(),
            launcher_exe: Some(launcher),
            upstream_commit: UPSTREAM_DEFAULT.to_string(),
            java_exe: java.clone().into_os_string(),
            java_args: vec![OsString::from("-DauthToken=VERY_SECRET"), OsString::from("-cp"), cp, OsString::from("com.moulberry.pandora.LaunchWrapper")],
        };
        let p1 = build_launch_plan(&parsed).unwrap();
        let p2 = build_launch_plan(&parsed).unwrap();
        assert_eq!(p1.bytes, p2.bytes);
        assert!(!String::from_utf8_lossy(&p1.bytes).contains("VERY_SECRET"));
        fs::write(&lib, b"jar-v2").unwrap();
        let p3 = build_launch_plan(&parsed).unwrap();
        assert_ne!(p1.sha256, p3.sha256);

        fs::write(&lib, b"jar-v1").unwrap();
        let java2_root = d.join("runtime copy");
        fs::create_dir_all(java2_root.join("bin")).unwrap();
        let java2 = java2_root.join(if cfg!(windows) { "bin/javaw.exe" } else { "bin/java" });
        fs::write(&java2, b"fake-java-v1").unwrap();
        fs::write(java2_root.join("release"), b"JAVA_VERSION=\"25.0.4\"\nIMPLEMENTOR=\"Oracle Corporation\"\n").unwrap();
        let moved = ParsedArgs {
            instance_dir: parsed.instance_dir.clone(),
            launcher_exe: parsed.launcher_exe.clone(),
            upstream_commit: parsed.upstream_commit.clone(),
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
}
