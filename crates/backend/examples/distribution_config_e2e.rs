use std::{error::Error, fs, path::PathBuf, time::SystemTime};

use backend::{
    distribution::{DistributionClient, ManifestConfigSetting, selected_revision},
    profile_branch::ProfileBranchManifest,
};
use schema::backend_config::DistributionConfig;

const ROOT_PROFILE: &str = "profile_wachiland-config-rules-e2e-root";
const CHILD_PROFILE: &str = "profile_wachiland-config-rules-e2e-child";
const PUBLIC_KEY: &str = "q8uG8c8IBO-N7tnRWeKKqpl7pSj6G7FvKuRiCycpYwk";

fn setting<'a>(settings: &'a [ManifestConfigSetting], path: &str, key: &str) -> &'a ManifestConfigSetting {
    settings
        .iter()
        .find(|rule| rule.path == path && rule.key == key)
        .expect("missing effective rule")
}

fn check_rule(settings: &[ManifestConfigSetting], path: &str, key: &str, policy: &str, value: &str) {
    let rule = setting(settings, path, key);
    assert_eq!(rule.policy, policy, "wrong policy for {path}:{key}");
    assert_eq!(
        rule.value.as_str().map(str::to_owned).or_else(|| Some(rule.value.to_string())),
        Some(value.to_owned())
    );
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = DistributionConfig {
        base_url: "https://welite.ddns.net:8444".into(),
        release_key_id: "wachiland-release-2026-09".into(),
        release_public_key_base64url: PUBLIC_KEY.into(),
        ..DistributionConfig::default()
    };
    let client = DistributionClient::new(&config)?;
    let profiles = client.list_profiles().await?;
    let root = profiles
        .iter()
        .find(|profile| profile.profile_id == ROOT_PROFILE)
        .expect("root profile not listed");
    let child = profiles
        .iter()
        .find(|profile| profile.profile_id == CHILD_PROFILE)
        .expect("child profile not listed");
    let cache = std::env::temp_dir().join(format!(
        "wachiland-config-rules-e2e-{}",
        SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?.as_nanos()
    ));
    fs::create_dir_all(&cache)?;

    let root_resolved = client.resolve_profile(ROOT_PROFILE, selected_revision(root), &cache).await?;
    let child_resolved = client.resolve_profile(CHILD_PROFILE, selected_revision(child), &cache).await?;
    assert_eq!(root_resolved.entries.len(), 6, "root should resolve all 6 staged synthetic files");
    assert_eq!(child_resolved.entries.len(), 6, "child should inherit all 6 synthetic files");
    assert_eq!(child_resolved.config_settings.len(), 6, "child should resolve 6 effective config rules");

    let toml = "config/bootoptim-synthetic.toml";
    let properties = "config/bootoptim-synthetic.properties";
    let text = "config/bootoptim-synthetic.txt";
    check_rule(&root_resolved.config_settings, toml, "enabled", "enforced", "true");
    check_rule(&root_resolved.config_settings, toml, "level", "default_once", "1");
    check_rule(&root_resolved.config_settings, properties, "feature.enabled", "enforced", "true");
    check_rule(&root_resolved.config_settings, properties, "menu.label", "default_once", "Root profile");
    check_rule(&root_resolved.config_settings, text, "line:1", "enforced", "Synthetic root profile");
    check_rule(&root_resolved.config_settings, text, "line:2", "default_once", "Keep this line");

    for path in [toml, properties, text] {
        let published = fs::read(&root_resolved.entries.iter().find(|entry| entry.path == path).unwrap().source)?;
        let merged = root_resolved.merge_config_file(path, &published, None, &mut ProfileBranchManifest::default())?;
        assert!(!merged.bytes.is_empty(), "first install produced empty {path}");
    }

    check_rule(&child_resolved.config_settings, toml, "enabled", "enforced", "false");
    check_rule(&child_resolved.config_settings, toml, "level", "default_once", "2");
    check_rule(&child_resolved.config_settings, properties, "feature.enabled", "enforced", "false");
    check_rule(&child_resolved.config_settings, properties, "menu.label", "default_once", "Child profile");
    check_rule(&child_resolved.config_settings, text, "line:1", "enforced", "Synthetic child profile");
    check_rule(&child_resolved.config_settings, text, "line:2", "default_once", "Child keep");

    let mut branch = ProfileBranchManifest::default();
    let mut first_install = std::collections::BTreeMap::new();
    for path in [toml, properties, text] {
        let published = fs::read(&child_resolved.entries.iter().find(|entry| entry.path == path).unwrap().source)?;
        let merged = child_resolved.merge_config_file(path, &published, None, &mut branch)?;
        first_install.insert(path, merged.bytes);
    }
    let toml_live = String::from_utf8(first_install[toml].clone())?
        .replace("level = 2", "level = 99")
        .replace("level=2", "level=99");
    let properties_live = String::from_utf8(first_install[properties].clone())?
        .replace("menu.label=Child profile", "menu.label=Local label");
    let text_live = String::from_utf8(first_install[text].clone())?.replace("Child keep", "Local keep");
    let cases: [(&str, &str); 3] = [(toml, &toml_live), (properties, &properties_live), (text, &text_live)];
    for (path, live) in cases {
        let published = fs::read(&child_resolved.entries.iter().find(|entry| entry.path == path).unwrap().source)?;
        let merged = child_resolved.merge_config_file(path, &published, Some(live.as_bytes()), &mut branch)?;
        let output = String::from_utf8(merged.bytes)?;
        match path {
            p if p == toml => {
                assert!(output.contains("enabled = false") || output.contains("enabled=false"));
                assert!(output.contains("level = 99") || output.contains("level=99"));
            },
            p if p == properties => {
                assert!(output.contains("feature.enabled=false"));
                assert!(output.contains("menu.label=Local label"));
            },
            _ => {
                assert!(output.contains("Synthetic child profile"));
                assert!(output.contains("Local keep"));
            },
        }
    }
    assert_eq!(
        branch.initialized_config_settings.len(),
        3,
        "first-install state should persist all three default_once selectors"
    );
    fs::remove_dir_all(PathBuf::from(cache))?;
    println!(
        "PASS: signed root and child revisions verified; 6 inherited files; three formats merge on first install; inherited mandatory rules replace; default_once values preserve local edits on later merge."
    );
    Ok(())
}
