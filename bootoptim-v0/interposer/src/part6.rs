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
    } else if relative_components_equal(relative, &["config", "moreculling.toml"]) {
        canonical_moreculling_mod_compatibility(&fs::read(path)?)
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

// This deliberately recognizes only the exact, observed MoreCulling 1.0.8
// layout. It does not parse or reserialize TOML and never writes the source
// file: the prefix stays byte-sensitive, while the final boolean table is
// treated as an unordered key/value mapping. Any comments, extra table,
// duplicate, escape, non-ASCII character or unexpected value falls back to a
// raw hash in pack_input_artifact.
fn canonical_moreculling_mod_compatibility(bytes: &[u8]) -> Option<Vec<u8>> {
    if !bytes.is_ascii() || !bytes.ends_with(b"\n") {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let mut before_table = Vec::new();
    let mut values = BTreeMap::<String, bool>::new();
    let mut found_table = false;

    for line in text.strip_suffix('\n')?.split('\n') {
        if !found_table {
            if line == "[modCompatibility]" {
                found_table = true;
            } else {
                before_table.push(line);
            }
            continue;
        }

        let (key, value) = line.split_once(" = ")?;
        if key.is_empty()
            || !key.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
            || values.contains_key(key)
        {
            return None;
        }
        let value = match value {
            "true" => true,
            "false" => false,
            _ => return None,
        };
        values.insert(key.to_owned(), value);
    }
    if !found_table || values.is_empty() {
        return None;
    }

    let mut canonical = Vec::new();
    for line in before_table {
        canonical.extend_from_slice(line.as_bytes());
        canonical.push(b'\n');
    }
    canonical.extend_from_slice(b"[modCompatibility]\n");
    for (key, value) in values {
        canonical.extend_from_slice(key.as_bytes());
        canonical.extend_from_slice(if value { b" = true\n" } else { b" = false\n" });
    }
    Some(canonical)
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
