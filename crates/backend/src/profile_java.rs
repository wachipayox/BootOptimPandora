//! Java of a profile: `profile.java_runtime` in the signed manifest (BootOptimDistribution
//! `docs/PROFILE_PROTOCOL.md`, «Java del perfil»).
//!
//! Each profile says, per platform, whether the launcher uses Mojang's Java for the Minecraft
//! version (`mojang`, also the value when nobody in the chain says anything) or a Java major the
//! profile recommends (`profile` + `major`). Pandora reads only `pc`. The choice is resolved over
//! the verified chain when the profile is applied and stored in the instance's committed branch
//! state, so Start never contacts the server.

use serde::{Deserialize, Deserializer};

/// `profile.java_runtime`: at least one platform; `{}` and `null` are rejected (omit it to inherit).
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ManifestJavaRuntime {
    #[serde(default, deserialize_with = "present")]
    pub pc: Option<ManifestJavaPlatform>,
    #[serde(default, deserialize_with = "present")]
    pub android: Option<ManifestJavaPlatform>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum JavaSource {
    Mojang,
    Profile,
}

/// One platform's choice. `major` is required with `profile` and forbidden with `mojang`.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ManifestJavaPlatform {
    pub source: JavaSource,
    #[serde(default, deserialize_with = "present")]
    pub major: Option<u32>,
}

pub const MIN_MAJOR: u32 = 8;
pub const MAX_MAJOR: u32 = 99;

/// A key that is present must hold a value: `null` fails instead of reading as "absent".
pub(crate) fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

impl ManifestJavaPlatform {
    pub const MOJANG: Self = Self {
        source: JavaSource::Mojang,
        major: None,
    };

    fn is_valid(&self) -> bool {
        match (self.source, self.major) {
            (JavaSource::Mojang, None) => true,
            (JavaSource::Profile, Some(major)) => (MIN_MAJOR..=MAX_MAJOR).contains(&major),
            _ => false,
        }
    }

    /// The Java major the profile recommends, or `None` for Mojang's Java.
    pub fn profile_major(&self) -> Option<u32> {
        match self.source {
            JavaSource::Profile => self.major,
            JavaSource::Mojang => None,
        }
    }
}

impl ManifestJavaRuntime {
    pub fn is_valid(&self) -> bool {
        (self.pc.is_some() || self.android.is_some())
            && self.pc.as_ref().is_none_or(ManifestJavaPlatform::is_valid)
            && self.android.as_ref().is_none_or(ManifestJavaPlatform::is_valid)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Pc,
    Android,
}

/// Resolves one platform over a chain ordered from the target (descendant) to the root: the first
/// revision that names the platform wins; if none does, Mojang. Also returns the profile id that
/// decided it, if any.
pub fn resolve<'a>(
    chain: impl IntoIterator<Item = (&'a str, Option<&'a ManifestJavaRuntime>)>,
    platform: Platform,
) -> (ManifestJavaPlatform, Option<&'a str>) {
    chain
        .into_iter()
        .find_map(|(profile_id, java_runtime)| {
            let java_runtime = java_runtime?;
            let choice = match platform {
                Platform::Pc => java_runtime.pc,
                Platform::Android => java_runtime.android,
            }?;
            Some((choice, Some(profile_id)))
        })
        .unwrap_or((ManifestJavaPlatform::MOJANG, None))
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;
    use crate::distribution::ManifestProfile;

    const VECTORS: &str = include_str!("testdata/java-runtime-vectors.json");

    fn vectors() -> Value {
        serde_json::from_str(VECTORS).unwrap()
    }

    /// Reads a case the way a revision is read: as the signed `profile` object, then validated.
    fn read_case(case: &Value) -> Result<ManifestProfile, String> {
        let mut profile = case.as_object().unwrap().clone();
        profile.remove("why");
        profile.insert("id".into(), "profile_test".into());
        profile.insert("name".into(), "Test".into());
        profile.insert("official".into(), false.into());
        let profile: ManifestProfile =
            serde_json::from_value(Value::Object(profile)).map_err(|error| error.to_string())?;
        if profile.java_runtime.as_ref().is_some_and(|java| !java.is_valid()) {
            return Err("invalid java_runtime".into());
        }
        Ok(profile)
    }

    #[test]
    fn shared_vectors_are_accepted_and_rejected_like_the_server() {
        let vectors = vectors();
        for case in vectors["accept"].as_array().unwrap() {
            let profile = read_case(case).unwrap_or_else(|error| panic!("{case} must be accepted: {error}"));
            assert!(profile.java_runtime.is_some());
        }
        for case in vectors["reject"].as_array().unwrap() {
            assert!(read_case(case).is_err(), "{case} must be rejected");
        }
    }

    #[test]
    fn a_profile_without_the_field_is_still_read() {
        let profile = read_case(&serde_json::json!({})).unwrap();
        assert_eq!(profile.java_runtime, None);
    }

    #[test]
    fn shared_vectors_resolve_like_the_server() {
        let vectors = vectors();
        for case in vectors["resolve"].as_array().unwrap() {
            let chain: Vec<(String, Option<ManifestJavaRuntime>)> = case["chain"]
                .as_array()
                .unwrap()
                .iter()
                .map(|revision| {
                    let java_runtime = revision
                        .get("java_runtime")
                        .map(|java| serde_json::from_value::<ManifestJavaRuntime>(java.clone()).unwrap());
                    (revision["profile_id"].as_str().unwrap().to_owned(), java_runtime)
                })
                .collect();
            for (key, platform) in [("pc", Platform::Pc), ("android", Platform::Android)] {
                let (choice, from) = resolve(chain.iter().map(|(id, java)| (id.as_str(), java.as_ref())), platform);
                let expected = &case[key];
                let mut expected_choice = expected.as_object().unwrap().clone();
                let expected_from = expected_choice.remove("from_profile_id");
                let expected_choice: ManifestJavaPlatform =
                    serde_json::from_value(Value::Object(expected_choice)).unwrap();
                assert_eq!(choice, expected_choice, "{} ({key})", case["name"]);
                assert_eq!(from, expected_from.as_ref().and_then(Value::as_str), "{} ({key})", case["name"]);
            }
        }
    }

    #[test]
    fn profile_major_is_none_for_mojang() {
        assert_eq!(ManifestJavaPlatform::MOJANG.profile_major(), None);
        let profile = ManifestJavaPlatform {
            source: JavaSource::Profile,
            major: Some(25),
        };
        assert_eq!(profile.profile_major(), Some(25));
    }
}
