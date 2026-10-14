//! Merge signed per-setting config rules without taking ownership of the whole file.
//!
//! Callers persist `initialized_default_once` in the profile's private state and
//! retain recoverable copies for every key listed in `changed_enforced` before
//! writing `bytes` over an existing live file.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use thiserror::Error;
use toml_edit::{Array, DocumentMut, Item, Table, Value};

use crate::distribution::ManifestConfigSetting;

pub const MAX_CONFIG_SETTING_BYTES: usize = 1 << 20;

#[derive(Debug, Error)]
pub enum ConfigSettingError {
    #[error("invalid TOML config: {0}")]
    Parse(#[from] toml_edit::TomlError),
    #[error("config file is not UTF-8")]
    InvalidUtf8,
    #[error("unsupported config setting identity: {0}")]
    InvalidKey(String),
    #[error("TOML key path collides with a non-table value: {0}")]
    KeyCollision(String),
    #[error("unsupported value for config setting {0}")]
    InvalidValue(String),
    #[error("config file exceeds the 1 MiB structured-edit limit")]
    FileTooLarge,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigMergeResult {
    pub bytes: Vec<u8>,
    pub initialized_default_once: BTreeSet<String>,
    pub changed_enforced: Vec<String>,
}

pub fn config_setting_signatures(
    rules: &[ManifestConfigSetting],
) -> Result<BTreeMap<String, String>, serde_json::Error> {
    let mut signatures = BTreeMap::new();
    for rule in rules {
        let bytes = serde_json::to_vec(rule)?;
        signatures.insert(rule.identity(), hex::encode(Sha256::digest(bytes)));
    }
    Ok(signatures)
}

/// Apply settings for a single config path. `live_contents` is `None` only on
/// first installation. Existing unselected options and comments are preserved.
pub fn merge_toml_settings(
    path: &str,
    published_contents: &[u8],
    live_contents: Option<&[u8]>,
    initialized_default_once: &BTreeSet<String>,
    rules: &[ManifestConfigSetting],
) -> Result<ConfigMergeResult, ConfigSettingError> {
    let source = live_contents.unwrap_or(published_contents);
    let source = std::str::from_utf8(source).map_err(|_| ConfigSettingError::InvalidUtf8)?;
    let mut document = source.parse::<DocumentMut>()?;
    let mut initialized = initialized_default_once.clone();
    let mut changed_enforced = Vec::new();

    for rule in rules.iter().filter(|rule| rule.path == path) {
        if rule.format != "toml" || !valid_key_path(&rule.key) {
            return Err(ConfigSettingError::InvalidKey(rule.key.clone()));
        }
        let identity = rule.identity();
        let desired = json_to_toml(&rule.value).ok_or_else(|| ConfigSettingError::InvalidValue(rule.key.clone()))?;
        match rule.policy.as_str() {
            "enforced" => {
                let desired_text = desired.to_string();
                let previous_text = get_value(document.as_table(), &rule.key).map(ToString::to_string);
                if previous_text.as_deref() != Some(desired_text.as_str()) {
                    if live_contents.is_some() {
                        changed_enforced.push(rule.key.clone());
                    }
                    set_value(document.as_table_mut(), &rule.key, desired, &rule.key)?;
                }
            },
            "default_once" => {
                if !initialized.contains(&identity) {
                    // Preserve an existing live key on migration/upgrade. A new
                    // profile gets the published default even when the source
                    // TOML already has a value at this key.
                    if live_contents.is_none() || get_value(document.as_table(), &rule.key).is_none() {
                        set_value(document.as_table_mut(), &rule.key, desired, &rule.key)?;
                    }
                    initialized.insert(identity);
                }
            },
            _ => return Err(ConfigSettingError::InvalidValue(rule.key.clone())),
        }
    }

    Ok(ConfigMergeResult {
        bytes: document.to_string().into_bytes(),
        initialized_default_once: initialized,
        changed_enforced,
    })
}

/// Apply signed rules for TOML, Java properties, or line-oriented text.
pub fn merge_config_settings(
    path: &str,
    published_contents: &[u8],
    live_contents: Option<&[u8]>,
    initialized_default_once: &BTreeSet<String>,
    rules: &[ManifestConfigSetting],
) -> Result<ConfigMergeResult, ConfigSettingError> {
    if published_contents.len() > MAX_CONFIG_SETTING_BYTES
        || live_contents.is_some_and(|bytes| bytes.len() > MAX_CONFIG_SETTING_BYTES)
    {
        return Err(ConfigSettingError::FileTooLarge);
    }
    let format = rules.iter().find(|rule| rule.path == path).map(|rule| rule.format.as_str());
    match format {
        None => Ok(ConfigMergeResult {
            bytes: live_contents.unwrap_or(published_contents).to_vec(),
            initialized_default_once: initialized_default_once.clone(),
            changed_enforced: Vec::new(),
        }),
        Some("toml") => merge_toml_settings(path, published_contents, live_contents, initialized_default_once, rules),
        Some("properties") => {
            merge_properties_settings(path, published_contents, live_contents, initialized_default_once, rules)
        },
        Some("text_lines") => {
            merge_text_line_settings(path, published_contents, live_contents, initialized_default_once, rules)
        },
        Some(_) => Err(ConfigSettingError::InvalidValue("unsupported config format".into())),
    }
}

fn merge_properties_settings(
    path: &str,
    published_contents: &[u8],
    live_contents: Option<&[u8]>,
    initialized_default_once: &BTreeSet<String>,
    rules: &[ManifestConfigSetting],
) -> Result<ConfigMergeResult, ConfigSettingError> {
    let source = live_contents.unwrap_or(published_contents);
    let source = std::str::from_utf8(source).map_err(|_| ConfigSettingError::InvalidUtf8)?;
    let newline = if source.contains("\r\n") { "\r\n" } else { "\n" };
    let trailing_newline = source.ends_with('\n');
    let mut lines: Vec<String> = source
        .split_terminator('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
        .collect();
    if source.is_empty() {
        lines.clear();
    }
    let mut initialized = initialized_default_once.clone();
    let mut changed_enforced = Vec::new();

    for rule in rules.iter().filter(|rule| rule.path == path && rule.format == "properties") {
        if !valid_properties_key(&rule.key) {
            return Err(ConfigSettingError::InvalidKey(rule.key.clone()));
        }
        let desired = rule.value.as_str().ok_or_else(|| ConfigSettingError::InvalidValue(rule.key.clone()))?;
        let escaped_desired = escape_properties_value(desired);
        let identity = rule.identity();
        if lines.iter().filter(|line| property_key(line) == Some(rule.key.as_str())).count() > 1 {
            return Err(ConfigSettingError::InvalidKey(rule.key.clone()));
        }
        let line_index = lines.iter().rposition(|line| property_key(line) == Some(rule.key.as_str()));
        let should_apply = match rule.policy.as_str() {
            "enforced" => true,
            "default_once" => !initialized.contains(&identity),
            _ => return Err(ConfigSettingError::InvalidValue(rule.key.clone())),
        };
        if should_apply {
            if let Some(index) = line_index {
                if rule.policy == "enforced" && property_value(&lines[index]) != Some(escaped_desired.as_str()) {
                    if live_contents.is_some() {
                        changed_enforced.push(rule.key.clone());
                    }
                    let value_start = property_value_start(&lines[index]);
                    if property_has_separator(&lines[index]) {
                        lines[index].truncate(value_start);
                    } else {
                        lines[index].push('=');
                    }
                    lines[index].push_str(&escaped_desired);
                } else if rule.policy == "default_once" && live_contents.is_none() {
                    let value_start = property_value_start(&lines[index]);
                    if property_has_separator(&lines[index]) {
                        lines[index].truncate(value_start);
                    } else {
                        lines[index].push('=');
                    }
                    lines[index].push_str(&escaped_desired);
                }
            } else if rule.policy == "enforced" || rule.policy == "default_once" {
                if rule.policy == "enforced" && live_contents.is_some() {
                    changed_enforced.push(rule.key.clone());
                }
                lines.push(format!("{}={}", rule.key, escaped_desired));
            }
            if rule.policy == "default_once" {
                initialized.insert(identity);
            }
        }
    }
    Ok(ConfigMergeResult {
        bytes: join_lines(&lines, newline, trailing_newline).into_bytes(),
        initialized_default_once: initialized,
        changed_enforced,
    })
}

fn merge_text_line_settings(
    path: &str,
    published_contents: &[u8],
    live_contents: Option<&[u8]>,
    initialized_default_once: &BTreeSet<String>,
    rules: &[ManifestConfigSetting],
) -> Result<ConfigMergeResult, ConfigSettingError> {
    let source = live_contents.unwrap_or(published_contents);
    let source = std::str::from_utf8(source).map_err(|_| ConfigSettingError::InvalidUtf8)?;
    let newline = if source.contains("\r\n") { "\r\n" } else { "\n" };
    let trailing_newline = source.ends_with('\n');
    let mut lines: Vec<String> = source
        .split_terminator('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
        .collect();
    if source.is_empty() {
        lines.clear();
    }
    let mut initialized = initialized_default_once.clone();
    let mut changed_enforced = Vec::new();

    for rule in rules.iter().filter(|rule| rule.path == path && rule.format == "text_lines") {
        let line_number = rule
            .key
            .strip_prefix("line:")
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0)
            .ok_or_else(|| ConfigSettingError::InvalidKey(rule.key.clone()))?;
        let desired = rule.value.as_str().ok_or_else(|| ConfigSettingError::InvalidValue(rule.key.clone()))?;
        let identity = rule.identity();
        let should_apply = match rule.policy.as_str() {
            "enforced" => true,
            "default_once" => !initialized.contains(&identity),
            _ => return Err(ConfigSettingError::InvalidValue(rule.key.clone())),
        };
        if should_apply {
            let index = line_number - 1;
            if index >= lines.len() {
                return Err(ConfigSettingError::InvalidKey(rule.key.clone()));
            }
            if rule.policy == "enforced" {
                if lines[index] != desired {
                    if live_contents.is_some() {
                        changed_enforced.push(rule.key.clone());
                    }
                    lines[index] = desired.to_owned();
                }
            } else {
                // First install takes the published default; an existing user
                // line remains user-owned even if its marker is absent.
                if live_contents.is_none() {
                    lines[index] = desired.to_owned();
                }
                initialized.insert(identity);
            }
        }
    }
    Ok(ConfigMergeResult {
        bytes: join_lines(&lines, newline, trailing_newline).into_bytes(),
        initialized_default_once: initialized,
        changed_enforced,
    })
}

fn join_lines(lines: &[String], newline: &str, trailing_newline: bool) -> String {
    let mut joined = lines.join(newline);
    if trailing_newline && !joined.is_empty() {
        joined.push_str(newline);
    }
    joined
}

fn valid_properties_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 256
        && key.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn property_key(line: &str) -> Option<&str> {
    let indent = line.len() - line.trim_start().len();
    let trimmed = &line[indent..];
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
        return None;
    }
    let mut escaped = false;
    let separator = trimmed.char_indices().find_map(|(index, character)| {
        if escaped {
            escaped = false;
            return None;
        }
        if character == '\\' {
            escaped = true;
            return None;
        }
        if character == '=' || character == ':' || character.is_whitespace() {
            Some(index)
        } else {
            None
        }
    });
    let key = trimmed[..separator.unwrap_or(trimmed.len())].trim();
    if valid_properties_key(key) { Some(key) } else { None }
}

fn property_value_start(line: &str) -> usize {
    let indent = line.len() - line.trim_start().len();
    let content = &line[indent..];
    let mut escaped = false;
    let separator = content.char_indices().find_map(|(index, character)| {
        if escaped {
            escaped = false;
            return None;
        }
        if character == '\\' {
            escaped = true;
            return None;
        }
        if character == '=' || character == ':' || character.is_whitespace() {
            Some((index, character))
        } else {
            None
        }
    });
    let Some((index, separator)) = separator else {
        return line.len();
    };
    let mut cursor = index;
    while cursor < content.len() && content[cursor..].chars().next().is_some_and(char::is_whitespace) {
        cursor += content[cursor..].chars().next().unwrap().len_utf8();
    }
    if separator != ' ' && cursor < content.len() && matches!(content[cursor..].chars().next(), Some('=' | ':')) {
        cursor += content[cursor..].chars().next().unwrap().len_utf8();
    } else if cursor < content.len() && matches!(content[cursor..].chars().next(), Some('=' | ':')) {
        cursor += content[cursor..].chars().next().unwrap().len_utf8();
    }
    while cursor < content.len() && content[cursor..].chars().next().is_some_and(char::is_whitespace) {
        cursor += content[cursor..].chars().next().unwrap().len_utf8();
    }
    indent + cursor
}

fn property_has_separator(line: &str) -> bool {
    let content = line.trim_start();
    let mut escaped = false;
    content.chars().any(|character| {
        if escaped {
            escaped = false;
            return false;
        }
        if character == '\\' {
            escaped = true;
            return false;
        }
        character == '=' || character == ':' || character.is_whitespace()
    })
}

fn property_value(line: &str) -> Option<&str> {
    let start = property_value_start(line);
    (start <= line.len()).then(|| &line[start..])
}

fn escape_properties_value(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for (index, character) in value.chars().enumerate() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '\t' => escaped.push_str("\\t"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\u{000c}' => escaped.push_str("\\f"),
            ' ' if index == 0 => escaped.push_str("\\ "),
            _ => escaped.push(character),
        }
    }
    escaped
}

fn valid_key_path(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 256
        && key.split('.').all(|part| {
            !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        })
}

fn get_value<'a>(table: &'a Table, key: &str) -> Option<&'a Value> {
    let mut parts = key.split('.');
    let first = parts.next()?;
    let mut item = table.get(first)?;
    for part in parts {
        item = item.as_table()?.get(part)?;
    }
    item.as_value()
}

