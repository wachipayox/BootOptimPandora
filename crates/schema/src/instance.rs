use std::{path::Path, sync::Arc};

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use ustr::Ustr;
use uuid::Uuid;

use crate::{
    curseforge::CurseforgeReleaseType,
    fabric_loader_manifest::FabricLoaderManifest,
    forge::{ForgeMavenManifest, NeoforgeMavenManifest, VersionFragment},
    loader::Loader,
    modrinth::ModrinthVersionType,
};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct InstanceConfiguration {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home_artwork: Option<std::path::PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home_logo: Option<std::path::PathBuf>,
    pub minecraft_version: Ustr,
    pub loader: Loader,
    #[serde(default, skip_serializing_if = "crate::skip_if_none")]
    pub preferred_loader_version: Option<Ustr>,
    #[serde(default, deserialize_with = "crate::try_deserialize")]
    pub update_channel: UpdateChannel,
    #[serde(
        default,
        deserialize_with = "crate::try_deserialize",
        skip_serializing_if = "crate::skip_if_none"
    )]
    pub preferred_account: Option<Uuid>,
    #[serde(
        default,
        deserialize_with = "crate::try_deserialize",
        skip_serializing_if = "is_default_memory_configuration"
    )]
    pub memory: Option<InstanceMemoryConfiguration>,
    #[serde(
        default,
        deserialize_with = "crate::try_deserialize",
        skip_serializing_if = "is_default_wrapper_command_configuration"
    )]
    pub wrapper_command: Option<InstanceWrapperCommandConfiguration>,
    #[serde(
        default,
        deserialize_with = "crate::try_deserialize",
        skip_serializing_if = "is_default_jvm_flags_configuration"
    )]
    pub jvm_flags: Option<InstanceJvmFlagsConfiguration>,
    #[serde(
        default,
        deserialize_with = "crate::try_deserialize",
        skip_serializing_if = "is_default_jvm_binary_configuration"
    )]
    pub jvm_binary: Option<InstanceJvmBinaryConfiguration>,
    /// Player choices for the profile's distributed JVM options, by option id. Only what the
    /// player changed is stored; everything else follows the current profile revision.
    #[serde(
        default,
        deserialize_with = "crate::try_deserialize",
        skip_serializing_if = "std::collections::BTreeMap::is_empty"
    )]
    pub profile_jvm_options: std::collections::BTreeMap<String, crate::profile_jvm::ProfileJvmOptionChoice>,
    /// Set when a Java 25 start of a Wachiland global profile exited with an error before it could
    /// have reached the main menu. Later starts of this instance use Java 21 until it is removed.
    #[serde(
        default,
        deserialize_with = "crate::try_deserialize",
        skip_serializing_if = "crate::skip_if_none"
    )]
    pub java25_fallback: Option<Java25Fallback>,
    /// "Optimize launch (Java 25 AOT cache)". `None` means the default: on.
    #[serde(
        default,
        deserialize_with = "crate::try_deserialize",
        skip_serializing_if = "crate::skip_if_none"
    )]
    pub aot_cache: Option<AotCacheSetting>,
    #[serde(
        default,
        deserialize_with = "crate::try_deserialize",
        skip_serializing_if = "is_default_linux_wrapper_configuration"
    )]
    pub linux_wrapper: Option<InstanceLinuxWrapperConfiguration>,
    #[serde(
        default,
        deserialize_with = "crate::try_deserialize",
        skip_serializing_if = "is_default_system_libraries_configuration"
    )]
    pub system_libraries: Option<InstanceSystemLibrariesConfiguration>,
    #[serde(
        default,
        deserialize_with = "crate::try_deserialize",
        skip_serializing_if = "crate::skip_if_none"
    )]
    pub instance_fallback_icon: Option<Ustr>,
    #[serde(default, deserialize_with = "crate::try_deserialize")]
    pub disable_file_syncing: bool,
    #[serde(default, deserialize_with = "crate::try_deserialize")]
    pub show_shader_tab: bool,
    #[serde(default, deserialize_with = "crate::try_deserialize")]
    pub sandbox: bool, // Default sandbox to false when loading old configuration json
    /// Created by `--create-instance` on the command line. Only such instances (and those of test
    /// profiles) can be deleted by `--delete-instance`. Older launchers ignore the field.
    #[serde(
        default,
        deserialize_with = "crate::try_deserialize",
        skip_serializing_if = "crate::skip_if_default"
    )]
    pub created_by_cli: bool,
}

