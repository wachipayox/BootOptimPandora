//! Paths under an instance's .minecraft excluded from profile inheritance and comparison.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct IgnoredProfilePaths(pub Vec<String>);

impl Default for IgnoredProfilePaths {
    fn default() -> Self {
        Self(vec![
            "mods/.connector".into(),
            ".analogaudio".into(),
            ".bootoptim".into(),
            "mods/mcef-cache".into(),
            ".mixin.out".into(),
            "unilog".into(),
        ])
    }
}

impl IgnoredProfilePaths {
    /// Both `foo` and `/foo` mean a path relative to .minecraft.
    pub fn normalized(paths: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut result = BTreeSet::new();
        for raw in paths {
            let raw = raw.trim();
            let path = raw.strip_prefix('/').unwrap_or(raw).trim_end_matches('/');
            if path.is_empty()
                || path.contains('\\')
                || path.contains(':')
                || path.chars().any(char::is_control)
                || path.split('/').any(|part| part.is_empty() || part == "." || part == "..")
            {
                return Err(format!("Ruta no válida desde .minecraft: {raw}"));
            }
            result.insert(path.to_ascii_lowercase());
        }
        Ok(Self(result.into_iter().collect()))
    }

    pub fn contains(&self, relative: &str) -> bool {
        let candidate = relative.replace('\\', "/").trim_start_matches('/').to_ascii_lowercase();
        self.0.iter().any(|path| {
            let path = path.trim_start_matches('/');
            candidate == path || candidate.strip_prefix(path).is_some_and(|rest| rest.starts_with('/'))
        })
    }
}
