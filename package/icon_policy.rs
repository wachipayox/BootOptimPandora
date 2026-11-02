//! Shared by Cargo resource/asset selection and the packaging scripts.
use std::env;

pub fn stable_version(version: &str) -> bool {
    let core = version.split('+').next().unwrap_or("");
    let parts: Vec<_> = core.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|c| c.is_ascii_digit())
                && (part.len() == 1 || !part.starts_with('0'))
                && part.parse::<u16>().is_ok()
        })
}

pub fn development(profile: &str, version: Option<&str>, ci: bool, release_tag: bool) -> bool {
    profile != "release" || !version.is_some_and(stable_version) || (ci && !release_tag)
}

pub fn selected_development() -> bool {
    let ci = ["CI", "GITHUB_ACTIONS"]
        .iter()
        .any(|key| env::var(key).is_ok_and(|v| !v.is_empty() && v != "false" && v != "0"));
    let version = env::var("PANDORA_RELEASE_VERSION").ok();
    let release_tag = env::var("GITHUB_REF_TYPE").as_deref() == Ok("tag")
        && env::var("GITHUB_EVENT_NAME").as_deref() != Ok("pull_request")
        && version
            .as_deref()
            .is_some_and(|v| env::var("GITHUB_REF_NAME").is_ok_and(|tag| tag == v || tag == format!("v{v}")));
    development(&env::var("PROFILE").unwrap_or_default(), version.as_deref(), ci, release_tag)
}

pub fn emit_rerun_inputs() {
    for key in [
        "PROFILE",
        "PANDORA_RELEASE_VERSION",
        "CI",
        "GITHUB_ACTIONS",
        "GITHUB_REF_TYPE",
        "GITHUB_REF_NAME",
        "GITHUB_EVENT_NAME",
    ] {
        println!("cargo:rerun-if-env-changed={key}");
    }
    println!("cargo:rerun-if-changed=../../package/icon_policy.rs");
}

// Also compiled directly by package_icon_root.sh; no extra build dependency.
#[allow(dead_code)]
fn main() {
    println!(
        "{}",
        if selected_development() {
            "package/development"
        } else {
            "package"
        }
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn build_type_controls_branding() {
        assert!(!development("release", Some("1.0.0"), false, false));
        assert!(!development("release", Some("1.0.0"), true, true));
        assert!(development("release", Some("1.0.0"), true, false));
        assert!(development("debug", Some("1.0.0"), false, false));
        assert!(development("release", None, false, false));
        assert!(development("release", Some("1.0.0-nightly.1"), false, false));
        assert!(development("release", Some("1.0.0-rc.1"), false, false));
    }
    #[test]
    fn stable_versions_fit_windows_version_components() {
        for valid in ["0.1.0", "1.0.0", "1.0.0+build.1"] {
            assert!(stable_version(valid));
        }
        for invalid in ["", "1.0", "01.0.0", "1.0.65536", "1.0.0-nightly", "v1.0.0"] {
            assert!(!stable_version(invalid));
        }
    }
}
