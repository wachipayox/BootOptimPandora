//! Launcher-wide Minecraft settings (`options.txt`) copied into new instances.
//!
//! Unlike a shared file, the global settings are stored in one canonical form
//! and written into a new instance's own `options.txt` in the format that its
//! Minecraft version and loader expect; from then on the instance's file is its
//! own. The format of a setting is detected from
//! the target file first (the key it already uses and how its value looks), and
//! only falls back to the data version when the target does not have the key.
//!
//! The canonical form is the modern one (1.19+ value encoding, GLFW key names,
//! Forge/NeoForge `:MODIFIER` suffixes kept). Unknown keys are carried verbatim.
use std::collections::BTreeMap;

use crate::loader::Loader;

/// First release data versions where a format changed.
pub const DATA_VERSION_1_11: u32 = 819;
pub const DATA_VERSION_1_13: u32 = 1519;
pub const DATA_VERSION_1_16: u32 = 2566;
pub const DATA_VERSION_1_19: u32 = 3105;

/// Never global: tied to the instance's content, its server or the file itself.
pub const EXCLUDED_KEYS: &[&str] = &["version", "resourcePacks", "incompatibleResourcePacks", "lastServer"];

/// Enum options that 1.19+ writes as JSON strings (`mainHand:"right"`).
const QUOTED_KEYS: &[&str] = &[
    "renderClouds",
    "mainHand",
    "soundDevice",
    "graphicsPreset",
    "inactivityFpsLimit",
];

pub type Canonical = BTreeMap<String, String>;

/// One `options.txt`, preserving order and lines that are not `key:value`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OptionsText {
    entries: Vec<Entry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Entry {
    Pair(String, String),
    Other(String),
}

impl OptionsText {
    pub fn parse(text: &str) -> Self {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let entries = text
            .lines()
            .map(|line| match line.split_once(':') {
                Some((key, value)) if !key.is_empty() => Entry::Pair(key.to_string(), value.to_string()),
                _ => Entry::Other(line.to_string()),
            })
            .collect();
        Self { entries }
    }

    pub fn is_empty(&self) -> bool {
        !self.entries.iter().any(|entry| matches!(entry, Entry::Pair(..)))
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.iter().find_map(|entry| match entry {
            Entry::Pair(k, v) if k == key => Some(v.as_str()),
            _ => None,
        })
    }

    pub fn pairs(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().filter_map(|entry| match entry {
            Entry::Pair(k, v) => Some((k.as_str(), v.as_str())),
            Entry::Other(_) => None,
        })
    }

    /// Replaces the value in place, or appends the key. Returns whether the
    /// file changed.
    pub fn set(&mut self, key: &str, value: &str) -> bool {
        for entry in &mut self.entries {
            if let Entry::Pair(k, v) = entry
                && k == key
            {
                if v == value {
                    return false;
                }
                *v = value.to_string();
                return true;
            }
        }
        self.entries.push(Entry::Pair(key.to_string(), value.to_string()));
        true
    }

    pub fn data_version(&self) -> Option<u32> {
        self.get("version")?.trim().parse().ok()
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        for entry in &self.entries {
            match entry {
                Entry::Pair(k, v) => {
                    out.push_str(k);
                    out.push(':');
                    out.push_str(v);
                },
                Entry::Other(line) => out.push_str(line),
            }
            out.push('\n');
        }
        out
    }
}

fn is_excluded(key: &str) -> bool {
    EXCLUDED_KEYS.contains(&key)
}

fn quote(value: &str) -> String {
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        value.to_string()
    } else {
        serde_json::to_string(value).unwrap_or_else(|_| format!("\"{value}\""))
    }
}

fn unquote(value: &str) -> String {
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        serde_json::from_str::<String>(value).unwrap_or_else(|_| value[1..value.len() - 1].to_string())
    } else {
        value.to_string()
    }
}

fn is_numeric(value: &str) -> bool {
    value.trim().parse::<i64>().is_ok()
}

fn split_modifier(value: &str) -> (&str, Option<&str>) {
    if let Some((key, modifier)) = value.rsplit_once(':')
        && matches!(modifier, "NONE" | "SHIFT" | "CONTROL" | "ALT")
    {
        return (key, Some(modifier));
    }
    (value, None)
}

fn loader_has_key_modifiers(loader: Loader) -> bool {
    matches!(loader, Loader::Forge | Loader::NeoForge)
}

