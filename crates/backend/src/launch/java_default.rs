//! Java of the game JVM on PC: Mojang's Java for the Minecraft version, or the Java major the
//! profile recommends (`profile.java_runtime`, see `crate::profile_java`).
//!
//! The choice is taken at Start from the committed branch state and the instance settings; nothing
//! here contacts the distribution service. The recommended major is mapped to Mojang's runtime
//! component of the platform whose version starts with it (25 is `java-runtime-epsilon`),
//! downloaded through the existing hash-verified path. If a start with it exits with an error
//! within [`EARLY_EXIT_WINDOW`], the instance records a fallback and later Starts use Mojang's Java
//! until the profile recommends another major or the player removes the record.

use std::{collections::HashMap, time::Duration};

use schema::java_runtimes::JavaRuntimeComponent;
use ustr::Ustr;

/// The Java major of the JDK 25 mechanisms (AOT cache, G1 heap sizing).
pub(crate) const JAVA_MAJOR: u32 = 25;
/// Measured on the reference PC: the main menu was reached after about 69 s with Java 25. An error
/// exit inside this window cannot come from a run that already reached the menu. Errors later than
/// this are not observed by the launcher, which does not read the game's output by default.
pub(crate) const EARLY_EXIT_WINDOW: Duration = Duration::from_secs(15);

/// What the Start of one instance should use for the game JVM.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Choice<'a> {
    /// The usual path: Mojang's Java for the version, or the player's custom binary, pinned
    /// runtime or forced Java.
    Usual,
    /// The Java major the profile recommends; Mojang's Java if the manifest has no such runtime.
    Profile(u32),
    /// The profile recommends this major, but an earlier start with it failed. Carries the reason.
    FallbackSaved {
        major: u32,
        reason: &'a str,
    },
}

pub(crate) struct Inputs<'a> {
    /// The Java major the applied profile chain recommends on PC, or `None` for Mojang's Java.
    pub profile_major: Option<u32>,
    /// The player enabled a custom Java for this instance: their choice always wins.
    pub jvm_binary_enabled: bool,
    /// The player pinned one of the launcher's runtimes for this instance: it wins too.
    pub runtime_pinned: bool,
    /// `FORCE_EXTERNAL_JAVA` is set (developer override).
    pub force_external_java: bool,
    /// The instance's failure record: the major that failed and why.
    pub saved_fallback: Option<(u32, &'a str)>,
}

pub(crate) fn choose<'a>(inputs: &Inputs<'a>) -> Choice<'a> {
    if inputs.jvm_binary_enabled || inputs.runtime_pinned || inputs.force_external_java {
        return Choice::Usual;
    }
    let Some(major) = inputs.profile_major else {
        return Choice::Usual;
    };
    match inputs.saved_fallback {
        Some((failed, reason)) if failed == major => Choice::FallbackSaved { major, reason },
        _ => Choice::Profile(major),
    }
}

/// A failure record no longer applies once the profile recommends another major or Mojang's Java.
pub(crate) fn fallback_is_stale(profile_major: Option<u32>, failed_major: u32) -> bool {
    profile_major != Some(failed_major)
}

/// Major version of a Mojang runtime version name, such as `25.0.1` or `21.0.7`.
pub(crate) fn major_version(version_name: &str) -> Option<u32> {
    version_name.split('.').next()?.parse().ok()
}

/// The first component of a platform whose version starts with `major`, taken in component-name
/// order so the choice is stable. Returns `None` when the manifest has no such component; the
/// caller then uses Mojang's Java for the version.
pub(crate) fn component_for_major(
    components: &HashMap<Ustr, Vec<JavaRuntimeComponent>>,
    major: u32,
) -> Option<(Ustr, &JavaRuntimeComponent)> {
    let mut names: Vec<Ustr> = components.keys().copied().collect();
    names.sort_unstable_by(|a, b| a.as_str().cmp(b.as_str()));
    names.into_iter().find_map(|name| {
        let component = components.get(&name)?.first()?;
        (major_version(component.version.name.as_str()) == Some(major)).then_some((name, component))
    })
}

