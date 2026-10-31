//! Profile JVM options distributed by BootOptim Distribution (manifest schema 3).
//!
//! The contract lives in Distribution's `docs/PROFILE_PROTOCOL.md`. The launcher re-checks every
//! rendered argument against its own allowlist even though the revision is signed, and passes an
//! option only when the platform, the real Java version and the VM match: an unknown `-XX` option
//! stops the JVM from starting. The player's personal JVM arguments are never modified and always
//! win, because profile options are placed before them and HotSpot keeps the last occurrence.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// `-D` keys a profile may set. Intentionally empty; adding one needs the same change in
/// Distribution, every launcher and the shared `jvm-options-vectors.json`.
pub const ALLOWED_SYSTEM_PROPERTIES: &[&str] = &[];

/// `-XX` names refused even with a boolean or numeric value: option files, hiding unknown-option
/// failures, alternative compilers, unbounded log files, VM pauses and management access.
const DENIED_XX_FLAGS: &[&str] = &[
    "Flags",
    "VMOptionsFile",
    "IgnoreUnrecognizedVMOptions",
    "EnableJVMCI",
    "EnableJVMCIProduct",
    "UseJVMCICompiler",
    "UseJVMCINativeLibrary",
    "LogVMOutput",
    "ShowMessageBoxOnError",
    "PauseAtStartup",
    "PauseAtExit",
    "ManagementServer",
];

/// Machine facts automatic rules may test. Anything else makes the option not applicable.
pub const AUTO_FACTS: &[&str] = &["logical_cpus", "memory_mib"];