/// Canonical view of a file: every non-excluded key, in modern format.
pub fn capture(file: &OptionsText) -> Canonical {
    let mut result = Canonical::new();
    for (key, value) in file.pairs() {
        if is_excluded(key) {
            continue;
        }
        match key {
            "fancyGraphics" => {
                result
                    .entry("graphicsMode".into())
                    .or_insert_with(|| if value == "true" { "1" } else { "0" }.into());
            },
            "graphicsPreset" => {
                result.insert(key.into(), quote(value));
                let mode = match unquote(value).as_str() {
                    "fast" => Some("0"),
                    "fancy" => Some("1"),
                    "fabulous" => Some("2"),
                    _ => None,
                };
                if let Some(mode) = mode
                    && file.get("graphicsMode").is_none()
                {
                    result.insert("graphicsMode".into(), mode.into());
                }
            },
            "graphicsMode" => {
                result.insert(key.into(), value.into());
            },
            "ao" if is_numeric(value) => {
                result.insert(key.into(), if value.trim() == "0" { "false" } else { "true" }.into());
            },
            "lang" => {
                result.insert(key.into(), value.to_ascii_lowercase());
            },
            _ if key.starts_with("key_") => {
                let (binding, modifier) = split_modifier(value);
                let binding = if is_numeric(binding) {
                    match binding.trim().parse::<i32>().ok().and_then(lwjgl2_to_name) {
                        Some(name) => name.to_string(),
                        None => continue,
                    }
                } else {
                    binding.to_string()
                };
                let value = match modifier {
                    Some(modifier) if modifier != "NONE" => format!("{binding}:{modifier}"),
                    _ => binding,
                };
                result.insert(key.into(), value);
            },
            _ if QUOTED_KEYS.contains(&key) => {
                result.insert(key.into(), quote(value));
            },
            _ => {
                result.insert(key.into(), value.into());
            },
        }
    }
    result
}

/// How the target encodes values, detected from its own file.
struct TargetFormat<'a> {
    file: &'a OptionsText,
    data_version: Option<u32>,
    loader: Loader,
}

impl TargetFormat<'_> {
    /// Whether the target is at least `version`. Unknown means modern.
    fn at_least(&self, version: u32) -> bool {
        self.data_version.is_none_or(|current| current >= version)
    }

    fn quoted(&self, key: &str) -> bool {
        match self.file.get(key) {
            Some(existing) => existing.starts_with('"'),
            None => self.at_least(DATA_VERSION_1_19),
        }
    }

    fn numeric_keys(&self) -> bool {
        match self.file.get("key_key.forward").or_else(|| self.file.get("key_key.jump")) {
            Some(existing) => is_numeric(split_modifier(existing).0),
            None => !self.at_least(DATA_VERSION_1_13),
        }
    }
}

