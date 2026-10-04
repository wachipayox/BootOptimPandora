//! Paths under an instance's .minecraft excluded from profile inheritance and comparison.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct IgnoredProfilePaths(pub Vec<String>);

impl Default for IgnoredProfilePaths {
    fn default() -> Self {
        Self::normalized(Self::recommended_defaults()).expect("valid built-in ignored paths")
    }
}

impl IgnoredProfilePaths {
    /// `*` excludes roots outside the small set of conventional modpack content.
    /// It can be removed from Settings like any other default rule.
    pub fn recommended_defaults() -> Vec<String> {
        [
            "*", "mods/.connector", "mods/mcef-cache", "mods/mcef-libraries",
            ".analogaudio", ".bootoptim", ".mixin.out", ".sable", ".voxy", "unilog",
        ].into_iter().map(str::to_owned).collect()
    }

    /// Both `foo` and `/foo` mean a path relative to .minecraft.
    pub fn normalized(paths: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut result = BTreeSet::new();
        for raw in paths {
            let raw = raw.trim();
            let path = raw.strip_prefix('/').unwrap_or(raw).trim_end_matches('/');
            if path.is_empty()
                || path.contains('\\')
                || path.contains(':')
                || (path.contains('*') && path != "*")
                || path.chars().any(char::is_control)
                || path.split('/').any(|part| part.is_empty() || part == "." || part == "..")
            {
                return Err(format!("Ruta no válida desde .minecraft: {raw}"));
            }
            let path = path.to_ascii_lowercase();
            let path = if path.starts_with("mods/") && path.ends_with(".jar.disabled") {
                path.strip_suffix(".disabled").unwrap().to_owned()
            } else {
                path
            };
            result.insert(path);
        }
        Ok(Self(result.into_iter().collect()))
    }

    pub fn contains(&self, relative: &str) -> bool {
        let candidate = relative.replace('\\', "/").trim_start_matches('/').to_ascii_lowercase();
        let canonical = candidate.strip_suffix(".jar.disabled")
            .filter(|_| candidate.starts_with("mods/"))
            .map(|prefix| format!("{prefix}.jar"));
        self.0.iter().any(|path| {
            let path = path.trim_start_matches('/');
            if path == "*" {
                return !is_conventional_modpack_root(candidate.split('/').next().unwrap_or(""));
            }
            [Some(candidate.as_str()), canonical.as_deref()].into_iter().flatten().any(|candidate| {
                candidate == path || candidate.strip_prefix(path).is_some_and(|rest| rest.starts_with('/'))
            })
        })
    }
}

fn is_conventional_modpack_root(root: &str) -> bool {
    matches!(root,
        "mods" | "config" | "defaultconfigs" | "resourcepacks" | "shaderpacks"
        | "datapacks" | "kubejs" | "scripts" | "fancymenu_data"
        | "openloader" | "global_packs" | "patchouli_books"
        | "options.txt" | "servers.dat" | "optionsof.txt"
        | "optionsshaders.txt" | "optionsviveprofiles.txt"
    )
}
