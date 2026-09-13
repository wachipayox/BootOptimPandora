#[cfg(test)]
mod config_identity_tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let p = env::temp_dir().join(format!("bootoptim-config-id-{label}-{}", unique_suffix()));
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn pack_artifact(root: &Path, relative: &str, bytes: &[u8]) -> Artifact {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        pack_input_artifact(root, &path).unwrap()
    }

    #[test]
    fn authorized_properties_canonicalize_comments_and_only_drippy_timestamp() {
        let d = temp_dir("authorized");
        for relative in [
            "config/asyncparticles/asyncparticles-mixin.properties",
            "config/fabric/indigo-renderer.properties",
            "config/iris.properties",
        ] {
            let a = pack_artifact(&d, relative, b"#generated\n#Sun Sep 13 20:00:00 CEST 2026\nenabled=true\nquality=2\n");
            let b = pack_artifact(&d, relative, b"#generated\n#Mon Sep 14 20:00:00 CEST 2026\nquality=2\nenabled=true\n");
            assert_eq!(a.sha256, b.sha256, "authorized Java Properties mapping must ignore comments/order");
            let changed = pack_artifact(&d, relative, b"#generated\nenabled=false\nquality=2\n");
            assert_ne!(a.sha256, changed.sha256, "effective property value must invalidate");
        }

        let relative = "config/drippyloadingscreen/early_window_reference.properties";
        let a = pack_artifact(&d, relative, b"#Drippy Loading Screen early-window reference size\n#Sun Sep 13 20:00:00 CEST 2026\nheight=480\ntimestamp=1779653354159\nwidth=854\n");
        let b = pack_artifact(&d, relative, b"#Drippy Loading Screen early-window reference size\n#Mon Sep 14 20:00:00 CEST 2026\nwidth=854\ntimestamp=1779739754159\nheight=480\n");
        assert_eq!(a.sha256, b.sha256, "Drippy generated timestamp/comment must not define config identity");
        let changed = pack_artifact(&d, relative, b"#generated\nheight=480\ntimestamp=1779739754159\nwidth=1024\n");
        assert_ne!(a.sha256, changed.sha256, "Drippy width is effective and must invalidate");
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn unknown_or_ambiguous_properties_and_moreculling_toml_remain_raw() {
        let d = temp_dir("raw-fallback");
        let unknown = "config/other.properties";
        let u1 = pack_artifact(&d, unknown, b"#Sun Sep 13\nenabled=true\n");
        let u2 = pack_artifact(&d, unknown, b"#Mon Sep 14\nenabled=true\n");
        assert_ne!(u1.sha256, u2.sha256, "unknown properties path must stay raw");

        let authorized = "config/iris.properties";
        let duplicate = b"#Sun Sep 13\nenabled=true\nenabled=true\n";
        let duplicate_artifact = pack_artifact(&d, authorized, duplicate);
        assert_eq!(duplicate_artifact.sha256, sha256_hex(duplicate), "duplicate key must fall back to raw");
        let escaped = b"#Sun Sep 13\npath=a\\ b\n";
        let escaped_artifact = pack_artifact(&d, authorized, escaped);
        assert_eq!(escaped_artifact.sha256, sha256_hex(escaped), "escaped syntax must fall back to raw");
        let malformed = b"#Sun Sep 13\nnot-a-property-line\n";
        let malformed_artifact = pack_artifact(&d, authorized, malformed);
        assert_eq!(malformed_artifact.sha256, sha256_hex(malformed), "malformed properties must fall back to raw");

        let toml = "config/moreculling.toml";
        let t1 = pack_artifact(&d, toml, b"[modCompatibility]\na=true\nb=false\n");
        let t2 = pack_artifact(&d, toml, b"[modCompatibility]\nb=false\na=true\n");
        assert_ne!(t1.sha256, t2.sha256, "MoreCulling TOML remains raw pending exact byte/source proof");
        let bad_toml = b"[modCompatibility\na=true\n";
        let bad = pack_artifact(&d, toml, bad_toml);
        assert_eq!(bad.sha256, sha256_hex(bad_toml), "malformed TOML must remain raw");
        let _ = fs::remove_dir_all(d);
    }

    fn plan_fixture() -> (PathBuf, ParsedArgs, PathBuf, PathBuf, PathBuf) {
        let d = temp_dir("plan");
        let java_root = d.join("runtime");
        fs::create_dir_all(java_root.join("bin")).unwrap();
        let java = java_root.join(if cfg!(windows) { "bin/javaw.exe" } else { "bin/java" });
        fs::write(&java, b"fake-java-v1").unwrap();
        fs::write(java_root.join("release"), b"JAVA_VERSION=\"25.0.4\"\nIMPLEMENTOR=\"Oracle Corporation\"\n").unwrap();
        let lib = d.join("library.jar");
        fs::write(&lib, b"lib-v1").unwrap();
        fs::create_dir_all(d.join("mods")).unwrap();
        let mod_jar = d.join("mods/mod.jar");
        fs::write(&mod_jar, b"mod-v1").unwrap();
        fs::create_dir_all(d.join("config/fabric")).unwrap();
        let config = d.join("config/fabric/indigo-renderer.properties");
        fs::write(&config, b"#Sun Sep 13 20:00:00 CEST 2026\nenabled=true\n").unwrap();
        fs::write(d.join("options.txt"), b"resourcePacks:[\"vanilla\"]\nincompatibleResourcePacks:[]\n").unwrap();
        let launcher = d.join("Pandora.exe");
        fs::write(&launcher, b"launcher").unwrap();
        let parsed = ParsedArgs {
            instance_dir: d.clone(),
            launcher_exe: Some(launcher),
            upstream_commit: UPSTREAM_DEFAULT.to_string(),
            java_exe: java.clone().into_os_string(),
            java_args: vec![OsString::from("-cp"), env::join_paths([lib.clone()]).unwrap(), OsString::from("com.moulberry.pandora.LaunchWrapper")],
        };
        (d, parsed, java, lib, mod_jar)
    }

    #[test]
    fn plan_ignores_only_canonical_noise_and_still_invalidates_real_inputs() {
        let (d, parsed, java, lib, mod_jar) = plan_fixture();
        let config = d.join("config/fabric/indigo-renderer.properties");
        let p1 = build_launch_plan(&parsed).unwrap();

        fs::write(&config, b"#Mon Sep 14 20:00:00 CEST 2026\nenabled=true\n").unwrap();
        let comment_only = build_launch_plan(&parsed).unwrap();
        assert_eq!(p1.sha256, comment_only.sha256, "generated comment alone must not change plan identity");

        fs::write(&config, b"#Mon Sep 14\nenabled=false\n").unwrap();
        assert_ne!(p1.sha256, build_launch_plan(&parsed).unwrap().sha256);
        fs::write(&config, b"#Sun Sep 13\nenabled=true\n").unwrap();

        fs::write(&mod_jar, b"mod-v2").unwrap();
        assert_ne!(p1.sha256, build_launch_plan(&parsed).unwrap().sha256);
        fs::write(&mod_jar, b"mod-v1").unwrap();

        fs::write(&lib, b"lib-v2").unwrap();
        assert_ne!(p1.sha256, build_launch_plan(&parsed).unwrap().sha256);
        fs::write(&lib, b"lib-v1").unwrap();

        fs::write(d.join("options.txt"), b"resourcePacks:[\"vanilla\",\"file/Test.zip\"]\nincompatibleResourcePacks:[]\n").unwrap();
        assert_ne!(p1.sha256, build_launch_plan(&parsed).unwrap().sha256);
        fs::write(d.join("options.txt"), b"resourcePacks:[\"vanilla\"]\nincompatibleResourcePacks:[]\n").unwrap();

        fs::write(&java, b"fake-java-v2").unwrap();
        assert_ne!(p1.sha256, build_launch_plan(&parsed).unwrap().sha256);
        let _ = fs::remove_dir_all(d);
    }
}