/// Converts one canonical setting into the target's format. `None` leaves the
/// target's value alone.
fn adapt(key: &str, value: &str, target: &TargetFormat) -> Option<(String, String)> {
    let file = target.file;
    match key {
        "graphicsMode" => {
            if file.get("graphicsPreset").is_some() {
                return None;
            }
            if file.get("fancyGraphics").is_some()
                || (file.get("graphicsMode").is_none() && !target.at_least(DATA_VERSION_1_16))
            {
                let fancy = value.trim() != "0";
                return Some(("fancyGraphics".into(), fancy.to_string()));
            }
            Some((key.into(), value.into()))
        },
        "graphicsPreset" => {
            // Only versions that already use presets understand them.
            file.get("graphicsPreset")?;
            Some((key.into(), value.into()))
        },
        "ao" => {
            let numeric = match file.get("ao") {
                Some(existing) => is_numeric(existing),
                None => !target.at_least(DATA_VERSION_1_19),
            };
            if numeric {
                Some((key.into(), if value == "false" { "0" } else { "2" }.into()))
            } else {
                Some((key.into(), value.into()))
            }
        },
        "lang" => {
            let upper_region = match file.get("lang") {
                Some(existing) => existing.chars().any(|c| c.is_ascii_uppercase()),
                None => !target.at_least(DATA_VERSION_1_11),
            };
            if upper_region && let Some((language, region)) = value.split_once('_') {
                return Some((key.into(), format!("{language}_{}", region.to_ascii_uppercase())));
            }
            Some((key.into(), value.into()))
        },
        "gamma" if target.at_least(DATA_VERSION_1_19) => {
            // 1.19+ resets an out-of-range gamma to the default.
            let gamma = value.trim().parse::<f64>().ok()?;
            Some((key.into(), if gamma > 1.0 { "1.0".into() } else { value.into() }))
        },
        _ if key.starts_with("key_") => {
            let (binding, modifier) = split_modifier(value);
            let modifier = modifier.filter(|m| *m != "NONE");
            if modifier.is_some() && !loader_has_key_modifiers(target.loader) {
                // Without modifier support the binding would lose its Ctrl/Shift
                // and collide; keep the target's own binding instead.
                return None;
            }
            let binding = if target.numeric_keys() {
                name_to_lwjgl2(binding)?.to_string()
            } else {
                binding.to_string()
            };
            Some((
                key.into(),
                match modifier {
                    Some(modifier) => format!("{binding}:{modifier}"),
                    None => binding,
                },
            ))
        },
        _ if QUOTED_KEYS.contains(&key) => {
            if target.quoted(key) {
                Some((key.into(), quote(value)))
            } else {
                Some((key.into(), unquote(value)))
            }
        },
        _ => Some((key.into(), value.into())),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyOutcome {
    /// The file was rewritten with this many changed keys.
    Changed(usize),
    Unchanged,
    /// The target has no `options.txt` and its data version is unknown, so its
    /// format cannot be decided yet.
    NeedsDataVersion,
}

/// Writes the global settings into `file` in the target's format.
/// `fallback_data_version` is used when the file has no `version:` line, e.g.
/// for an instance that has never been started.
pub fn apply(
    file: &mut OptionsText,
    global: &Canonical,
    loader: Loader,
    fallback_data_version: Option<u32>,
) -> ApplyOutcome {
    let data_version = file.data_version().or(fallback_data_version);
    if file.is_empty() {
        let Some(data_version) = data_version else {
            return ApplyOutcome::NeedsDataVersion;
        };
        // Minecraft runs its data fixers from version 0 on a file without a
        // version line; record the version we wrote for.
        file.set("version", &data_version.to_string());
    }
    let snapshot = file.clone();
    let target = TargetFormat {
        file: &snapshot,
        data_version,
        loader,
    };
    let mut changed = 0;
    for (key, value) in global {
        if is_excluded(key) {
            continue;
        }
        if let Some((key, value)) = adapt(key, value, &target)
            && file.set(&key, &value)
        {
            changed += 1;
        }
    }
    if changed == 0 {
        ApplyOutcome::Unchanged
    } else {
        ApplyOutcome::Changed(changed)
    }
}

const LWJGL2_KEYS: &[(i32, &str)] = &[
    (0, "key.keyboard.unknown"),
    (1, "key.keyboard.escape"),
    (2, "key.keyboard.1"),
    (3, "key.keyboard.2"),
    (4, "key.keyboard.3"),
    (5, "key.keyboard.4"),
    (6, "key.keyboard.5"),
    (7, "key.keyboard.6"),
    (8, "key.keyboard.7"),
    (9, "key.keyboard.8"),
    (10, "key.keyboard.9"),
    (11, "key.keyboard.0"),
    (12, "key.keyboard.minus"),
    (13, "key.keyboard.equal"),
    (14, "key.keyboard.backspace"),
    (15, "key.keyboard.tab"),
    (16, "key.keyboard.q"),
    (17, "key.keyboard.w"),
    (18, "key.keyboard.e"),
    (19, "key.keyboard.r"),
    (20, "key.keyboard.t"),
    (21, "key.keyboard.y"),
    (22, "key.keyboard.u"),
    (23, "key.keyboard.i"),
    (24, "key.keyboard.o"),
    (25, "key.keyboard.p"),
    (26, "key.keyboard.left.bracket"),
    (27, "key.keyboard.right.bracket"),
    (28, "key.keyboard.enter"),
    (29, "key.keyboard.left.control"),
    (30, "key.keyboard.a"),
    (31, "key.keyboard.s"),
    (32, "key.keyboard.d"),
    (33, "key.keyboard.f"),
    (34, "key.keyboard.g"),
    (35, "key.keyboard.h"),
    (36, "key.keyboard.j"),
    (37, "key.keyboard.k"),
    (38, "key.keyboard.l"),
    (39, "key.keyboard.semicolon"),
    (40, "key.keyboard.apostrophe"),
    (41, "key.keyboard.grave.accent"),
    (42, "key.keyboard.left.shift"),
    (43, "key.keyboard.backslash"),
    (44, "key.keyboard.z"),
    (45, "key.keyboard.x"),
    (46, "key.keyboard.c"),
    (47, "key.keyboard.v"),
    (48, "key.keyboard.b"),
    (49, "key.keyboard.n"),
    (50, "key.keyboard.m"),
    (51, "key.keyboard.comma"),
    (52, "key.keyboard.period"),
    (53, "key.keyboard.slash"),
    (54, "key.keyboard.right.shift"),
    (55, "key.keyboard.keypad.multiply"),
    (56, "key.keyboard.left.alt"),
    (57, "key.keyboard.space"),
    (58, "key.keyboard.caps.lock"),
    (59, "key.keyboard.f1"),
    (60, "key.keyboard.f2"),
    (61, "key.keyboard.f3"),
    (62, "key.keyboard.f4"),
    (63, "key.keyboard.f5"),
    (64, "key.keyboard.f6"),
    (65, "key.keyboard.f7"),
    (66, "key.keyboard.f8"),
    (67, "key.keyboard.f9"),
    (68, "key.keyboard.f10"),
    (69, "key.keyboard.num.lock"),
    (70, "key.keyboard.scroll.lock"),
    (71, "key.keyboard.keypad.7"),
    (72, "key.keyboard.keypad.8"),
    (73, "key.keyboard.keypad.9"),
    (74, "key.keyboard.keypad.subtract"),
    (75, "key.keyboard.keypad.4"),
    (76, "key.keyboard.keypad.5"),
    (77, "key.keyboard.keypad.6"),
    (78, "key.keyboard.keypad.add"),
    (79, "key.keyboard.keypad.1"),
    (80, "key.keyboard.keypad.2"),
    (81, "key.keyboard.keypad.3"),
    (82, "key.keyboard.keypad.0"),
    (83, "key.keyboard.keypad.decimal"),
    (87, "key.keyboard.f11"),
    (88, "key.keyboard.f12"),
    (156, "key.keyboard.keypad.enter"),
    (157, "key.keyboard.right.control"),
    (181, "key.keyboard.keypad.divide"),
    (184, "key.keyboard.right.alt"),
    (197, "key.keyboard.pause"),
    (199, "key.keyboard.home"),
    (200, "key.keyboard.up"),
    (201, "key.keyboard.page.up"),
    (203, "key.keyboard.left"),
    (205, "key.keyboard.right"),
    (207, "key.keyboard.end"),
    (208, "key.keyboard.down"),
    (209, "key.keyboard.page.down"),
    (210, "key.keyboard.insert"),
    (211, "key.keyboard.delete"),
    (219, "key.keyboard.left.win"),
    (220, "key.keyboard.right.win"),
    (-100, "key.mouse.left"),
    (-99, "key.mouse.right"),
    (-98, "key.mouse.middle"),
    (-97, "key.mouse.4"),
    (-96, "key.mouse.5"),
    (-95, "key.mouse.6"),
    (-94, "key.mouse.7"),
    (-93, "key.mouse.8"),
];

fn lwjgl2_to_name(code: i32) -> Option<&'static str> {
    LWJGL2_KEYS.iter().find(|(c, _)| *c == code).map(|(_, name)| *name)
}

fn name_to_lwjgl2(name: &str) -> Option<i32> {
    LWJGL2_KEYS.iter().find(|(_, n)| *n == name).map(|(code, _)| *code)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODERN_NEOFORGE: &str = "version:3955\nao:true\nfov:0.75\ngraphicsMode:1\nrenderClouds:\"true\"\nmainHand:\"right\"\nlang:es_es\nresourcePacks:[\"fabric\"]\nlastServer:example.invalid\nkey_key.forward:key.keyboard.w\nkey_key.jei.toggleOverlay:key.keyboard.o:CONTROL\nkey_key.attack:key.mouse.left\n";

    fn global() -> Canonical {
        capture(&OptionsText::parse(MODERN_NEOFORGE))
    }

    #[test]
    fn capture_skips_instance_keys_and_keeps_modern_values() {
        let global = global();
        assert!(!global.contains_key("version"));
        assert!(!global.contains_key("resourcePacks"));
        assert!(!global.contains_key("lastServer"));
        assert_eq!(global["renderClouds"], "\"true\"");
        assert_eq!(global["key_key.jei.toggleOverlay"], "key.keyboard.o:CONTROL");
    }

    #[test]
    fn round_trip_on_the_same_format_changes_nothing() {
        let mut file = OptionsText::parse(MODERN_NEOFORGE);
        assert_eq!(apply(&mut file, &global(), Loader::NeoForge, None), ApplyOutcome::Unchanged);
        assert_eq!(file.render(), MODERN_NEOFORGE);
    }

    #[test]
    fn fabric_drops_modifier_bindings_but_keeps_plain_ones() {
        let mut file = OptionsText::parse(
            "version:3955\nkey_key.forward:key.keyboard.z\nkey_key.jei.toggleOverlay:key.keyboard.o\n",
        );
        apply(&mut file, &global(), Loader::Fabric, None);
        assert_eq!(file.get("key_key.forward"), Some("key.keyboard.w"));
        assert_eq!(file.get("key_key.jei.toggleOverlay"), Some("key.keyboard.o"));
    }

    #[test]
    fn old_versions_get_old_encodings() {
        let mut file = OptionsText::parse(
            "version:1343\nao:1\nfancyGraphics:false\nrenderClouds:fast\nlang:en_US\nkey_key.forward:30\n",
        );
        apply(&mut file, &global(), Loader::Forge, None);
        assert_eq!(file.get("ao"), Some("2"));
        assert_eq!(file.get("fancyGraphics"), Some("true"));
        assert_eq!(file.get("graphicsMode"), None);
        assert_eq!(file.get("renderClouds"), Some("true"));
        assert_eq!(file.get("mainHand"), Some("right"));
        assert_eq!(file.get("lang"), Some("es_ES"));
        assert_eq!(file.get("key_key.forward"), Some("17"));
        assert_eq!(file.get("key_key.attack"), Some("-100"));
        assert_eq!(file.get("version"), Some("1343"));
    }

    #[test]
    fn old_files_capture_into_modern_form() {
        let captured = capture(&OptionsText::parse(
            "version:1343\nao:0\nfancyGraphics:true\nmainHand:left\nlang:en_US\nkey_key.jump:57\n",
        ));
        assert_eq!(captured["ao"], "false");
        assert_eq!(captured["graphicsMode"], "1");
        assert_eq!(captured["mainHand"], "\"left\"");
        assert_eq!(captured["lang"], "en_us");
        assert_eq!(captured["key_key.jump"], "key.keyboard.space");
    }

    #[test]
    fn graphics_presets_are_left_to_versions_that_use_them() {
        let mut newest = OptionsText::parse("version:5126\ngraphicsPreset:\"custom\"\nimprovedTransparency:false\n");
        apply(&mut newest, &global(), Loader::Vanilla, None);
        assert_eq!(newest.get("graphicsMode"), None);
        assert_eq!(newest.get("graphicsPreset"), Some("\"custom\""));

        let captured = capture(&OptionsText::parse("version:5126\ngraphicsPreset:\"fabulous\"\n"));
        let mut older = OptionsText::parse(MODERN_NEOFORGE);
        apply(&mut older, &captured, Loader::NeoForge, None);
        assert_eq!(older.get("graphicsMode"), Some("2"));
        assert_eq!(older.get("graphicsPreset"), None);
    }

    #[test]
    fn a_new_instance_needs_its_data_version() {
        let mut file = OptionsText::default();
        assert_eq!(apply(&mut file, &global(), Loader::NeoForge, None), ApplyOutcome::NeedsDataVersion);
        assert!(file.is_empty());
        assert!(matches!(
            apply(&mut file, &global(), Loader::NeoForge, Some(3955)),
            ApplyOutcome::Changed(_)
        ));
        assert_eq!(file.render().lines().next(), Some("version:3955"));
    }

    #[test]
    fn gamma_is_clamped_where_the_game_would_reset_it() {
        let mut global = Canonical::new();
        global.insert("gamma".into(), "5.0".into());
        let mut modern = OptionsText::parse("version:3955\ngamma:0.5\n");
        apply(&mut modern, &global, Loader::Vanilla, None);
        assert_eq!(modern.get("gamma"), Some("1.0"));
        let mut old = OptionsText::parse("version:1343\ngamma:0.5\n");
        apply(&mut old, &global, Loader::Vanilla, None);
        assert_eq!(old.get("gamma"), Some("5.0"));
    }
}