const VALUE_PLACEHOLDER: &str = "{value}";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ManifestJvmOption {
    pub id: String,
    pub label: String,
    pub template: String,
    #[serde(rename = "type")]
    pub kind: JvmOptionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
    pub help: String,
    pub java: JvmJavaRange,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vm: Vec<String>,
    pub platforms: BTreeMap<String, JvmOptionPlatform>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum JvmOptionKind {
    Bool,
    Int,
    Enum,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct JvmJavaRange {
    pub min: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct JvmOptionPlatform {
    pub default_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<JvmValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto: Option<JvmAuto>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct JvmAuto {
    pub rules: Vec<JvmAutoRule>,
    pub otherwise: JvmAutoDecision,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct JvmAutoRule {
    /// Kept as a map so a fact this launcher does not know yet makes only this option not
    /// applicable instead of rejecting the whole signed revision.
    pub when: BTreeMap<String, JvmFactRange>,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<JvmValue>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct JvmAutoDecision {
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<JvmValue>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct JvmFactRange {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum JvmValue {
    Int(i64),
    Text(String),
}

impl std::fmt::Display for JvmValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JvmValue::Int(value) => write!(f, "{value}"),
            JvmValue::Text(value) => f.write_str(value),
        }
    }
}

/// What the player changed for one option on one instance. `None` fields follow the profile
/// (its default, or its automatic rule), so a new profile default reaches untouched choices.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct ProfileJvmOptionChoice {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<JvmValue>,
}

impl ProfileJvmOptionChoice {
    pub fn is_untouched(&self) -> bool {
        self.enabled.is_none() && self.value.is_none()
    }
}

/// Returns why a rendered argument fails the launcher allowlist, or `None` when it passes.
pub fn argument_error(arg: &str) -> Option<&'static str> {
    if arg.is_empty() || arg.len() > 256 {
        return Some("argument is empty or longer than 256 bytes");
    }
    if !arg.bytes().all(|byte| (0x21..=0x7e).contains(&byte)) {
        return Some("argument must be printable ASCII without spaces");
    }
    if let Some(body) = arg.strip_prefix("-XX:") {
        let name = if let Some(name) = body.strip_prefix('+').or_else(|| body.strip_prefix('-')) {
            name
        } else {
            let Some((name, value)) = body.split_once('=') else {
                return Some("-XX values must be a number or size");
            };
            if !valid_number(value) {
                return Some("-XX values must be a number or size");
            }
            name
        };
        if !valid_flag_name(name) {
            return Some("-XX flag name is malformed");
        }
        if DENIED_XX_FLAGS.contains(&name) {
            return Some("-XX flag is not allowed");
        }
        return None;
    }
    if ["-Xss", "-Xms", "-Xmx"].iter().any(|prefix| arg.starts_with(prefix)) {
        return if valid_size(&arg[4..]) {
            None
        } else {
            Some("-Xss/-Xms/-Xmx require a size")
        };
    }
    if let Some(body) = arg.strip_prefix("-D") {
        let Some((key, value)) = body.split_once('=') else {
            return Some("system property is not in the allowlist");
        };
        if !ALLOWED_SYSTEM_PROPERTIES.contains(&key) {
            return Some("system property is not in the allowlist");
        }
        if value.contains(['"', '\'', '\\']) {
            return Some("system property value contains quotes or backslashes");
        }
        return None;
    }
    Some("only -XX:, -Xss, -Xms, -Xmx and allowlisted -D arguments are accepted")
}

/// The identity HotSpot uses for repeated options (the last one wins). Works on any argument,
/// including the player's own flags, so conflicts are found even for unusual spellings.
pub fn flag_key(arg: &str) -> Option<String> {
    if let Some(body) = arg.strip_prefix("-XX:") {
        let name = body.strip_prefix('+').or_else(|| body.strip_prefix('-')).unwrap_or(body);
        let name = name.split('=').next().unwrap_or(name);
        return (!name.is_empty()).then(|| format!("XX:{name}"));
    }
    for prefix in ["-Xss", "-Xms", "-Xmx"] {
        if arg.starts_with(prefix) {
            return Some(prefix[1..].to_owned());
        }
    }
    if let Some(body) = arg.strip_prefix("-D") {
        let key = body.split('=').next().unwrap_or(body);
        return (!key.is_empty()).then(|| format!("D:{key}"));
    }
    None
}

fn valid_flag_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 80
        && bytes[0].is_ascii_alphabetic()
        && bytes.iter().all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
}

fn all_digits(value: &str) -> bool {
    !value.is_empty() && value.len() <= 19 && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn valid_size(value: &str) -> bool {
    let trimmed = value.strip_suffix(['k', 'K', 'm', 'M', 'g', 'G', 't', 'T']).unwrap_or(value);
    all_digits(trimmed)
}

fn valid_number(value: &str) -> bool {
    valid_size(value)
        || value
            .split_once('.')
            .is_some_and(|(whole, fraction)| all_digits(whole) && all_digits(fraction))
}

fn value_text(option: &ManifestJvmOption, value: &JvmValue) -> Option<String> {
    match (option.kind, value) {
        (JvmOptionKind::Int, JvmValue::Int(number)) => {
            let (min, max) = (option.min?, option.max?);
            (*number >= min && *number <= max && i32::try_from(*number).is_ok()).then(|| number.to_string())
        },
        (JvmOptionKind::Enum, JvmValue::Text(text)) => option.values.contains(text).then(|| text.clone()),
        _ => None,
    }
}

/// Renders the argument for a value (ignored for bool options); `None` if the value does not fit.
pub fn render(option: &ManifestJvmOption, value: Option<&JvmValue>) -> Option<String> {
    match option.kind {
        JvmOptionKind::Bool => Some(option.template.clone()),
        _ => Some(option.template.replacen(VALUE_PLACEHOLDER, &value_text(option, value?)?, 1)),
    }
}

/// Every argument the option can produce. Checking these covers all values of an int range
/// because only digits change between its bounds.
fn renderable_arguments(option: &ManifestJvmOption) -> Vec<String> {
    match option.kind {
        JvmOptionKind::Bool => vec![option.template.clone()],
        JvmOptionKind::Int => [option.min, option.max]
            .into_iter()
            .flatten()
            .map(|bound| option.template.replacen(VALUE_PLACEHOLDER, &bound.to_string(), 1))
            .collect(),
        JvmOptionKind::Enum => option
            .values
            .iter()
            .map(|value| option.template.replacen(VALUE_PLACEHOLDER, value, 1))
            .collect(),
    }
}

/// Launcher-side check of a signed option. A failing option is never passed to the JVM.
pub fn allowlist_error(option: &ManifestJvmOption) -> Option<String> {
    let placeholders = option.template.matches(VALUE_PLACEHOLDER).count();
    if option.template.replace(VALUE_PLACEHOLDER, "").contains(['{', '}']) {
        return Some("template contains an unsupported placeholder".into());
    }
    let shape_ok = match option.kind {
        JvmOptionKind::Bool => {
            placeholders == 0 && option.min.is_none() && option.max.is_none() && option.values.is_empty()
        },
        JvmOptionKind::Int => {
            placeholders == 1
                && option.values.is_empty()
                && matches!((option.min, option.max), (Some(min), Some(max)) if min <= max
                    && i32::try_from(min).is_ok() && i32::try_from(max).is_ok())
        },
        JvmOptionKind::Enum => {
            placeholders == 1 && option.min.is_none() && option.max.is_none() && !option.values.is_empty()
        },
    };
    if !shape_ok {
        return Some("template, range or values do not match the option type".into());
    }
    let arguments = renderable_arguments(option);
    let mut keys = BTreeSet::new();
    for arg in &arguments {
        if let Some(reason) = argument_error(arg) {
            return Some(format!("{arg}: {reason}"));
        }
        keys.extend(flag_key(arg));
    }
    if keys.len() != 1 {
        return Some("values render different JVM flags".into());
    }
    None
}

/// The flag key an option sets (all of its values share it once `allowlist_error` passes).
pub fn option_flag_key(option: &ManifestJvmOption) -> Option<String> {
    renderable_arguments(option).first().and_then(|arg| flag_key(arg))
}

/// Facts about the JVM about to start and the machine it runs on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JvmLaunchFacts {
    pub platform: String,
    pub java_major: Option<u32>,
    /// `hotspot` or `openj9`.
    pub java_vm: Option<String>,
    pub logical_cpus: Option<i64>,
    pub memory_mib: Option<i64>,
}

impl JvmLaunchFacts {
    fn fact(&self, name: &str) -> Option<i64> {
        match name {
            "logical_cpus" => self.logical_cpus,
            "memory_mib" => self.memory_mib,
            _ => None,
        }
    }

    /// `java@21-hotspot`, used in compatibility log lines.
    pub fn java_label(&self) -> String {
        format!(
            "java@{}-{}",
            self.java_major.map_or_else(|| "unknown".to_owned(), |major| major.to_string()),
            self.java_vm.as_deref().unwrap_or("unknown")
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JvmDecisionSource {
    ProfileDefault,
    /// Facts that matched, for example `logical_cpus=4`, or `otherwise`.
    Automatic(String),
    Player,
}

impl std::fmt::Display for JvmDecisionSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JvmDecisionSource::ProfileDefault => f.write_str("profile-default"),
            JvmDecisionSource::Automatic(facts) => write!(f, "auto({facts})"),
            JvmDecisionSource::Player => f.write_str("player"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JvmPlanStatus {
    Applied {
        arg: String,
        source: JvmDecisionSource,
    },
    Disabled {
        source: JvmDecisionSource,
    },
    /// The player's own JVM arguments (or memory setting) already set this flag.
    Overridden {
        arg: String,
        flag_key: String,
    },
    /// Not passed because it does not fit this launch; logged as `[WBOMBO-COMPAT] FALLBACK`.
    Skipped { reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JvmPlanEntry {
    pub id: String,
    pub status: JvmPlanStatus,
    /// A saved player value no longer fits the option and the profile value was used instead.
    pub discarded_player_value: bool,
}

/// Flag keys the player controls: personal JVM arguments plus `-Xms`/`-Xmx` when the memory
/// setting is enabled.
pub fn personal_flag_keys(personal_args: &[String], memory_enabled: bool) -> BTreeSet<String> {
    let mut keys = personal_args.iter().filter_map(|arg| flag_key(arg)).collect::<BTreeSet<_>>();
    if memory_enabled {
        keys.insert("Xms".into());
        keys.insert("Xmx".into());
    }
    keys
}

/// A personal `-XX:ActiveProcessorCount=N` changes what the JVM sees, so automatic rules use it.
pub fn active_processor_count(personal_args: &[String]) -> Option<i64> {
    personal_args
        .iter()
        .rev()
        .find_map(|arg| arg.strip_prefix("-XX:ActiveProcessorCount=")?.parse::<i64>().ok())
        .filter(|count| *count > 0)
}

fn decide_automatic(auto: &JvmAuto, facts: &JvmLaunchFacts) -> Result<(bool, Option<JvmValue>, String), String> {
    for rule in &auto.rules {
        let mut matched = Vec::new();
        let mut matches = true;
        for (name, range) in &rule.when {
            if !AUTO_FACTS.contains(&name.as_str()) {
                return Err(format!("auto-fact-unknown:{name}"));
            }
            let Some(value) = facts.fact(name) else {
                return Err(format!("auto-fact-unavailable:{name}"));
            };
            if range.min.is_some_and(|min| value < min) || range.max.is_some_and(|max| value > max) {
                matches = false;
            }
            matched.push(format!("{name}={value}"));
        }
        if matches {
            return Ok((rule.enabled, rule.value.clone(), matched.join(",")));
        }
    }
    Ok((auto.otherwise.enabled, auto.otherwise.value.clone(), "otherwise".into()))
}

/// Decides, for one launch, which profile options are passed and why the others are not.
pub fn plan(
    options: &[ManifestJvmOption],
    choices: &BTreeMap<String, ProfileJvmOptionChoice>,
    facts: &JvmLaunchFacts,
    personal_keys: &BTreeSet<String>,
) -> Vec<JvmPlanEntry> {
    options
        .iter()
        .map(|option| {
            let (status, discarded_player_value) = plan_one(option, choices.get(&option.id), facts, personal_keys);
            JvmPlanEntry {
                id: option.id.clone(),
                status,
                discarded_player_value,
            }
        })
        .collect()
}

fn plan_one(
    option: &ManifestJvmOption,
    choice: Option<&ProfileJvmOptionChoice>,
    facts: &JvmLaunchFacts,
    personal_keys: &BTreeSet<String>,
) -> (JvmPlanStatus, bool) {
    let skip = |reason: String| (JvmPlanStatus::Skipped { reason }, false);
    let Some(platform) = option.platforms.get(&facts.platform) else {
        return skip(format!("platform-not-{}", facts.platform));
    };
    if let Some(reason) = allowlist_error(option) {
        return skip(format!("not-allowed ({reason})"));
    }
    let Some(java_major) = facts.java_major else {
        return skip("java-version-unknown".into());
    };
    if java_major < option.java.min || option.java.max.is_some_and(|max| java_major > max) {
        let range = match option.java.max {
            Some(max) => format!("{}-{max}", option.java.min),
            None => format!("{}+", option.java.min),
        };
        return skip(format!("requires-java-{range}"));
    }
    if !option.vm.is_empty() {
        match &facts.java_vm {
            Some(vm) if option.vm.contains(vm) => {},
            Some(vm) => return skip(format!("requires-vm-{}-not-{vm}", option.vm.join("|"))),
            None => return skip("java-vm-unknown".into()),
        }
    }

    let (enabled, profile_value, source) = match choice.and_then(|choice| choice.enabled) {
        Some(enabled) => (enabled, platform.default_value.clone(), JvmDecisionSource::Player),
        None => match &platform.auto {
            Some(auto) => match decide_automatic(auto, facts) {
                Ok((enabled, value, matched)) => (
                    enabled,
                    value.or_else(|| platform.default_value.clone()),
                    JvmDecisionSource::Automatic(matched),
                ),
                Err(reason) => return skip(reason),
            },
            None => (
                platform.default_enabled,
                platform.default_value.clone(),
                JvmDecisionSource::ProfileDefault,
            ),
        },
    };
    if !enabled {
        return (JvmPlanStatus::Disabled { source }, false);
    }

    let player_value = choice.and_then(|choice| choice.value.as_ref());
    let mut discarded = false;
    let arg = match player_value.and_then(|value| render(option, Some(value))) {
        Some(arg) => Some(arg),
        None => {
            discarded = player_value.is_some();
            render(option, profile_value.as_ref())
        },
    };
    let Some(arg) = arg else {
        return skip("profile-value-invalid".into());
    };
    let source = if player_value.is_some() && !discarded {
        JvmDecisionSource::Player
    } else {
        source
    };
    let key = flag_key(&arg).unwrap_or_default();
    if personal_keys.contains(&key) {
        return (JvmPlanStatus::Overridden { arg, flag_key: key }, discarded);
    }
    (JvmPlanStatus::Applied { arg, source }, discarded)
}

/// What the instance settings page shows: the effective options with the decision the next
/// Start would take for each, and the facts it was based on. Java facts come from the last
/// launch of this instance; before that they are unknown and checked at Start.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProfileJvmOptionsView {
    pub options: Vec<(ManifestJvmOption, JvmPlanEntry)>,
    pub facts: JvmLaunchFacts,
    /// Last launcher-managed runtime for this instance/configuration, if known.
    /// Local UI metadata only; this is not part of the signed profile protocol.
    pub automatic_java_major: Option<u32>,
}

/// Resolves a signed chain (oldest first): children replace options by id and
/// `remove_jvm_options` withdraws inherited ones. Declaration order is preserved.
pub fn apply_manifest_options(
    effective: &mut Vec<ManifestJvmOption>,
    options: &[ManifestJvmOption],
    removals: &[String],
) -> Result<(), String> {
    for id in removals {
        let Some(index) = effective.iter().position(|option| &option.id == id) else {
            return Err(format!("revision removes JVM option {id}, which its parent does not define"));
        };
        effective.remove(index);
    }
    let mut seen = BTreeSet::new();
    for option in options {
        if !seen.insert(option.id.as_str()) {
            return Err(format!("revision defines JVM option {} twice", option.id));
        }
        match effective.iter_mut().find(|existing| existing.id == option.id) {
            Some(existing) => *existing = option.clone(),
            None => effective.push(option.clone()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct Vectors {
        allowed_system_properties: Vec<String>,
        accept: Vec<AcceptVector>,
        reject: Vec<RejectVector>,
    }
    #[derive(Deserialize)]
    struct AcceptVector {
        arg: String,
        flag_key: String,
    }
    #[derive(Deserialize)]
    struct RejectVector {
        arg: String,
        why: String,
    }

    #[test]
    fn shared_allowlist_vectors() {
        let vectors: Vectors = serde_json::from_str(include_str!("profile_jvm/jvm-options-vectors.json")).unwrap();
        assert_eq!(vectors.allowed_system_properties, ALLOWED_SYSTEM_PROPERTIES);
        for vector in &vectors.accept {
            assert_eq!(argument_error(&vector.arg), None, "{}", vector.arg);
            assert_eq!(flag_key(&vector.arg).as_deref(), Some(vector.flag_key.as_str()), "{}", vector.arg);
        }
        for vector in &vectors.reject {
            assert!(argument_error(&vector.arg).is_some(), "{} accepted ({})", vector.arg, vector.why);
        }
    }

    fn jit() -> ManifestJvmOption {
        serde_json::from_str(
            r#"{"id":"jit-compiler-threads","label":"Hilos del compilador JIT","template":"-XX:CICompilerCount={value}",
            "type":"int","min":2,"max":4,"help":"…","java":{"min":21},"vm":["hotspot"],
            "platforms":{"pc":{"default_enabled":false,"default_value":2}}}"#,
        )
        .unwrap()
    }

    fn pc(java: u32, cpus: i64) -> JvmLaunchFacts {
        JvmLaunchFacts {
            platform: "pc".into(),
            java_major: Some(java),
            java_vm: Some("hotspot".into()),
            logical_cpus: Some(cpus),
            memory_mib: Some(16384),
        }
    }

    fn status(
        option: ManifestJvmOption,
        choice: Option<ProfileJvmOptionChoice>,
        facts: JvmLaunchFacts,
        personal: &[&str],
    ) -> JvmPlanEntry {
        let choices = choice.into_iter().map(|choice| (option.id.clone(), choice)).collect();
        let personal = personal.iter().map(|arg| arg.to_string()).collect::<Vec<_>>();
        plan(&[option], &choices, &facts, &personal_flag_keys(&personal, false)).remove(0)
    }

    #[test]
    fn defaults_choices_and_conflicts() {
        assert_eq!(
            status(jit(), None, pc(21, 4), &[]).status,
            JvmPlanStatus::Disabled {
                source: JvmDecisionSource::ProfileDefault
            }
        );
        let enabled = ProfileJvmOptionChoice {
            enabled: Some(true),
            value: None,
        };
        assert_eq!(
            status(jit(), Some(enabled.clone()), pc(21, 4), &[]).status,
            JvmPlanStatus::Applied {
                arg: "-XX:CICompilerCount=2".into(),
                source: JvmDecisionSource::Player
            }
        );
        let three = ProfileJvmOptionChoice {
            enabled: Some(true),
            value: Some(JvmValue::Int(3)),
        };
        assert_eq!(
            status(jit(), Some(three), pc(21, 4), &[]).status,
            JvmPlanStatus::Applied {
                arg: "-XX:CICompilerCount=3".into(),
                source: JvmDecisionSource::Player
            }
        );
        let stale = ProfileJvmOptionChoice {
            enabled: Some(true),
            value: Some(JvmValue::Int(9)),
        };
        let entry = status(jit(), Some(stale), pc(21, 4), &[]);
        assert!(entry.discarded_player_value);
        assert!(matches!(entry.status, JvmPlanStatus::Applied { ref arg, .. } if arg == "-XX:CICompilerCount=2"));
        assert_eq!(
            status(jit(), Some(enabled), pc(21, 4), &["-Xmx8G", "-XX:CICompilerCount=6"]).status,
            JvmPlanStatus::Overridden {
                arg: "-XX:CICompilerCount=2".into(),
                flag_key: "XX:CICompilerCount".into()
            }
        );
    }

    #[test]
    fn platform_java_and_vm_gates() {
        let enabled = Some(ProfileJvmOptionChoice {
            enabled: Some(true),
            value: None,
        });
        assert!(
            matches!(status(jit(), enabled.clone(), pc(17, 4), &[]).status, JvmPlanStatus::Skipped { ref reason } if reason == "requires-java-21+")
        );
        let mut android = pc(21, 4);
        android.platform = "android".into();
        assert!(
            matches!(status(jit(), enabled.clone(), android, &[]).status, JvmPlanStatus::Skipped { ref reason } if reason == "platform-not-android")
        );
        let mut openj9 = pc(21, 4);
        openj9.java_vm = Some("openj9".into());
        assert!(matches!(
            status(jit(), enabled.clone(), openj9, &[]).status,
            JvmPlanStatus::Skipped { .. }
        ));
        let mut unknown = pc(21, 4);
        unknown.java_major = None;
        assert!(
            matches!(status(jit(), enabled, unknown, &[]).status, JvmPlanStatus::Skipped { ref reason } if reason == "java-version-unknown")
        );
        let mut agent = jit();
        agent.template = "-javaagent:{value}.jar".into();
        assert!(
            matches!(status(agent, None, pc(21, 4), &[]).status, JvmPlanStatus::Skipped { ref reason } if reason.starts_with("not-allowed"))
        );
    }

    #[test]
    fn automatic_rules() {
        let mut option = jit();
        option.platforms.get_mut("pc").unwrap().auto = Some(serde_json::from_str(
            r#"{"rules":[{"when":{"logical_cpus":{"max":4}},"enabled":true,"value":2}],"otherwise":{"enabled":false}}"#,
        ).unwrap());
        assert_eq!(
            status(option.clone(), None, pc(21, 4), &[]).status,
            JvmPlanStatus::Applied {
                arg: "-XX:CICompilerCount=2".into(),
                source: JvmDecisionSource::Automatic("logical_cpus=4".into())
            }
        );
        assert_eq!(
            status(option.clone(), None, pc(21, 16), &[]).status,
            JvmPlanStatus::Disabled {
                source: JvmDecisionSource::Automatic("otherwise".into())
            }
        );
        // A personal ActiveProcessorCount is what the JVM sees.
        let personal = vec!["-XX:ActiveProcessorCount=4".to_owned()];
        let mut facts = pc(21, 16);
        facts.logical_cpus = active_processor_count(&personal).or(facts.logical_cpus);
        assert!(matches!(status(option.clone(), None, facts, &[]).status, JvmPlanStatus::Applied { .. }));
        // The player can still force it off.
        let off = ProfileJvmOptionChoice {
            enabled: Some(false),
            value: None,
        };
        assert_eq!(
            status(option.clone(), Some(off), pc(21, 4), &[]).status,
            JvmPlanStatus::Disabled {
                source: JvmDecisionSource::Player
            }
        );
        let mut future = option;
        future.platforms.get_mut("pc").unwrap().auto.as_mut().unwrap().rules[0].when.insert(
            "gpu_vendor".into(),
            JvmFactRange {
                min: Some(1),
                max: None,
            },
        );
        assert!(
            matches!(status(future, None, pc(21, 4), &[]).status, JvmPlanStatus::Skipped { ref reason } if reason == "auto-fact-unknown:gpu_vendor")
        );
    }

    #[test]
    fn memory_setting_owns_heap_flags() {
        let mut heap = jit();
        heap.id = "heap".into();
        heap.template = "-Xmx{value}m".into();
        heap.min = Some(1024);
        heap.max = Some(8192);
        heap.vm.clear();
        heap.platforms.get_mut("pc").unwrap().default_enabled = true;
        heap.platforms.get_mut("pc").unwrap().default_value = Some(JvmValue::Int(4096));
        let choices = BTreeMap::new();
        let applied = plan(&[heap.clone()], &choices, &pc(21, 4), &personal_flag_keys(&[], false));
        assert!(matches!(applied[0].status, JvmPlanStatus::Applied { .. }));
        let overridden = plan(&[heap], &choices, &pc(21, 4), &personal_flag_keys(&[], true));
        assert!(matches!(overridden[0].status, JvmPlanStatus::Overridden { .. }));
    }

    #[test]
    fn chain_resolution() {
        let mut effective = Vec::new();
        apply_manifest_options(&mut effective, &[jit()], &[]).unwrap();
        let mut replaced = jit();
        replaced.platforms.get_mut("pc").unwrap().default_enabled = true;
        apply_manifest_options(&mut effective, &[replaced], &[]).unwrap();
        assert!(effective[0].platforms["pc"].default_enabled);
        assert!(apply_manifest_options(&mut effective, &[], &["missing".into()]).is_err());
        apply_manifest_options(&mut effective, &[], &["jit-compiler-threads".into()]).unwrap();
        assert!(effective.is_empty());
        assert!(apply_manifest_options(&mut effective, &[jit(), jit()], &[]).is_err());
    }
}