impl InstanceConfiguration {
    pub fn new(minecraft_version: Ustr, loader: Loader) -> Self {
        Self {
            home_artwork: None,
            home_logo: None,
            minecraft_version,
            loader,
            preferred_loader_version: None,
            update_channel: UpdateChannel::default(),
            preferred_account: None,
            memory: None,
            wrapper_command: None,
            jvm_flags: None,
            jvm_binary: None,
            profile_jvm_options: std::collections::BTreeMap::new(),
            java25_fallback: None,
            aot_cache: None,
            linux_wrapper: None,
            system_libraries: None,
            instance_fallback_icon: None,
            disable_file_syncing: false,
            show_shader_tab: false,
            sandbox: false, // todo: for now, off by default. In the future, turn this on by default
            created_by_cli: false,
        }
    }
}

impl InstanceConfiguration {
    pub fn determine_fabric_loader_version(&self, manifest: &FabricLoaderManifest) -> Option<Ustr> {
        if let Some(preferred_version) = self.preferred_loader_version {
            return Some(preferred_version);
        }

        let mut latest_loader_version = manifest.0.iter().find(|v| v.stable);
        if latest_loader_version.is_none() {
            latest_loader_version = manifest.0.first();
        }
        Some(latest_loader_version?.version)
    }

    pub fn determine_neoforge_loader_version(&self, manifest: &NeoforgeMavenManifest) -> Option<Ustr> {
        if let Some(preferred_loader_version) = self.preferred_loader_version {
            return Some(preferred_loader_version);
        }

        let mut minecraft_version_parts = VersionFragment::string_to_parts(self.minecraft_version.as_str());

        // 1.21.5 -> 21.5
        // 25w14craftmine -> 0.25w14craftmine
        // 1.21 -> 21.0
        // 26.1 -> 26.1.0
        if minecraft_version_parts[0] == VersionFragment::String("25w14craftmine".into()) {
            minecraft_version_parts.insert(0, VersionFragment::Number(0))
        } else {
            if minecraft_version_parts.len() < 3 {
                minecraft_version_parts.push(VersionFragment::Number(0))
            }
            if minecraft_version_parts[0] == VersionFragment::Number(1) {
                minecraft_version_parts.remove(0);
            }
        }

        let mut latest_loader_version = None;
        let mut latest_loader_version_parts = Vec::new();
        for version in manifest.0.iter() {
            let parts = VersionFragment::string_to_parts(version);

            if parts.starts_with(&minecraft_version_parts) {
                if parts > latest_loader_version_parts {
                    latest_loader_version_parts = parts;
                    latest_loader_version = Some(version.clone());
                }
            }
        }

        latest_loader_version
    }

    pub fn determine_forge_loader_version(&self, manifest: &ForgeMavenManifest) -> Option<Ustr> {
        if let Some(preferred_loader_version) = self.preferred_loader_version {
            return Some(preferred_loader_version);
        }

        let mut latest_loader_version = None;
        let mut latest_loader_version_parts = Vec::new();

        for loader_version in &manifest.0 {
            let Some((loader_minecraft_version, _)) = loader_version.split_once('-') else {
                continue;
            };
            if loader_minecraft_version != self.minecraft_version {
                continue;
            }

            let parts = VersionFragment::string_to_parts(loader_version);
            if parts > latest_loader_version_parts {
                latest_loader_version_parts = parts;
                latest_loader_version = Some(loader_version.clone());
            }
        }

        latest_loader_version
    }
}

