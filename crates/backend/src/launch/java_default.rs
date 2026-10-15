//! Java 25 by default for Wachiland global-profile instances on PC.
//!
//! The choice is taken at Start from the committed profile identity and the instance settings;
//! nothing here contacts the distribution service. Java 25 is Mojang's runtime component for the
//! platform (`java-runtime-epsilon`), downloaded through the existing hash-verified path. If a
//! Java 25 start exits with an error within [`EARLY_EXIT_WINDOW`], the instance records a fallback
//! and later Starts use Java 21 until the record is removed.

use std::{collections::HashMap, time::Duration};

use schema::java_runtimes::JavaRuntimeComponent;
use ustr::Ustr;

/// Mechanism id in the `[WBOMBO-COMPAT]` log lines.
pub(crate) const MECHANISM: &str = "java25-default";
pub(crate) const JAVA_MAJOR: u32 = 25;
/// Global profile ids of Wachiland (`profile_wachiland-elite`, `profile_wachiland-elite-beta`, ...).
pub(crate) const WACHILAND_PROFILE_PREFIX: &str = "profile_wachiland-";
/// Measured on the reference PC: the main menu was reached after about 69 s with Java 25. An error
/// exit inside this window cannot come from a run that already reached the menu. Errors later than
/// this are not observed by the launcher, which does not read the game's output by default.
pub(crate) const EARLY_EXIT_WINDOW: Duration = Duration::from_secs(15);

/// What the Start of one instance should use for the game JVM.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Choice<'a> {
    /// Java 21 through the existing path (Mojang's version runtime, custom binary, forced Java).
    Java21,
    /// Java 25 requested; the launcher still uses Java 21 if the manifest has no such component.
    Java25,
    /// Wachiland profile, but an earlier Java 25 start failed. Carries the saved reason.
    FallbackSaved(&'a str),
}

pub(crate) struct Inputs<'a> {
    /// Profile id of the committed global revision applied to the instance, if any.
    pub profile_id: Option<&'a str>,
    /// The player enabled a custom Java for this instance: their choice always wins.
    pub jvm_binary_enabled: bool,
    /// `FORCE_EXTERNAL_JAVA` is set (developer override).
    pub force_external_java: bool,
    pub saved_fallback_reason: Option<&'a str>,
}

pub(crate) fn choose<'a>(inputs: &Inputs<'a>) -> Choice<'a> {
    if inputs.jvm_binary_enabled || inputs.force_external_java {
        return Choice::Java21;
    }
    if !inputs.profile_id.is_some_and(|id| id.starts_with(WACHILAND_PROFILE_PREFIX)) {
        return Choice::Java21;
    }
    match inputs.saved_fallback_reason {
        Some(reason) => Choice::FallbackSaved(reason),
        None => Choice::Java25,
    }
}

/// Major version of a Mojang runtime version name, such as `25.0.1` or `21.0.7`.
pub(crate) fn major_version(version_name: &str) -> Option<u32> {
    version_name.split('.').next()?.parse().ok()
}

/// The first Java 25 component of a platform, taken in component-name order so the choice is
/// stable. Returns `None` when the manifest has no such component; the caller then uses Java 21.
pub(crate) fn java25_component(
    components: &HashMap<Ustr, Vec<JavaRuntimeComponent>>,
) -> Option<(Ustr, &JavaRuntimeComponent)> {
    let mut names: Vec<Ustr> = components.keys().copied().collect();
    names.sort_unstable_by(|a, b| a.as_str().cmp(b.as_str()));
    names.into_iter().find_map(|name| {
        let component = components.get(&name)?.first()?;
        (major_version(component.version.name.as_str()) == Some(JAVA_MAJOR)).then_some((name, component))
    })
}

/// The reason to record when a Java 25 start ended with an error inside the early window, or
/// `None` for a clean exit or a run that lasted at least [`EARLY_EXIT_WINDOW`].
pub(crate) fn early_exit_reason(exit_code: Option<i32>, lived: Duration) -> Option<String> {
    if exit_code == Some(0) || lived >= EARLY_EXIT_WINDOW {
        return None;
    }
    Some(match exit_code {
        Some(code) => format!("exit-code={code} after {}s", lived.as_secs()),
        None => format!("signal after {}s", lived.as_secs()),
    })
}

pub(crate) fn log_active(version: &str) {
    log::info!("[WBOMBO-COMPAT] ACTIVE project=Pandora mechanism={MECHANISM} mod=java@{version}");
}

