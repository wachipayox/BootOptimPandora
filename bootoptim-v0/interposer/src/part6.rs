fn pack_input_artifact(instance_dir: &Path, path: &Path) -> io::Result<Artifact> {
    let meta = fs::metadata(path)?;
    if !meta.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "pack input not file"));
    }
    let relative = path.strip_prefix(instance_dir)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "pack input outside instance"))?;
    let canonical = if relative_components_equal(relative, &["config", "asyncparticles", "asyncparticles-mixin.properties"])
        || relative_components_equal(relative, &["config", "fabric", "indigo-renderer.properties"])
        || relative_components_equal(relative, &["config", "iris.properties"])
    {
        canonical_simple_properties(&fs::read(path)?, false)
    } else if relative_components_equal(relative, &["config", "drippyloadingscreen", "early_window_reference.properties"]) {
        canonical_simple_properties(&fs::read(path)?, true)
    } else {
        None
    };

    if let Some(bytes) = canonical {
        Ok(Artifact {
            role: "pack-input",
            path: encode_os(path.as_os_str()),
            size: bytes.len() as u64,
            sha256: sha256_hex(&bytes),
        })
    } else {
        Ok(Artifact {
            role: "pack-input",
            path: encode_os(path.as_os_str()),
            size: meta.len(),
            sha256: hash_file(path)?,
        })
    }
}

fn relative_components_equal(path: &Path, expected: &[&str]) -> bool {
    let actual = path.components().map(|c| c.as_os_str()).collect::<Vec<_>>();
    actual.len() == expected.len()
        && actual.iter().zip(expected).all(|(a, b)| *a == OsStr::new(b))
}

#[cfg(test)]
mod exact_config_path_tests {
    use super::*;

    #[test]
    fn lookalike_nested_path_is_not_authorized() {
        let root = env::temp_dir().join(format!("bootoptim-exact-path-{}", unique_suffix()));
        let nested = root.join("defaultconfigs/config/iris.properties");
        fs::create_dir_all(nested.parent().unwrap()).unwrap();
        let first = b"#Sun Sep 13\nenabled=true\n";
        fs::write(&nested, first).unwrap();
        let a = pack_input_artifact(&root, &nested).unwrap();
        assert_eq!(a.sha256, sha256_hex(first));
        let second = b"#Mon Sep 14\nenabled=true\n";
        fs::write(&nested, second).unwrap();
        let b = pack_input_artifact(&root, &nested).unwrap();
        assert_ne!(a.sha256, b.sha256, "lookalike non-authorized path must stay raw");
        let _ = fs::remove_dir_all(root);
    }
}