#[derive(Serialize, Deserialize, Debug, Copy, Clone, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    #[default]
    Release,
    Beta,
    Alpha,
}

impl UpdateChannel {
    pub fn modrinth_version_types_with_fallback(self) -> &'static [&'static [ModrinthVersionType]] {
        match self {
            Self::Release => &[
                &[ModrinthVersionType::Release],
                &[ModrinthVersionType::Beta],
                &[ModrinthVersionType::Alpha],
            ],
            Self::Beta => &[
                &[ModrinthVersionType::Release, ModrinthVersionType::Beta],
                &[ModrinthVersionType::Alpha],
            ],
            Self::Alpha => &[&[
                ModrinthVersionType::Release,
                ModrinthVersionType::Beta,
                ModrinthVersionType::Alpha,
            ]],
        }
    }

    pub fn curseforge_release_types_with_fallback(self) -> &'static [&'static [CurseforgeReleaseType]] {
        match self {
            Self::Release => &[
                &[CurseforgeReleaseType::Release],
                &[CurseforgeReleaseType::Beta],
                &[CurseforgeReleaseType::Alpha],
            ],
            Self::Beta => &[
                &[CurseforgeReleaseType::Release, CurseforgeReleaseType::Beta],
                &[CurseforgeReleaseType::Alpha],
            ],
            Self::Alpha => &[&[
                CurseforgeReleaseType::Release,
                CurseforgeReleaseType::Beta,
                CurseforgeReleaseType::Alpha,
            ]],
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Copy, Clone)]
pub struct InstanceMemoryConfiguration {
    pub enabled: bool,
    pub min: u32,
    pub max: u32,
}

impl InstanceMemoryConfiguration {
    pub const DEFAULT_MIN: u32 = 512;
    pub const DEFAULT_MAX: u32 = 4096;
}

impl Default for InstanceMemoryConfiguration {
    fn default() -> Self {
        Self {
            enabled: false,
            min: Self::DEFAULT_MIN,
            max: Self::DEFAULT_MAX,
        }
    }
}