pub(crate) fn log_fallback(version: &str, reason: &str) {
    log::warn!("[WBOMBO-COMPAT] FALLBACK project=Pandora mechanism={MECHANISM} mod=java@{version} reason={reason}");
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};
    use schema::java_runtimes::{
        JavaRuntimeComponent, JavaRuntimeComponentAvailability, JavaRuntimeComponentManifestLink,
        JavaRuntimeComponentVersion,
    };

    use super::*;

    fn inputs(profile_id: Option<&str>) -> Inputs<'_> {
        Inputs {
            profile_id,
            jvm_binary_enabled: false,
            force_external_java: false,
            saved_fallback_reason: None,
        }
    }

    fn component(name: &str) -> JavaRuntimeComponent {
        JavaRuntimeComponent {
            availability: JavaRuntimeComponentAvailability {
                group: 1,
                progress: 100,
            },
            manifest: JavaRuntimeComponentManifestLink {
                sha1: Ustr::from("0000000000000000000000000000000000000000"),
                size: 1,
                url: Ustr::from("https://example.invalid/manifest.json"),
            },
            version: JavaRuntimeComponentVersion {
                name: Ustr::from(name),
                released: DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
            },
        }
    }

    #[test]
    fn wachiland_global_profiles_default_to_java25() {
        assert_eq!(choose(&inputs(Some("profile_wachiland-elite"))), Choice::Java25);
        assert_eq!(choose(&inputs(Some("profile_wachiland-elite-beta"))), Choice::Java25);
    }

    #[test]
    fn other_profiles_and_vanilla_keep_java21() {
        assert_eq!(choose(&inputs(Some("profile_other-pack"))), Choice::Java21);
        assert_eq!(choose(&inputs(Some("profile_wachiland"))), Choice::Java21);
        assert_eq!(choose(&inputs(None)), Choice::Java21);
    }

    #[test]
    fn player_custom_java_always_wins() {
        let mut input = inputs(Some("profile_wachiland-elite"));
        input.jvm_binary_enabled = true;
        assert_eq!(choose(&input), Choice::Java21);
    }

    #[test]
    fn forced_external_java_keeps_java21() {
        let mut input = inputs(Some("profile_wachiland-elite"));
        input.force_external_java = true;
        assert_eq!(choose(&input), Choice::Java21);
    }

    #[test]
    fn saved_fallback_applies_only_to_wachiland_profiles() {
        let mut input = inputs(Some("profile_wachiland-elite"));
        input.saved_fallback_reason = Some("exit-code=1 after 3s");
        assert_eq!(choose(&input), Choice::FallbackSaved("exit-code=1 after 3s"));

        let mut other = inputs(Some("profile_other-pack"));
        other.saved_fallback_reason = Some("exit-code=1 after 3s");
        assert_eq!(choose(&other), Choice::Java21);
    }

    #[test]
    fn java25_component_is_found_by_version_major() {
        let mut components = HashMap::new();
        components.insert(Ustr::from("java-runtime-delta"), vec![component("21.0.7")]);
        components.insert(Ustr::from("java-runtime-epsilon"), vec![component("25.0.1")]);
        let (name, found) = java25_component(&components).expect("java 25 component");
        assert_eq!(name.as_str(), "java-runtime-epsilon");
        assert_eq!(found.version.name.as_str(), "25.0.1");
    }

    #[test]
    fn missing_java25_component_is_not_selected() {
        let mut components = HashMap::new();
        components.insert(Ustr::from("java-runtime-delta"), vec![component("21.0.7")]);
        components.insert(Ustr::from("jre-legacy"), vec![component("8u202")]);
        components.insert(Ustr::from("java-runtime-epsilon"), Vec::new());
        assert!(java25_component(&components).is_none());
    }

    #[test]
    fn major_version_parses_mojang_names() {
        assert_eq!(major_version("25.0.1"), Some(25));
        assert_eq!(major_version("21.0.7"), Some(21));
        assert_eq!(major_version("8u202"), None);
    }

    #[test]
    fn clean_or_late_exit_is_not_a_fallback() {
        assert_eq!(early_exit_reason(Some(0), Duration::from_secs(3)), None);
        assert_eq!(early_exit_reason(Some(1), Duration::from_secs(120)), None);
        assert_eq!(early_exit_reason(Some(1), EARLY_EXIT_WINDOW), None);
    }

    #[test]
    fn early_error_exit_is_recorded_with_reason() {
        assert_eq!(
            early_exit_reason(Some(1), Duration::from_secs(3)),
            Some("exit-code=1 after 3s".to_owned())
        );
        assert_eq!(early_exit_reason(None, Duration::from_secs(2)), Some("signal after 2s".to_owned()));
    }
}