/// The reason to record when a start with the profile's Java ended with an error inside the early window, or
/// `None` for a clean exit or a run that lasted at least [`EARLY_EXIT_WINDOW`].
pub(crate) fn early_exit_reason(exit_code: Option<i32>, lived: Duration) -> Option<String> {
    if exit_code == Some(0) || lived >= EARLY_EXIT_WINDOW {
        return None;
    }
    Some(match exit_code {
        // `lived` is zero for a JVM that showed the launcher's error dialog (see `jvm_error_dialogs`).
        Some(code) if lived.is_zero() => format!("exit-code={code}, JVM error dialog"),
        Some(code) => format!("exit-code={code} after {}s", lived.as_secs()),
        None => format!("signal after {}s", lived.as_secs()),
    })
}

pub(crate) fn log_active(major: u32, version: &str) {
    log::info!("[WBOMBO-COMPAT] ACTIVE project=Pandora mechanism=java-runtime/{major} mod=java@{version}");
}

pub(crate) fn log_fallback(major: u32, reason: &str) {
    log::warn!(
        "[WBOMBO-COMPAT] FALLBACK project=Pandora mechanism=java-runtime/{major} mod=java@{major} reason={reason}"
    );
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};
    use schema::java_runtimes::{
        JavaRuntimeComponent, JavaRuntimeComponentAvailability, JavaRuntimeComponentManifestLink,
        JavaRuntimeComponentVersion,
    };

    use super::*;

    fn inputs(profile_major: Option<u32>) -> Inputs<'static> {
        Inputs {
            profile_major,
            jvm_binary_enabled: false,
            runtime_pinned: false,
            force_external_java: false,
            saved_fallback: None,
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
    fn the_profile_decides_and_no_profile_means_mojang() {
        assert_eq!(choose(&inputs(Some(25))), Choice::Profile(25));
        assert_eq!(choose(&inputs(Some(17))), Choice::Profile(17));
        assert_eq!(choose(&inputs(None)), Choice::Usual);
    }

    #[test]
    fn player_custom_pinned_or_forced_java_always_wins() {
        let mut custom = inputs(Some(25));
        custom.jvm_binary_enabled = true;
        assert_eq!(choose(&custom), Choice::Usual);
        let mut pinned = inputs(Some(25));
        pinned.runtime_pinned = true;
        pinned.saved_fallback = Some((25, "exit-code=1 after 3s"));
        assert_eq!(choose(&pinned), Choice::Usual);
        let mut forced = inputs(Some(25));
        forced.force_external_java = true;
        assert_eq!(choose(&forced), Choice::Usual);
    }

    #[test]
    fn a_saved_fallback_applies_only_to_the_major_that_failed() {
        let mut input = inputs(Some(25));
        input.saved_fallback = Some((25, "exit-code=1 after 3s"));
        assert_eq!(
            choose(&input),
            Choice::FallbackSaved {
                major: 25,
                reason: "exit-code=1 after 3s"
            }
        );
        input.profile_major = Some(26);
        assert_eq!(choose(&input), Choice::Profile(26));
        input.profile_major = None;
        assert_eq!(choose(&input), Choice::Usual);
    }

    #[test]
    fn the_record_is_stale_when_the_profile_changes_major_or_goes_back_to_mojang() {
        assert!(!fallback_is_stale(Some(25), 25));
        assert!(fallback_is_stale(Some(21), 25));
        assert!(fallback_is_stale(None, 25));
    }

    #[test]
    fn the_component_is_found_by_version_major() {
        let mut components = HashMap::new();
        components.insert(Ustr::from("java-runtime-delta"), vec![component("21.0.7")]);
        components.insert(Ustr::from("java-runtime-epsilon"), vec![component("25.0.1")]);
        let (name, found) = component_for_major(&components, 25).expect("java 25 component");
        assert_eq!(name.as_str(), "java-runtime-epsilon");
        assert_eq!(found.version.name.as_str(), "25.0.1");
        assert_eq!(component_for_major(&components, 21).unwrap().0.as_str(), "java-runtime-delta");
    }

    #[test]
    fn a_missing_major_is_not_selected() {
        let mut components = HashMap::new();
        components.insert(Ustr::from("java-runtime-delta"), vec![component("21.0.7")]);
        components.insert(Ustr::from("jre-legacy"), vec![component("8u202")]);
        components.insert(Ustr::from("java-runtime-epsilon"), Vec::new());
        assert!(component_for_major(&components, 25).is_none());
        assert!(component_for_major(&components, 17).is_none());
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