fn is_default_memory_configuration(config: &Option<InstanceMemoryConfiguration>) -> bool {
    // An explicit disabled override retains the last Manual values, including
    // 512/4096 MiB. Omitting it would lose those values after restarting.
    config.is_none()
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct InstanceWrapperCommandConfiguration {
    pub enabled: bool,
    pub flags: Arc<str>,
}

fn is_default_wrapper_command_configuration(config: &Option<InstanceWrapperCommandConfiguration>) -> bool {
    if let Some(config) = config {
        !config.enabled && config.flags.trim_ascii().is_empty()
    } else {
        true
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct InstanceJvmFlagsConfiguration {
    pub enabled: bool,
    pub flags: Arc<str>,
}

fn is_default_jvm_flags_configuration(config: &Option<InstanceJvmFlagsConfiguration>) -> bool {
    if let Some(config) = config {
        !config.enabled && config.flags.trim_ascii().is_empty()
    } else {
        true
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct InstanceJvmBinaryConfiguration {
    pub enabled: bool,
    pub path: Option<Arc<Path>>,
}

/// The player's "Optimize launch" choice for the JDK 25 AOT cache.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct AotCacheSetting {
    pub enabled: bool,
    /// Set when the launcher turned the option off because the JVM rejected the cache.
    #[serde(default, skip_serializing_if = "crate::skip_if_none")]
    pub auto_disabled_reason: Option<String>,
    #[serde(default, skip_serializing_if = "crate::skip_if_none")]
    pub disabled_unix_secs: Option<u64>,
}

impl InstanceConfiguration {
    /// Whether the JDK 25 AOT cache may be used and trained for this instance (on by default).
    pub fn aot_cache_enabled(&self) -> bool {
        self.aot_cache.as_ref().is_none_or(|setting| setting.enabled)
    }
}

/// Why Java 25 was dropped for this instance. Removing the key from the instance configuration
/// makes the next Start try Java 25 again.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Java25Fallback {
    pub reason: String,
    pub recorded_unix_secs: u64,
}

fn is_default_jvm_binary_configuration(config: &Option<InstanceJvmBinaryConfiguration>) -> bool {
    if let Some(config) = config {
        !config.enabled && config.path.is_none()
    } else {
        true
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct InstanceLinuxWrapperConfiguration {
    #[serde(default, deserialize_with = "crate::try_deserialize")]
    pub use_mangohud: bool,
    #[serde(default, deserialize_with = "crate::try_deserialize")]
    pub use_gamemode: bool,
    #[serde(default = "crate::default_true", deserialize_with = "crate::try_deserialize")]
    pub use_discrete_gpu: bool,
    #[serde(default, deserialize_with = "crate::try_deserialize")]
    pub disable_gl_threaded_optimizations: bool,
}

impl Default for InstanceLinuxWrapperConfiguration {
    fn default() -> Self {
        Self {
            use_mangohud: false,
            use_gamemode: false,
            use_discrete_gpu: true,
            disable_gl_threaded_optimizations: false,
        }
    }
}

fn is_default_linux_wrapper_configuration(config: &Option<InstanceLinuxWrapperConfiguration>) -> bool {
    if let Some(config) = config {
        !config.use_mangohud
            && !config.use_gamemode
            && config.use_discrete_gpu
            && !config.disable_gl_threaded_optimizations
    } else {
        true
    }
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct InstanceSystemLibrariesConfiguration {
    pub override_glfw: bool,
    pub glfw: LwjglLibraryPath,
    pub override_openal: bool,
    pub openal: LwjglLibraryPath,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub enum LwjglLibraryPath {
    #[default]
    Auto,
    AutoPreferred(Arc<Path>),
    Explicit(Arc<Path>),
}

fn is_default_system_libraries_configuration(config: &Option<InstanceSystemLibrariesConfiguration>) -> bool {
    if let Some(config) = config {
        matches!(config.glfw, LwjglLibraryPath::Auto) && matches!(config.openal, LwjglLibraryPath::Auto)
    } else {
        true
    }
}

impl LwjglLibraryPath {
    pub fn get_or_auto(self, auto: &Option<Arc<Path>>) -> Option<Arc<Path>> {
        match self {
            LwjglLibraryPath::Auto => auto.clone(),
            LwjglLibraryPath::AutoPreferred(preferred) => {
                if preferred.exists() {
                    Some(preferred)
                } else {
                    auto.clone()
                }
            },
            LwjglLibraryPath::Explicit(path) => Some(path),
        }
    }
}

pub static AUTO_LIBRARY_PATH_GLFW: Lazy<Option<Arc<Path>>> = Lazy::new(|| get_shared_library_path_for_name("glfw"));
pub static AUTO_LIBRARY_PATH_OPENAL: Lazy<Option<Arc<Path>>> = Lazy::new(|| get_shared_library_path_for_name("openal"));

#[cfg(not(unix))]
fn get_shared_library_path_for_name(name: &str) -> Option<Arc<Path>> {
    None
}

#[cfg(unix)]
fn get_shared_library_path_for_name(name: &str) -> Option<Arc<Path>> {
    let filename = format!("{}{}{}", std::env::consts::DLL_PREFIX, name, std::env::consts::DLL_SUFFIX);

    let search_paths = &[
        "/lib/",
        "/lib64/",
        "/usr/lib/",
        "/usr/lib64/",
        "/usr/local/lib/",
        #[cfg(target_os = "macos")]
        "/opt/homebrew/lib/",
    ];

    for search_path in search_paths {
        let path = Path::new(search_path).join(&filename);
        if path.exists() {
            return Some(path.into());
        }
    }

    None
}

/// UI recommendation range. The instance's `memory.enabled` persists the mode:
/// true is Manual, false (or no memory override) is Recommended. Disabled
/// values are retained so returning to Manual restores the player's settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryRecommendation {
    pub total_mib: Option<u64>,
    pub minimum_mib: u32,
    pub maximum_mib: u32,
    pub recommended_mib: u32,
}
impl Default for MemoryRecommendation {
    fn default() -> Self {
        Self::for_total_mib(None)
    }
}
impl MemoryRecommendation {
    pub fn for_total_mib(total: Option<u64>) -> Self {
        let Some(total) = total.filter(|n| *n >= 1024) else {
            return Self {
                total_mib: None,
                minimum_mib: 2048,
                maximum_mib: 8192,
                recommended_mib: 4096,
            };
        };
        let align = |n: u64| n.saturating_add(128) / 256 * 256;
        let ceiling = total.saturating_sub(1024).max(512).min(8192);
        let minimum = align(total / 4).clamp(512, 4096).min(ceiling);
        let maximum = align(total / 2).clamp(512, 8192).min(ceiling).max(minimum);
        let recommended = align(total.saturating_mul(3) / 8).clamp(minimum, maximum);
        Self {
            total_mib: Some(total),
            minimum_mib: minimum as u32,
            maximum_mib: maximum as u32,
            recommended_mib: recommended as u32,
        }
    }
    /// Resolve a launch copy without rewriting the persisted Manual values.
    pub fn effective_memory(self, stored: Option<InstanceMemoryConfiguration>) -> InstanceMemoryConfiguration {
        stored.filter(|m| m.enabled).unwrap_or(InstanceMemoryConfiguration {
            enabled: true,
            min: InstanceMemoryConfiguration::DEFAULT_MIN,
            max: self.recommended_mib,
        })
    }
}
#[cfg(test)]
mod memory_recommendation_tests {
    use super::*;
    #[test]
    fn sixteen_gib_matches_the_design_and_small_machines_keep_os_headroom() {
        let m = MemoryRecommendation::for_total_mib(Some(16 * 1024));
        assert_eq!((m.minimum_mib, m.recommended_mib, m.maximum_mib), (4096, 6144, 8192));
        for total in [1024, 2048, 4096, 8192, 16310, 32768, 262144] {
            let m = MemoryRecommendation::for_total_mib(Some(total));
            assert!(m.minimum_mib <= m.recommended_mib && m.recommended_mib <= m.maximum_mib);
            assert!(m.recommended_mib <= 8192 && m.recommended_mib < total as u32);
        }
    }
    #[test]
    fn manual_values_survive_recommended_mode_and_unknown_ram_uses_four_gib() {
        let m = MemoryRecommendation::for_total_mib(Some(16384));
        let stored = InstanceMemoryConfiguration {
            enabled: false,
            min: 1024,
            max: 7168,
        };
        assert_eq!(m.effective_memory(Some(stored)).max, 6144);
        assert_eq!(stored.max, 7168);
        assert_eq!(
            m.effective_memory(Some(InstanceMemoryConfiguration {
                enabled: true,
                ..stored
            }))
            .max,
            7168
        );
        assert_eq!(MemoryRecommendation::default().effective_memory(None).max, 4096);
        let mut configuration = InstanceConfiguration::new("1.21.1".into(), Loader::Vanilla);
        configuration.memory = Some(InstanceMemoryConfiguration::default());
        let saved = serde_json::to_value(&configuration).unwrap();
        assert!(saved.get("memory").is_some());
        let restored: InstanceConfiguration = serde_json::from_value(saved).unwrap();
        let stored = restored.memory.unwrap();
        assert!(!stored.enabled);
        assert_eq!(stored.max, 4096);
    }
}