fn set_value(table: &mut Table, key: &str, value: Value, full_key: &str) -> Result<(), ConfigSettingError> {
    let mut parts = key.split('.').peekable();
    let mut current = table;
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            current.insert(part, Item::Value(value));
            return Ok(());
        }
        if !current.contains_key(part) {
            current.insert(part, Item::Table(Table::new()));
        }
        let item = current
            .get_mut(part)
            .ok_or_else(|| ConfigSettingError::KeyCollision(full_key.to_owned()))?;
        current = item.as_table_mut().ok_or_else(|| ConfigSettingError::KeyCollision(full_key.to_owned()))?;
    }
    Err(ConfigSettingError::InvalidKey(full_key.to_owned()))
}

fn json_to_toml(value: &JsonValue) -> Option<Value> {
    match value {
        JsonValue::Null | JsonValue::Object(_) => None,
        JsonValue::Bool(value) => Some(Value::from(*value)),
        JsonValue::Number(value) => {
            if let Some(number) = value.as_i64() {
                Some(Value::from(number))
            } else if let Some(number) = value.as_u64() {
                i64::try_from(number).ok().map(Value::from)
            } else {
                value.as_f64().map(Value::from)
            }
        },
        JsonValue::String(value) => Some(Value::from(value.as_str())),
        JsonValue::Array(values) => {
            let mut array = Array::new();
            for value in values {
                array.push(json_to_toml(value)?);
            }
            Some(Value::Array(array))
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::distribution::ManifestConfigSetting;

    fn rule(path: &str, format: &str, key: &str, value: JsonValue, policy: &str) -> ManifestConfigSetting {
        ManifestConfigSetting {
            path: path.to_owned(),
            format: format.to_owned(),
            key: key.to_owned(),
            value,
            policy: policy.to_owned(),
        }
    }

    #[test]
    fn toml_rules_preserve_unselected_values_and_track_default_once() {
        let path = "config/example.toml";
        let first_rules = vec![
            rule(path, "toml", "video.render_distance", JsonValue::from(12), "enforced"),
            rule(path, "toml", "video.smooth_lighting", JsonValue::from(false), "default_once"),
        ];
        let initial = merge_config_settings(
            path,
            b"[video]\nrender_distance = 8\nsmooth_lighting = true\nparticles = \"all\"\n",
            None,
            &BTreeSet::new(),
            &first_rules,
        )
        .unwrap();
        let initial_text = String::from_utf8(initial.bytes.clone()).unwrap();
        assert!(initial_text.contains("render_distance = 12"));
        assert!(initial_text.contains("smooth_lighting = false"));
        assert!(initial_text.contains("particles = \"all\""));
        assert_eq!(initial.initialized_default_once.len(), 1);

        let updated = merge_config_settings(
            path,
            b"[video]\nrender_distance = 12\nsmooth_lighting = false\nparticles = \"all\"\n",
            Some(b"[video]\nrender_distance = 9\nsmooth_lighting = true\nparticles = \"minimal\"\n"),
            &initial.initialized_default_once,
            &[
                rule(path, "toml", "video.render_distance", JsonValue::from(16), "enforced"),
                rule(path, "toml", "video.smooth_lighting", JsonValue::from(false), "default_once"),
            ],
        )
        .unwrap();
        let updated_text = String::from_utf8(updated.bytes).unwrap();
        assert!(updated_text.contains("render_distance = 16"));
        assert!(updated_text.contains("smooth_lighting = true"));
        assert!(updated_text.contains("particles = \"minimal\""));
        assert_eq!(updated.changed_enforced, ["video.render_distance"]);
    }

    #[test]
    fn properties_and_text_rules_preserve_unselected_lines() {
        let properties = merge_config_settings(
            "config/options.properties",
            b"# Keep this comment\nmode=fast\nname=Player\n",
            Some(b"# Local comment\nmode=slow\nname=Ana\n"),
            &BTreeSet::new(),
            &[rule(
                "config/options.properties",
                "properties",
                "mode",
                JsonValue::from("fast mode"),
                "enforced",
            )],
        )
        .unwrap();
        let properties_text = String::from_utf8(properties.bytes).unwrap();
        assert!(properties_text.contains("# Local comment"));
        assert!(properties_text.contains("mode=fast mode"));
        assert!(properties_text.contains("name=Ana"));

        let text = merge_config_settings(
            "config/allowlist.txt",
            b"alpha\nbeta\ngamma\n",
            Some(b"local-alpha\nlocal-beta\nlocal-gamma\n"),
            &BTreeSet::new(),
            &[rule(
                "config/allowlist.txt",
                "text_lines",
                "line:2",
                JsonValue::from("required-beta"),
                "enforced",
            )],
        )
        .unwrap();
        assert_eq!(text.bytes, b"local-alpha\nrequired-beta\nlocal-gamma\n");
    }

    #[test]
    fn duplicate_properties_key_is_rejected_as_ambiguous() {
        let result = merge_config_settings(
            "config/options.properties",
            b"mode=one\nmode=two\n",
            None,
            &BTreeSet::new(),
            &[rule(
                "config/options.properties",
                "properties",
                "mode",
                JsonValue::from("safe"),
                "enforced",
            )],
        );
        assert!(matches!(result, Err(ConfigSettingError::InvalidKey(_))));
    }
}
