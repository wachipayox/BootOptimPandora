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

fn canonical_simple_properties(bytes: &[u8], drippy_reference: bool) -> Option<Vec<u8>> {
    if !bytes.is_ascii() {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let mut values = BTreeMap::<String, String>::new();
    for raw_line in text.lines() {
        if raw_line.is_empty() || raw_line.starts_with('#') || raw_line.starts_with('!') {
            continue;
        }
        if raw_line.contains('\\')
            || raw_line.chars().next().map(char::is_whitespace).unwrap_or(false)
        {
            return None;
        }
        let (key, value) = raw_line.split_once('=')?;
        if key.is_empty()
            || key.trim() != key
            || key.chars().any(char::is_whitespace)
            || values.contains_key(key)
        {
            return None;
        }
        values.insert(key.to_string(), value.to_string());
    }
    if values.is_empty() {
        return None;
    }
    if drippy_reference {
        if values.len() != 3
            || !values.contains_key("height")
            || !values.contains_key("timestamp")
            || !values.contains_key("width")
            || values["height"].parse::<u32>().is_err()
            || values["width"].parse::<u32>().is_err()
            || values["timestamp"].parse::<u64>().is_err()
        {
            return None;
        }
        values.remove("timestamp");
    }
    let mut canonical = Vec::new();
    for (key, value) in values {
        canonical.extend_from_slice(key.as_bytes());
        canonical.push(b'=');
        canonical.extend_from_slice(value.as_bytes());
        canonical.push(b'\n');
    }
    Some(canonical)
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
