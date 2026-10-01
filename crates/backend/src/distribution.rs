//! Secure read-side client for the private BootOptim Distribution service.
//!
//! Publication identity follows the configured, certificate-verified HTTPS origin.
//! Manifests and content objects still undergo signature, digest and schema validation.
//! Previously pinned signing keys cannot be silently replaced by discovery.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use crate::profile_branch::{
    EffectiveProfileEntry, GlobalRevisionPin, ProfileEntryMetadata, ProfileEntryOrigin, ProfileEntryOwnership,
    ProfileFilePolicy,
};
use bridge::modal_action::{ModalAction, ProgressTracker, ProgressTrackerFinishType};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::{Certificate, Client, Url};
use schema::backend_config::DistributionConfig;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

const MAX_JSON_BYTES: usize = 4 * 1024 * 1024;
const MAX_OBJECT_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum DistributionError {
    #[error("Distribution HTTPS address is missing or invalid")]
    InvalidBaseUrl,
    #[error("Distribution TLS trust file could not be loaded: {0}")]
    TlsTrust(String),
    #[error("Distribution trusted release signing keys are missing or invalid")]
    InvalidSigningKey,
    #[error("Distribution request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("Distribution returned HTTP {0}")]
    HttpStatus(reqwest::StatusCode),
    #[error("Distribution response exceeded the configured size limit")]
    ResponseTooLarge,
    #[error("Distribution response was invalid: {0}")]
    InvalidResponse(String),
    #[error("Revision signature or digest verification failed")]
    InvalidSignature,
    #[error("Content object {0} failed SHA-256/size verification")]
    InvalidObject(String),
    #[error("Local content cache I/O failed: {0}")]
    CacheIo(#[from] io::Error),
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct GlobalProfileSummary {
    #[serde(default)]
    pub presentation: Option<ProfilePresentation>,
    pub profile_id: String,
    pub name: String,
    pub latest_revision: RevisionRef,
    #[serde(default)]
    pub channels: Vec<ChannelRef>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ProfilePresentation {
    pub name: String,
    pub description: String,
    pub icon: Option<ManifestObjectRef>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct RevisionRef {
    pub revision_id: String,
    pub sequence: i64,
    pub manifest_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ChannelRef {
    pub name: String,
    #[serde(flatten)]
    pub revision: RevisionRef,
}

#[derive(Debug, Deserialize)]
struct ProfilesResponse {
    schema_version: u32,
    protocol_version: u32,
    profiles: Vec<GlobalProfileSummary>,
    #[serde(default)]
    truncated: bool,
}

#[derive(Debug, Deserialize)]
pub struct DistributionServiceInfo {
    pub service: String,
    pub version: String,
    pub commit: String,
    pub protocol_schema: u32,
    pub capabilities: Vec<String>,
    pub server_time_utc: String,
}

/// Check HTTPS reachability and the service protocol without requiring the release-signing key.
/// This lets the settings page diagnose URL, TLS, and server-version problems before the
/// administrator has completed the separate profile-signing trust setup.
pub async fn probe_distribution_service(
    config: &DistributionConfig,
) -> Result<DistributionServiceInfo, DistributionError> {
    let base_url = parse_base_url(&config.base_url)?;
    let client = build_http_client(config)?;
    let response = client.get(join_url(&base_url, "/v1/meta/version")?).send().await?;
    let info: DistributionServiceInfo = decode_json_response(response).await?;
    if info.service != "bootoptim-distribution" || info.protocol_schema != 1 {
        return Err(DistributionError::InvalidResponse(
            "endpoint is not a compatible BootOptim Distribution service".into(),
        ));
    }
    Ok(info)
}

#[derive(Debug, Deserialize)]
struct RevisionResponse {
    schema_version: u32,
    protocol_version: u32,
    envelope: SignedRevisionEnvelope,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SignedRevisionEnvelope {
    canonicalization: String,
    manifest_sha256: String,
    manifest: serde_json::Value,
    signature: ReleaseSignature,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleaseSignature {
    key_id: String,
    algorithm: String,
    value: String,
}

/// A manifest subset that is deliberately schema-versioned and strict. Unsupported fields fail
/// closed so a newer server cannot silently change destination or policy semantics.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlobalRevisionManifest {
    pub schema_version: u32,
    pub revision: ManifestRevision,
    pub profile: ManifestProfile,
    pub game: ManifestGame,
    pub base: Option<ManifestParent>,
    pub permissions: ManifestPermissions,
    #[serde(default)]
    pub mods: Vec<ManifestMod>,
    #[serde(default)]
    pub remove_mods: Vec<ManifestRemoveMod>,
    #[serde(default)]
    pub configs: Vec<ManifestConfig>,
    #[serde(default)]
    pub config_settings: Vec<ManifestConfigSetting>,
    #[serde(default)]
    pub remove_configs: Vec<ManifestRemoveConfig>,
    #[serde(default)]
    pub objects: Vec<ManifestObject>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestRevision {
    pub id: String,
    pub sequence: i64,
    pub created_at: String,
    pub release_notes: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestProfile {
    #[serde(default)]
    pub description: String,
    pub icon: Option<ManifestObjectRef>,
    pub id: String,
    pub name: String,
    pub official: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ManifestGame {
    pub minecraft: String,
    pub neoforge: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestParent {
    pub profile_id: String,
    pub revision_id: String,
    pub manifest_sha256: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestPermissions {
    pub derive_local: bool,
    pub mods: ManifestModPermissions,
    pub configs: ManifestConfigPermissions,
    pub max_inheritance_depth: usize,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestModPermissions {
    pub add: bool,
    pub remove: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestConfigPermissions {
    pub override_enforced: bool,
    pub override_default_once: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ManifestObjectRef {
    pub sha256: String,
    pub size: u64,
    #[serde(default)]
    pub media_type: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestMod {
    pub id: String,
    pub path: String,
    pub object: ManifestObjectRef,
    pub expect_base_object_sha256: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestRemoveMod {
    pub id: String,
    pub expect_base_object_sha256: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestConfig {
    pub path: String,
    pub object: ManifestObjectRef,
    pub policy: String,
    pub expect_base_object_sha256: Option<String>,
}

/// A stable-key config override. Child revisions replace the rule with the same
/// path/key pair while resolving oldest ancestor to newest descendant.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ManifestConfigSetting {
    pub path: String,
    pub format: String,
    pub key: String,
    pub value: serde_json::Value,
    pub policy: String,
}

impl ManifestConfigSetting {
    pub fn identity(&self) -> String {
        format!("{}\0{}", self.path, self.key)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestRemoveConfig {
    pub path: String,
    pub expect_base_object_sha256: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestObject {
    pub id: String,
    pub path: String,
    pub object: ManifestObjectRef,
    pub expect_base_object_sha256: Option<String>,
}

#[derive(Clone, Debug)]
pub struct VerifiedRevision {
    pub manifest_sha256: String,
    pub manifest: GlobalRevisionManifest,
}

#[derive(Clone, Debug)]
pub struct ResolvedGlobalProfile {
    pub icon_path: Option<PathBuf>,
    pub pin: GlobalRevisionPin,
    pub name: String,
    pub minecraft_version: String,
    pub neoforge_version: String,
    pub entries: Vec<EffectiveProfileEntry>,
    /// Effective per-setting rules after applying ancestor-to-descendant overrides.
    pub config_settings: Vec<ManifestConfigSetting>,
}

impl ResolvedGlobalProfile {
    /// Merge the profile's effective format-specific rules into one config file and update
    /// the local first-install markers that belong in the persistent branch state.
    pub fn merge_config_file(
        &self,
        path: &str,
        published_contents: &[u8],
        live_contents: Option<&[u8]>,
        branch: &mut crate::profile_branch::ProfileBranchManifest,
    ) -> Result<crate::config_settings::ConfigMergeResult, crate::config_settings::ConfigSettingError> {
        let result = crate::config_settings::merge_config_settings(
            path,
            published_contents,
            live_contents,
            &branch.initialized_config_settings,
            &self.config_settings,
        )?;
        branch.initialized_config_settings = result.initialized_default_once.clone();
        Ok(result)
    }

    /// Backwards-compatible TOML entry point.
    pub fn merge_toml_config(
        &self,
        path: &str,
        published_contents: &[u8],
        live_contents: Option<&[u8]>,
        branch: &mut crate::profile_branch::ProfileBranchManifest,
    ) -> Result<crate::config_settings::ConfigMergeResult, crate::config_settings::ConfigSettingError> {
        self.merge_config_file(path, published_contents, live_contents, branch)
    }
}

#[derive(Clone)]
pub struct DistributionClient {
    client: Client,
    base_url: Url,
    trusted_keys: BTreeMap<String, [u8; 32]>,
    server_keys: Arc<tokio::sync::OnceCell<BTreeMap<String, [u8; 32]>>>,
}

#[derive(Deserialize)]
struct ServerSigningKeys {
    schema_version: u32,
    keys: BTreeMap<String, String>,
}

impl ServerSigningKeys {
    fn validated(self, pinned: &BTreeMap<String, [u8; 32]>) -> Result<BTreeMap<String, [u8; 32]>, DistributionError> {
        if self.schema_version != 1 || self.keys.is_empty() || self.keys.len() > 128 {
            return Err(DistributionError::InvalidSigningKey);
        }
        let mut keys = BTreeMap::new();
        for (id, encoded) in self.keys {
            insert_trusted_release_key(&mut keys, &id, &encoded)?;
            if pinned.get(&id).is_some_and(|expected| keys.get(&id) != Some(expected)) {
                return Err(DistributionError::InvalidSigningKey);
            }
        }
        Ok(keys)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdditionalReleaseKey {
    key_id: String,
    public_key_base64url: String,
}

impl DistributionClient {
    pub fn new(config: &DistributionConfig) -> Result<Self, DistributionError> {
        let base_url = parse_base_url(&config.base_url)?;

        let trusted_keys = if config.release_key_id.trim().is_empty()
            && config.release_public_key_base64url.trim().is_empty()
            && config.additional_release_keys_json.trim().is_empty() {
            BTreeMap::new()
        } else {
            trusted_release_keys(config)?
        };

        let client = build_http_client(config)?;

        Ok(Self {
            client,
            base_url,
            trusted_keys,
            server_keys: Arc::new(tokio::sync::OnceCell::new()),
        })
    }

    pub async fn list_profiles(&self) -> Result<Vec<GlobalProfileSummary>, DistributionError> {
        let response: ProfilesResponse = self.get_json("/v1/profiles").await?;
        if response.schema_version != 1 || response.protocol_version != 1 || response.truncated {
            return Err(DistributionError::InvalidResponse("unsupported or truncated profile catalog".into()));
        }
        let mut profile_ids = BTreeSet::new();
        for profile in &response.profiles {
            if !valid_identifier(&profile.profile_id, "profile_")
                || !valid_identifier(&profile.latest_revision.revision_id, "rev_")
                || !valid_digest(&profile.latest_revision.manifest_sha256)
                || profile.latest_revision.sequence <= 0
                || profile.name.trim().is_empty()
                || !profile_ids.insert(profile.profile_id.as_str())
            {
                return Err(DistributionError::InvalidResponse("profile catalog contains an invalid identity".into()));
            }
            let mut channel_names = BTreeSet::new();
            for channel in &profile.channels {
                if channel.name.trim().is_empty()
                    || !valid_identifier(&channel.revision.revision_id, "rev_")
                    || !valid_digest(&channel.revision.manifest_sha256)
                    || channel.revision.sequence <= 0
                    || !channel_names.insert(channel.name.to_ascii_lowercase())
                {
                    return Err(DistributionError::InvalidResponse(
                        "profile catalog contains an invalid channel".into(),
                    ));
                }
            }
        }
        Ok(response.profiles)
    }

    pub async fn fetch_verified_revision(
        &self,
        profile_id: &str,
        revision: &RevisionRef,
    ) -> Result<VerifiedRevision, DistributionError> {
        self.fetch_revision(profile_id, &revision.revision_id, &revision.manifest_sha256, Some(revision.sequence))
            .await
    }

    async fn fetch_revision(
        &self,
        profile_id: &str,
        revision_id: &str,
        expected_digest: &str,
        expected_sequence: Option<i64>,
    ) -> Result<VerifiedRevision, DistributionError> {
        if !valid_identifier(profile_id, "profile_")
            || !valid_identifier(revision_id, "rev_")
            || !valid_digest(expected_digest)
        {
            return Err(DistributionError::InvalidResponse("invalid requested revision pin".into()));
        }
        let path = format!("/v1/profiles/{profile_id}/revisions/{revision_id}");
        let response: RevisionResponse = self.get_json(&path).await?;
        if response.schema_version != 1 || response.protocol_version != 1 {
            return Err(DistributionError::InvalidResponse("unsupported revision protocol".into()));
        }

        let envelope = response.envelope;
        if envelope.canonicalization != "RFC8785-JCS"
            || envelope.manifest_sha256 != expected_digest
            || envelope.signature.algorithm != "Ed25519"
        {
            return Err(DistributionError::InvalidSignature);
        }
        let manifest: GlobalRevisionManifest = serde_json::from_value(envelope.manifest.clone())
            .map_err(|error| DistributionError::InvalidResponse(error.to_string()))?;
        // The manifest schema is strict and its property names are fixed ASCII strings. For that
        // schema, serde_json's sorted map order matches JCS UTF-16 key ordering. Its compact
        // serializer emits non-control Unicode directly, as JCS requires. Protocol numbers are
        // integers, avoiding cross-runtime floating-point formatting differences.
        let mut canonical = Vec::new();
        let mut serializer = serde_json::Serializer::new(&mut canonical);
        envelope
            .manifest
            .serialize(&mut serializer)
            .map_err(|error| DistributionError::InvalidResponse(error.to_string()))?;
        let digest = hex::encode(Sha256::digest(&canonical));
        if digest != envelope.manifest_sha256 {
            return Err(DistributionError::InvalidSignature);
        }
        // Resolve unknown signing identities only through the configured HTTPS origin.
        let keys = if self.trusted_keys.contains_key(&envelope.signature.key_id) {
            &self.trusted_keys
        } else {
            self.server_keys.get_or_try_init(|| async {
                let response: ServerSigningKeys = self.get_json("/v1/signing-keys").await?;
                response.validated(&self.trusted_keys)
            }).await?
        };
        verify_release_signature(
            keys,
            &envelope.signature.key_id,
            &canonical,
            &envelope.signature.value,
        )?;
        if !(manifest.schema_version == 1 || manifest.schema_version == 2)
            || manifest.profile.id != profile_id
            || manifest.revision.id != revision_id
            || manifest.revision.sequence <= 0
            || expected_sequence.is_some_and(|sequence| manifest.revision.sequence != sequence)
        {
            return Err(DistributionError::InvalidResponse(
                "signed manifest identity does not match its catalog pin".into(),
            ));
        }
        Ok(VerifiedRevision {
            manifest_sha256: digest,
            manifest,
        })
    }

    /// Resolve and verify an exact revision and its complete pinned ancestry, then populate local
    /// content-addressed storage only for files present in the resulting effective profile.
    pub async fn resolve_profile(
        &self,
        profile_id: &str,
        revision: &RevisionRef,
        cache_root: &Path,
    ) -> Result<ResolvedGlobalProfile, DistributionError> {
        self.resolve_profile_with_reuse(profile_id, revision, cache_root, &BTreeMap::new(), &BTreeSet::new())
            .await
    }

    pub async fn resolve_profile_with_progress(
        &self,
        profile_id: &str,
        revision: &RevisionRef,
        cache_root: &Path,
        modal_action: &ModalAction,
        overall_progress: &ProgressTracker,
    ) -> Result<ResolvedGlobalProfile, DistributionError> {
        self.resolve_profile_with_reuse_and_progress(
            profile_id,
            revision,
            cache_root,
            &BTreeMap::new(),
            &BTreeSet::new(),
            Some((modal_action, overall_progress)),
        )
        .await
    }

    /// Resolve a revision while reusing files already tracked by the local profile branch.
    /// Paths in `skip_paths` must be filtered by the caller because local ownership/tombstones
    /// keep them from flowing through an inherited update.
    pub async fn resolve_profile_with_reuse(
        &self,
        profile_id: &str,
        revision: &RevisionRef,
        cache_root: &Path,
        reusable_entries: &BTreeMap<String, ProfileEntryMetadata>,
        skip_paths: &BTreeSet<String>,
    ) -> Result<ResolvedGlobalProfile, DistributionError> {
        self.resolve_profile_with_reuse_and_progress(profile_id, revision, cache_root, reusable_entries, skip_paths, None)
            .await
    }

    async fn resolve_profile_with_reuse_and_progress(
        &self,
        profile_id: &str,
        revision: &RevisionRef,
        cache_root: &Path,
        reusable_entries: &BTreeMap<String, ProfileEntryMetadata>,
        skip_paths: &BTreeSet<String>,
        progress: Option<(&ModalAction, &ProgressTracker)>,
    ) -> Result<ResolvedGlobalProfile, DistributionError> {
        let mut chain = Vec::<VerifiedRevision>::new();
        let mut next = Some((
            profile_id.to_owned(),
            revision.revision_id.clone(),
            revision.manifest_sha256.clone(),
            Some(revision.sequence),
        ));
        let mut seen = BTreeSet::<(String, String, String)>::new();
        while let Some((next_profile, next_revision, next_digest, sequence)) = next.take() {
            if chain.len() > 8 || !seen.insert((next_profile.clone(), next_revision.clone(), next_digest.clone())) {
                return Err(DistributionError::InvalidResponse(
                    "revision ancestry is cyclic or exceeds the depth limit".into(),
                ));
            }
            let verified = self.fetch_revision(&next_profile, &next_revision, &next_digest, sequence).await?;
            next =
                verified.manifest.base.as_ref().map(|base| {
                    (base.profile_id.clone(), base.revision_id.clone(), base.manifest_sha256.clone(), None)
                });
            chain.push(verified);
        }
        if chain.is_empty() {
            return Err(DistributionError::InvalidResponse("revision ancestry is empty".into()));
        }

        let target = chain.first().unwrap();
        let base_depth = chain.len() - 1;
        let signed_depth_limit = target.manifest.permissions.max_inheritance_depth.min(8);
        if base_depth > signed_depth_limit {
            return Err(DistributionError::InvalidResponse(
                "revision ancestry exceeds the signed inheritance limit".into(),
            ));
        }
        if chain.windows(2).any(|pair| pair[0].manifest.game != pair[1].manifest.game) {
            return Err(DistributionError::InvalidResponse(
                "a revision and its pinned parent use different game versions".into(),
            ));
        }
        let target_pin = GlobalRevisionPin::new(
            target.manifest.profile.id.clone(),
            target.manifest.revision.id.clone(),
            target.manifest_sha256.clone(),
        )
        .map_err(|error| DistributionError::InvalidResponse(error.to_string()))?;
        let target_name = target.manifest.profile.name.clone();
        let icon_path = match &target.manifest.profile.icon {
            Some(icon) => Some(self.fetch_verified_icon(icon, cache_root).await?),
            None => None,
        };
        let minecraft_version = target.manifest.game.minecraft.clone();
        let neoforge_version = target.manifest.game.neoforge.clone();

        let mut files = BTreeMap::<String, EffectiveProfileEntry>::new();
        let mut mod_paths = BTreeMap::<String, String>::new();
        let mut object_paths = BTreeMap::<String, String>::new();
        let mut object_refs = BTreeMap::<(String, String, String), &ManifestObjectRef>::new();
        let mut config_settings = BTreeMap::<(String, String), ManifestConfigSetting>::new();
        let mut parent_config_permissions: Option<ManifestConfigPermissions> = None;
        for revision in chain.iter().rev() {
            let pin = GlobalRevisionPin::new(
                revision.manifest.profile.id.clone(),
                revision.manifest.revision.id.clone(),
                revision.manifest_sha256.clone(),
            )
            .map_err(|error| DistributionError::InvalidResponse(error.to_string()))?;
            let manifest = &revision.manifest;
            if manifest.schema_version == 1 && !manifest.config_settings.is_empty() {
                return Err(DistributionError::InvalidResponse(
                    "per-setting config rules require manifest schema 2".into(),
                ));
            }
            let mut seen_settings = BTreeSet::new();
            for item in &manifest.mods {
                object_refs
                    .insert((format!("mod:{}", item.id), item.path.clone(), item.object.sha256.clone()), &item.object);
            }
            for item in &manifest.configs {
                object_refs.insert(
                    (format!("config:{}", item.path), item.path.clone(), item.object.sha256.clone()),
                    &item.object,
                );
            }
            for item in &manifest.config_settings {
                check_manifest_path(&item.path)?;
                if manifest.schema_version < 2
                    || !valid_config_setting(item)
                    || (item.policy != "enforced" && item.policy != "default_once")
                {
                    return Err(DistributionError::InvalidResponse("invalid per-setting config rule".into()));
                }
                let key = (item.path.clone(), item.key.clone());
                if !seen_settings.insert(key.clone()) {
                    return Err(DistributionError::InvalidResponse("duplicate per-setting config rule".into()));
                }
                if let Some(previous) = config_settings.get(&key) {
                    let permissions = parent_config_permissions.as_ref().ok_or_else(|| {
                        DistributionError::InvalidResponse("config-setting lineage is inconsistent".into())
                    })?;
                    let allowed = if previous.policy == "enforced" {
                        permissions.override_enforced
                    } else {
                        permissions.override_default_once
                    };
                    if !allowed {
                        return Err(DistributionError::InvalidResponse(
                            "parent permissions prohibit overriding this config setting".into(),
                        ));
                    }
                }
                config_settings.insert(key, item.clone());
            }
            for item in &manifest.objects {
                object_refs.insert(
                    (format!("object:{}", item.id), item.path.clone(), item.object.sha256.clone()),
                    &item.object,
                );
            }

            for item in &manifest.mods {
                check_manifest_path(&item.path)?;
                verify_expected_base(&files, &format!("mod:{}", item.id), item.expect_base_object_sha256.as_deref())?;
                if let Some(previous) = mod_paths.insert(item.id.clone(), item.path.clone()) {
                    if previous != item.path {
                        files.remove(&previous);
                    }
                }
                insert_manifest_entry(
                    &mut files,
                    &item.path,
                    format!("mod:{}", item.id),
                    &item.object,
                    &pin,
                    if is_initial_player_setting(&item.path) { ProfileFilePolicy::DefaultOnce } else { ProfileFilePolicy::Enforced },
                )?;
            }
            for item in &manifest.remove_mods {
                let Some(path) = mod_paths.remove(&item.id) else {
                    return Err(DistributionError::InvalidResponse("revision removes an unknown mod identity".into()));
                };
                let existing = files.get(&path).ok_or_else(|| {
                    DistributionError::InvalidResponse("removed mod is absent from its pinned parent".into())
                })?;
                if existing.metadata.source_sha256 != item.expect_base_object_sha256 {
                    return Err(DistributionError::InvalidResponse(
                        "removed mod does not match its declared parent digest".into(),
                    ));
                }
                files.remove(&path);
            }
            for item in &manifest.configs {
                check_manifest_path(&item.path)?;
                let policy = if is_initial_player_setting(&item.path) {
                    ProfileFilePolicy::DefaultOnce
                } else {
                    match item.policy.as_str() {
                        "enforced" => ProfileFilePolicy::Enforced,
                        "default_once" => ProfileFilePolicy::DefaultOnce,
                        _ => return Err(DistributionError::InvalidResponse("unsupported config policy".into())),
                    }
                };
                verify_expected_base(
                    &files,
                    &format!("config:{}", item.path),
                    item.expect_base_object_sha256.as_deref(),
                )?;
                insert_manifest_entry(
                    &mut files,
                    &item.path,
                    format!("config:{}", item.path),
                    &item.object,
                    &pin,
                    policy,
                )?;
            }
            for item in &manifest.remove_configs {
                let existing = files.get(&item.path).ok_or_else(|| {
                    DistributionError::InvalidResponse("revision removes a config absent from its pinned parent".into())
                })?;
                if existing.metadata.source_sha256 != item.expect_base_object_sha256 {
                    return Err(DistributionError::InvalidResponse(
                        "removed config does not match its declared parent digest".into(),
                    ));
                }
                files.remove(&item.path);
                config_settings.retain(|(path, _), _| path != &item.path);
            }
            for item in &manifest.objects {
                check_manifest_path(&item.path)?;
                verify_expected_base(
                    &files,
                    &format!("object:{}", item.id),
                    item.expect_base_object_sha256.as_deref(),
                )?;
                if let Some(previous) = object_paths.insert(item.id.clone(), item.path.clone()) {
                    if previous != item.path {
                        files.remove(&previous);
                    }
                }
                // These two root-level files are player preferences. Seed them only
                // when an instance is first created; never enforce pack revisions over
                // edits the player makes later.
                let policy = if is_initial_player_setting(&item.path) {
                    ProfileFilePolicy::DefaultOnce
                } else {
                    ProfileFilePolicy::Enforced
                };
                insert_manifest_entry(
                    &mut files,
                    &item.path,
                    format!("object:{}", item.id),
                    &item.object,
                    &pin,
                    policy,
                )?;
            }
            parent_config_permissions = Some(manifest.permissions.configs.clone());
        }

        for ((path, _), _) in &config_settings {
            if !files
                .get(path)
                .is_some_and(|entry| entry.metadata.logical_identity.starts_with("config:"))
            {
                return Err(DistributionError::InvalidResponse(
                    "per-setting config rule refers to a TOML config absent from the effective profile".into(),
                ));
            }
        }

        let mut total_asset_bytes = 0u64;
        let mut distinct_assets = BTreeMap::<String, u64>::new();
        if progress.is_some() {
            for entry in files.values() {
                let has_config_settings = config_settings.iter().any(|((path, _), _)| path == &entry.path);
                if skip_paths.contains(&entry.path)
                    || (!has_config_settings
                        && reusable_entries.get(&entry.path).is_some_and(|existing| {
                            existing.logical_identity == entry.metadata.logical_identity
                                && existing.source_sha256 == entry.metadata.source_sha256
                                && existing.policy == entry.metadata.policy
                        }))
                {
                    continue;
                }
                let object = object_refs
                    .get(&(
                        entry.metadata.logical_identity.clone(),
                        entry.path.clone(),
                        entry.metadata.source_sha256.clone(),
                    ))
                    .ok_or_else(|| {
                        DistributionError::InvalidResponse("effective entry has no signed object reference".into())
                    })?;
                distinct_assets.entry(object.sha256.clone()).or_insert(object.size);
            }
            total_asset_bytes = distinct_assets.values().fold(0u64, |total, size| total.saturating_add(*size));
            if let Some((_, overall)) = progress {
                overall.set_title("Downloading modpack assets".into());
                overall.set_count(0);
                overall.set_total(progress_units(total_asset_bytes).max(1));
            }
        }

        let mut counted_digests = BTreeSet::new();
        let mut entries_with_sources = Vec::with_capacity(files.len());
        for (_, mut entry) in files {
            let has_config_settings = config_settings.iter().any(|((path, _), _)| path == &entry.path);
            if skip_paths.contains(&entry.path)
                || (!has_config_settings
                    && reusable_entries.get(&entry.path).is_some_and(|existing| {
                        existing.logical_identity == entry.metadata.logical_identity
                            && existing.source_sha256 == entry.metadata.source_sha256
                            && existing.policy == entry.metadata.policy
                    }))
            {
                entries_with_sources.push(entry);
                continue;
            }
            let object = object_refs
                .get(&(
                    entry.metadata.logical_identity.clone(),
                    entry.path.clone(),
                    entry.metadata.source_sha256.clone(),
                ))
                .ok_or_else(|| {
                    DistributionError::InvalidResponse("effective entry has no signed object reference".into())
                })?;
            if let Some((modal_action, overall)) = progress {
                let first_reference = counted_digests.insert(object.sha256.clone());
                if let Some(cached) = verified_cache_path(object, cache_root)? {
                    if first_reference {
                        overall.add_count(progress_units(object.size));
                    }
                    entry.source = cached;
                } else {
                    let file_progress = modal_action.push_sub_tracker(entry.path.as_str().into());
                    file_progress.set_total(progress_units(object.size));
                    let aggregate_progress = first_reference.then_some(overall);
                    match self
                        .fetch_verified_object_with_progress(object, cache_root, Some(&file_progress), aggregate_progress)
                        .await
                    {
                        Ok(source) => {
                            file_progress.set_count(progress_units(object.size));
                            file_progress.set_finished(ProgressTrackerFinishType::Normal);
                            entry.source = source;
                        },
                        Err(error) => {
                            file_progress.set_finished(ProgressTrackerFinishType::Error);
                            return Err(error);
                        },
                    }
                }
            } else {
                entry.source = self.fetch_verified_object(object, cache_root).await?;
            }
            entries_with_sources.push(entry);
        }
        if let Some((_, overall)) = progress
            && total_asset_bytes == 0
        {
            overall.set_count(1);
        }
        Ok(ResolvedGlobalProfile {
            icon_path,
            pin: target_pin,
            name: target_name,
            minecraft_version,
            neoforge_version,
            entries: entries_with_sources,
            config_settings: config_settings.into_values().collect(),
        })
    }

    async fn fetch_verified_icon(&self, icon: &ManifestObjectRef, cache_root: &Path) -> Result<PathBuf, DistributionError> {
        if !valid_digest(&icon.sha256) || icon.size > 2 * 1024 * 1024 || icon.media_type != "image/png" {
            return Err(DistributionError::InvalidResponse("invalid signed profile icon".into()));
        }
        let path = self.fetch_verified_object(icon, cache_root).await?;
        let reader = image::ImageReader::open(&path)?.with_guessed_format()?;
        if reader.format() != Some(image::ImageFormat::Png) { return Err(DistributionError::InvalidResponse("profile icon is not PNG".into())); }
        let dimensions = reader.into_dimensions().map_err(|e| DistributionError::InvalidResponse(e.to_string()))?;
        if dimensions.0 == 0 || dimensions.1 == 0 || dimensions.0 > 1024 || dimensions.1 > 1024 { return Err(DistributionError::InvalidResponse("profile icon exceeds 1024 × 1024 pixels".into())); }
        Ok(path)
    }

    pub async fn fetch_verified_object(
        &self,
        object: &ManifestObjectRef,
        cache_root: &Path,
    ) -> Result<PathBuf, DistributionError> {
        self.fetch_verified_object_with_progress(object, cache_root, None, None).await
    }

    async fn fetch_verified_object_with_progress(
        &self,
        object: &ManifestObjectRef,
        cache_root: &Path,
        file_progress: Option<&ProgressTracker>,
        aggregate_progress: Option<&ProgressTracker>,
    ) -> Result<PathBuf, DistributionError> {
        if !valid_digest(&object.sha256) || object.size > MAX_OBJECT_BYTES {
            return Err(DistributionError::InvalidResponse("invalid or oversized object reference".into()));
        }
        let directory = cache_root.join("sha256").join(&object.sha256[..2]);
        if let Some(cached) = verified_cache_path(object, cache_root)? {
            return Ok(cached);
        }
        let destination = directory.join(&object.sha256);

        let url = self.object_url(&object.sha256)?;
        let mut response = self.client.get(url).send().await?;
        if !response.status().is_success() {
            return Err(DistributionError::HttpStatus(response.status()));
        }
        if response.content_length().is_some_and(|size| size != object.size) {
            return Err(DistributionError::InvalidObject(object.sha256.clone()));
        }

        let temporary = directory.join(format!(".{}.{}.tmp", object.sha256, uuid::Uuid::new_v4()));
        let result = async {
            let mut output = OpenOptions::new().write(true).create_new(true).open(&temporary)?;
            let mut hasher = Sha256::new();
            let mut received = 0u64;
            while let Some(chunk) = response.chunk().await? {
                received = received
                    .checked_add(chunk.len() as u64)
                    .ok_or_else(|| DistributionError::InvalidObject(object.sha256.clone()))?;
                if received > object.size || received > MAX_OBJECT_BYTES {
                    return Err(DistributionError::InvalidObject(object.sha256.clone()));
                }
                hasher.update(&chunk);
                output.write_all(&chunk)?;
                if let Some(progress) = file_progress {
                    progress.add_count(chunk.len());
                }
                if let Some(progress) = aggregate_progress {
                    progress.add_count(chunk.len());
                }
            }
            output.sync_all()?;
            let digest = hex::encode(hasher.finalize());
            if received != object.size || digest != object.sha256 {
                return Err(DistributionError::InvalidObject(object.sha256.clone()));
            }
            drop(output);
            fs::rename(&temporary, &destination)?;
            Ok(destination.clone())
        }
        .await;
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T, DistributionError> {
        decode_json_response(self.client.get(self.join(path)?).send().await?).await
    }

    fn object_url(&self, digest: &str) -> Result<Url, DistributionError> {
        if !valid_digest(digest) {
            return Err(DistributionError::InvalidResponse("invalid object digest".into()));
        }
        self.join(&format!("/v1/objects/sha256/{digest}"))
    }

    fn join(&self, path: &str) -> Result<Url, DistributionError> {
        join_url(&self.base_url, path)
    }
}

fn trusted_release_keys(config: &DistributionConfig) -> Result<BTreeMap<String, [u8; 32]>, DistributionError> {
    let mut keys = BTreeMap::new();
    let primary_id = config.release_key_id.trim();
    let primary_public = config.release_public_key_base64url.trim();
    match (primary_id.is_empty(), primary_public.is_empty()) {
        (true, true) => {},
        (false, false) => insert_trusted_release_key(&mut keys, primary_id, primary_public)?,
        _ => return Err(DistributionError::InvalidSigningKey),
    }

    if !config.additional_release_keys_json.trim().is_empty() {
        let additional: Vec<AdditionalReleaseKey> = serde_json::from_str(&config.additional_release_keys_json)
            .map_err(|_| DistributionError::InvalidSigningKey)?;
        for key in additional {
            insert_trusted_release_key(&mut keys, &key.key_id, &key.public_key_base64url)?;
        }
    }
    if keys.is_empty() {
        return Err(DistributionError::InvalidSigningKey);
    }
    Ok(keys)
}

fn insert_trusted_release_key(
    keys: &mut BTreeMap<String, [u8; 32]>,
    key_id: &str,
    encoded_public: &str,
) -> Result<(), DistributionError> {
    if key_id.is_empty() || key_id.len() > 128 || key_id.trim() != key_id {
        return Err(DistributionError::InvalidSigningKey);
    }
    let key_bytes = URL_SAFE_NO_PAD
        .decode(encoded_public.trim())
        .map_err(|_| DistributionError::InvalidSigningKey)?;
    let key_bytes: [u8; 32] = key_bytes.try_into().map_err(|_| DistributionError::InvalidSigningKey)?;
    if keys.insert(key_id.to_owned(), key_bytes).is_some() {
        return Err(DistributionError::InvalidSigningKey);
    }
    Ok(())
}

fn verify_release_signature(
    keys: &BTreeMap<String, [u8; 32]>,
    key_id: &str,
    message: &[u8],
    encoded_signature: &str,
) -> Result<(), DistributionError> {
    let verifying_key = keys.get(key_id).ok_or(DistributionError::InvalidSignature)?;
    let signature = URL_SAFE_NO_PAD
        .decode(encoded_signature)
        .map_err(|_| DistributionError::InvalidSignature)?;
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, verifying_key)
        .verify(message, &signature)
        .map_err(|_| DistributionError::InvalidSignature)
}

fn parse_base_url(value: &str) -> Result<Url, DistributionError> {
    let url = Url::parse(value.trim()).map_err(|_| DistributionError::InvalidBaseUrl)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || url.username() != ""
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(DistributionError::InvalidBaseUrl);
    }
    Ok(url)
}

fn build_http_client(config: &DistributionConfig) -> Result<Client, DistributionError> {
    let mut builder = Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(5 * 60))
        .redirect(reqwest::redirect::Policy::none());
    if !config.tls_ca_certificate_path.trim().is_empty() {
        let pem = fs::read(&config.tls_ca_certificate_path)
            .map_err(|error| DistributionError::TlsTrust(error.to_string()))?;
        let certificate =
            Certificate::from_pem(&pem).map_err(|error| DistributionError::TlsTrust(error.to_string()))?;
        builder = builder.add_root_certificate(certificate);
    }
    builder.build().map_err(DistributionError::Request)
}

fn join_url(base_url: &Url, path: &str) -> Result<Url, DistributionError> {
    let base = format!("{}/", base_url.as_str().trim_end_matches('/'));
    let base = Url::parse(&base).map_err(|_| DistributionError::InvalidBaseUrl)?;
    base.join(path.trim_start_matches('/')).map_err(|_| DistributionError::InvalidBaseUrl)
}

async fn decode_json_response<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
) -> Result<T, DistributionError> {
    if !response.status().is_success() {
        return Err(DistributionError::HttpStatus(response.status()));
    }
    if response.content_length().is_some_and(|length| length > MAX_JSON_BYTES as u64) {
        return Err(DistributionError::ResponseTooLarge);
    }
    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len().saturating_add(chunk.len()) > MAX_JSON_BYTES {
            return Err(DistributionError::ResponseTooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|error| DistributionError::InvalidResponse(error.to_string()))
}

fn valid_identifier(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|tail| {
        !tail.is_empty()
            && tail.len() <= 120
            && tail
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    })
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn valid_toml_dotted_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.split('.').all(|part| {
            !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        })
}

fn valid_toml_json_value(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null | serde_json::Value::Object(_) => false,
        serde_json::Value::Array(values) => values.iter().all(|value| {
            matches!(
                value,
                serde_json::Value::Bool(_) | serde_json::Value::Number(_) | serde_json::Value::String(_)
            )
        }),
        serde_json::Value::Bool(_) | serde_json::Value::Number(_) | serde_json::Value::String(_) => true,
    }
}

fn valid_config_setting(setting: &ManifestConfigSetting) -> bool {
    if setting.key.len() > 256 {
        return false;
    }
    let path = setting.path.to_ascii_lowercase();
    match setting.format.as_str() {
        "toml" => {
            path.ends_with(".toml") && valid_toml_dotted_key(&setting.key) && valid_toml_json_value(&setting.value)
        },
        "properties" => {
            path.ends_with(".properties")
                && !setting.key.is_empty()
                && setting
                    .key
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
                && setting.value.is_string()
        },
        "text_lines" => {
            path.ends_with(".txt")
                && setting.value.is_string()
                && setting.key.strip_prefix("line:").is_some_and(|line| {
                    !line.is_empty()
                        && line.len() <= 9
                        && !line.starts_with('0')
                        && line.bytes().all(|byte| byte.is_ascii_digit())
                })
        },
        _ => false,
    }
}

fn verify_file(path: &Path, expected_digest: &str, expected_size: u64) -> Result<bool, io::Error> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() != expected_size {
        return Ok(false);
    }
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()) == expected_digest)
}

fn verified_cache_path(object: &ManifestObjectRef, cache_root: &Path) -> Result<Option<PathBuf>, DistributionError> {
    if !valid_digest(&object.sha256) || object.size > MAX_OBJECT_BYTES {
        return Err(DistributionError::InvalidResponse("invalid or oversized object reference".into()));
    }
    let directory = cache_root.join("sha256").join(&object.sha256[..2]);
    fs::create_dir_all(&directory)?;
    let destination = directory.join(&object.sha256);
    match fs::symlink_metadata(&destination) {
        Ok(metadata) if metadata.file_type().is_file() => {
            if verify_file(&destination, &object.sha256, object.size)? {
                Ok(Some(destination))
            } else {
                fs::remove_file(&destination)?;
                Ok(None)
            }
        },
        Ok(_) => Err(DistributionError::InvalidObject(object.sha256.clone())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn progress_units(bytes: u64) -> usize {
    usize::try_from(bytes).unwrap_or(usize::MAX)
}

fn check_manifest_path(path: &str) -> Result<(), DistributionError> {
    let components = Path::new(path).components().collect::<Vec<_>>();
    if path.is_empty()
        || path.len() > 512
        || Path::new(path).is_absolute()
        || components.is_empty()
        || components.iter().any(|part| !matches!(part, std::path::Component::Normal(_)))
        || path.contains('\\')
        || path.contains(':')
        || path.bytes().any(|byte| byte.is_ascii_control())
        || path.split('/').any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(DistributionError::InvalidResponse("manifest contains an unsafe destination path".into()));
    }
    let first = components[0].as_os_str().to_string_lossy().to_ascii_lowercase();
    let file_name = components.last().unwrap().as_os_str().to_string_lossy().to_ascii_lowercase();
    if path.split('/').any(|part| {
        let lower = part.to_ascii_lowercase();
        let device_name = lower
            .split('.')
            .next()
            .unwrap_or(&lower)
            .trim_end_matches(|character| character == ' ' || character == '.');
        part.ends_with(' ')
            || part.ends_with('.')
            || part.bytes().any(|byte| matches!(byte, b'<' | b'>' | b'"' | b'|' | b'?' | b'*'))
            || matches!(device_name, "con" | "prn" | "aux" | "nul")
            || (device_name.len() == 4
                && (device_name.starts_with("com") || device_name.starts_with("lpt"))
                && device_name.as_bytes()[3].is_ascii_digit())
    }) {
        return Err(DistributionError::InvalidResponse(
            "manifest contains a path that is not portable to Windows".into(),
        ));
    }
    if matches!(
        first.as_str(),
        "saves" | "screenshots" | "logs" | "crash-reports" | "server-resource-packs"
    ) || matches!(
        file_name.as_str(),
        "usercache.json" | "usernamecache.json" | "realms_persistence.json"
    ) || first == ".pandora-layout-v1"
    {
        return Err(DistributionError::InvalidResponse(
            "manifest attempts to manage player data or launcher state".into(),
        ));
    }
    Ok(())
}

fn is_initial_player_setting(path: &str) -> bool {
    path.eq_ignore_ascii_case("options.txt") || path.eq_ignore_ascii_case("servers.dat")
}

fn verify_expected_base(
    files: &BTreeMap<String, EffectiveProfileEntry>,
    logical_identity: &str,
    expected: Option<&str>,
) -> Result<(), DistributionError> {
    if let Some(expected) = expected {
        let Some(existing) = files.values().find(|entry| entry.metadata.logical_identity == logical_identity) else {
            return Err(DistributionError::InvalidResponse("manifest base expectation does not exist".into()));
        };
        if existing.metadata.source_sha256 != expected {
            return Err(DistributionError::InvalidResponse(
                "manifest base expectation digest does not match its pinned parent".into(),
            ));
        }
    }
    Ok(())
}

fn insert_manifest_entry(
    files: &mut BTreeMap<String, EffectiveProfileEntry>,
    path: &str,
    logical_identity: String,
    object: &ManifestObjectRef,
    pin: &GlobalRevisionPin,
    policy: ProfileFilePolicy,
) -> Result<(), DistributionError> {
    if !valid_digest(&object.sha256) || object.size > MAX_OBJECT_BYTES {
        return Err(DistributionError::InvalidResponse("manifest contains an invalid object reference".into()));
    }
    if let Some(existing) = files.get(path)
        && existing.metadata.logical_identity != logical_identity
    {
        return Err(DistributionError::InvalidResponse(
            "manifest aliases multiple logical entries to one destination".into(),
        ));
    }
    if files
        .keys()
        .any(|existing_path| existing_path != path && existing_path.eq_ignore_ascii_case(path))
    {
        return Err(DistributionError::InvalidResponse(
            "manifest contains destinations that collide on Windows".into(),
        ));
    }
    files.insert(
        path.to_owned(),
        EffectiveProfileEntry {
            path: path.to_owned(),
            source: PathBuf::new(),
            metadata: ProfileEntryMetadata {
                logical_identity,
                source_sha256: object.sha256.clone(),
                source_size_bytes: object.size,
                source_modified_unix_nanos: 0,
                origin: ProfileEntryOrigin::GlobalRevision { pin: pin.clone() },
                ownership: ProfileEntryOwnership::Inherited,
                policy,
            },
        },
    );
    Ok(())
}

/// Keeps a predictable catalog order for callers that want to sort before rendering.
pub fn sort_profiles(profiles: &mut [GlobalProfileSummary]) {
    profiles.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
}

/// Maps a server inventory to its stable channel pin when available, otherwise its latest
/// immutable revision. The returned pin is still verified by `fetch_verified_revision`.
pub fn selected_revision(profile: &GlobalProfileSummary) -> &RevisionRef {
    profile
        .channels
        .iter()
        .find(|channel| channel.name == "stable")
        .map(|channel| &channel.revision)
        .unwrap_or(&profile.latest_revision)
}

impl crate::BackendState {
    pub async fn create_global_profile_instance(
        self: &Arc<Self>,
        name: &str,
        profile_id: &str,
        revision: &RevisionRef,
        modal_action: &ModalAction,
        progress: &ProgressTracker,
        save_group_target: Option<bridge::message::GlobalProfileSaveGroupTarget>,
    ) -> Result<(), String> {
        progress.set_title("Preparing modpack assets".into());
        progress.set_count(0);
        progress.set_total(1);
        let config = self.config.lock().get().distribution.clone();
        let client = DistributionClient::new(&config).map_err(|error| error.to_string())?;
        let cache_root = self.directories.root_launcher_dir.join("distribution-objects");
        let resolved = client
            .resolve_profile_with_progress(profile_id, revision, &cache_root, modal_action, progress)
            .await
            .map_err(|error| error.to_string())?;
        progress.set_title("Applying modpack files".into());
        // The profile's presentation is unversioned and can have a newer icon than the selected
        // immutable revision. Use the current verified catalog icon, matching the carousel.
        let catalog = client.list_profiles().await.map_err(|error| error.to_string())?;
        let published_profile = catalog.iter().find(|profile| profile.profile_id == profile_id)
            .ok_or_else(|| "The selected global profile is no longer published".to_owned())?;
        let profile_icon = match &published_profile.presentation {
            Some(presentation) => match &presentation.icon {
                Some(icon) => Some(client.fetch_verified_icon(icon, &cache_root).await.map_err(|error| error.to_string())?),
                None => None,
            },
            None => resolved.icon_path.clone(),
        };
        validate_game_identity(&resolved.minecraft_version, &resolved.neoforge_version)?;

        let (loader, loader_version) = if resolved.neoforge_version.is_empty() {
            (schema::loader::Loader::Vanilla, None)
        } else {
            (schema::loader::Loader::NeoForge, Some(resolved.neoforge_version.as_str()))
        };
        let Some((root, libraries_ready)) = self
            .create_global_instance_sanitized(name, &resolved.minecraft_version, loader, loader_version)
            .await
        else {
            return Err("Wachiland Launcher could not create the local instance".to_owned());
        };

        // A newly provisioned instance may not have been launched yet, so Pandora has not
        // necessarily created its .minecraft directory. The profile layout reconciler applies
        // files relative to that directory and expects it to exist before it starts its transaction.
        std::fs::create_dir_all(root.join(".minecraft")).map_err(|error| {
            format!("Could not prepare the .minecraft directory for the global profile instance: {error}")
        })?;

        // Keep the normal file watcher path, but also load synchronously so the profile transaction
        // can acquire the new instance ID before the game-files publication guard is released.
        if let Some(icon) = &profile_icon {
            crate::fs::write_safe(&root.join("icon.png"), &std::fs::read(icon).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        }
        self.load_instance_from_path(&root, false, false);
        if !libraries_ready {
            return Err("The instance was created but game-file provisioning failed; it remains blocked from Start. Use Repair game files, then create the global profile again.".into());
        }
        let id = self
            .instance_state
            .read()
            .instances
            .iter()
            .find(|instance| instance.root_path.as_ref() == root.as_path())
            .map(|instance| instance.id)
            .ok_or_else(|| "Wachiland Launcher created the instance folder but could not load it".to_owned())?;

        let lineage = crate::profile_branch::ProfileLineage::from_global(resolved.pin.clone())
            .map_err(|error| error.to_string())?;
        self.configure_persistent_profile_lineage(id, lineage.clone())
            .map_err(|error| error.to_string())?;
        let changes = resolved
            .entries
            .iter()
            .cloned()
            .map(crate::profile_branch::ProfileDeltaChange::Upsert)
            .collect();
        let outcome = self
            .apply_persistent_profile_delta(
                id,
                &crate::profile_branch::ProfileRevisionDelta {
                    lineage,
                    target_revision: Some(resolved.pin),
                    config_settings: resolved.config_settings,
                    changes,
                },
            )
            .map_err(|error| error.to_string())?;
        if !matches!(outcome, crate::profile_layout_flow::ReconcileOutcome::Ready { .. }) {
            return Err(
                "Global profile files were not fully reconciled; the instance remains blocked from Start".into()
            );
        }

        let generation = crate::library_install_state::incomplete_generation(&root, "content-install-in-progress")
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "Global profile game-files state changed before publication".to_owned())?;
        let published = crate::library_install_state::publish_if_incomplete_generation(
            &root,
            "content-install-in-progress",
            generation,
            "global-profile-install-complete",
        )
        .map_err(|error| error.to_string())?;
        if !published {
            return Err("Global profile installation lost its game-files publication guard".into());
        }
        if let Some(target) = save_group_target {
            progress.set_title("Connecting shared worlds".into());
            if let Err(error) = self.attach_global_profile_save_group(id, profile_id, &target).await {
                self.send.send_warning(format!("Instance created, but automatic save grouping failed: {error}. Use Manage save group to retry."));
            }
        }
        Ok(())
    }

    pub async fn update_global_profile_instance(&self, id: bridge::instance::InstanceID) -> Result<bool, String> {
        let snapshot = self.persistent_profile_branch_status(id, None).map_err(|error| error.to_string())?;
        let Some(crate::profile_branch::ProfileParentRef::GlobalRevision { pin: parent_pin }) =
            snapshot.branch.lineage.parent.as_ref()
        else {
            return Err("This instance is not a direct child of a global profile yet".into());
        };
        let Some(applied_pin) = snapshot.branch.applied_revision.as_ref() else {
            return Err("This global profile instance has no applied revision; create or repair it first".into());
        };
        if applied_pin.profile_id != parent_pin.profile_id {
            return Err("The applied global revision does not match this instance's parent profile".into());
        }

        let config = self.config.lock().get().distribution.clone();
        let client = DistributionClient::new(&config).map_err(|error| error.to_string())?;
        let profiles = client.list_profiles().await.map_err(|error| error.to_string())?;
        let profile = profiles
            .iter()
            .find(|profile| profile.profile_id == parent_pin.profile_id)
            .ok_or_else(|| "The pinned global profile is no longer published".to_owned())?;
        let target_revision = selected_revision(profile).clone();
        let target_pin = GlobalRevisionPin::new(
            profile.profile_id.clone(),
            target_revision.revision_id.clone(),
            target_revision.manifest_sha256.clone(),
        )
        .map_err(|error| error.to_string())?;
        if snapshot.branch.applied_revision.as_ref() == Some(&target_pin) {
            return Ok(false);
        }

        let mut reusable_entries = BTreeMap::new();
        let mut skip_paths = BTreeSet::new();
        for (path, metadata) in &snapshot.branch.entries {
            let global_config_seed = metadata.logical_identity.starts_with("config:")
                && matches!(&metadata.origin, ProfileEntryOrigin::GlobalRevision { .. });
            if metadata.ownership == ProfileEntryOwnership::Inherited || global_config_seed {
                reusable_entries.insert(path.clone(), metadata.clone());
            } else {
                skip_paths.insert(path.clone());
            }
        }
        skip_paths.extend(snapshot.branch.tombstones.keys().cloned());

        let cache_root = self.directories.root_launcher_dir.join("distribution-objects");
        let resolved = client
            .resolve_profile_with_reuse(
                &profile.profile_id,
                &target_revision,
                &cache_root,
                &reusable_entries,
                &skip_paths,
            )
            .await
            .map_err(|error| error.to_string())?;

        let target_rule_signatures = crate::config_settings::config_setting_signatures(&resolved.config_settings)
            .map_err(|error| error.to_string())?;
        let mut changed_rule_paths = BTreeSet::new();
        for (identity, signature) in &target_rule_signatures {
            if snapshot.branch.config_setting_signatures.get(identity) != Some(signature) {
                if let Some((path, _)) = identity.split_once('\0') {
                    changed_rule_paths.insert(path.to_owned());
                }
            }
        }
        let target_rule_paths = resolved
            .config_settings
            .iter()
            .map(|setting| setting.path.as_str())
            .collect::<BTreeSet<_>>();

        let target_entries = resolved
            .entries
            .into_iter()
            .map(|entry| (entry.path.clone(), entry))
            .collect::<BTreeMap<_, _>>();
        let mut changes = Vec::new();
        for (path, entry) in &target_entries {
            if snapshot.branch.tombstones.contains_key(path) {
                continue;
            }
            let current = snapshot.branch.entries.get(path);
            if current.is_some_and(|metadata| metadata.ownership != ProfileEntryOwnership::Inherited) {
                if !target_rule_paths.contains(path.as_str())
                    || !current.is_some_and(|metadata| {
                        metadata.logical_identity.starts_with("config:")
                            && matches!(&metadata.origin, ProfileEntryOrigin::GlobalRevision { .. })
                    })
                {
                    continue;
                }
            }
            let file_unchanged = current.is_some_and(|metadata| {
                metadata.logical_identity == entry.metadata.logical_identity
                    && metadata.source_sha256 == entry.metadata.source_sha256
                    && metadata.policy == entry.metadata.policy
            });
            if file_unchanged && !changed_rule_paths.contains(path) {
                continue;
            }
            if entry.source.as_os_str().is_empty() {
                return Err(format!("The changed global file {path} was not downloaded"));
            }
            changes.push(crate::profile_branch::ProfileDeltaChange::Upsert(entry.clone()));
        }
        for (path, metadata) in &snapshot.branch.entries {
            if target_entries.contains_key(path)
                || metadata.ownership != ProfileEntryOwnership::Inherited
                || !matches!(&metadata.origin, ProfileEntryOrigin::GlobalRevision { .. })
            {
                continue;
            }
            changes.push(crate::profile_branch::ProfileDeltaChange::Remove {
                path: path.clone(),
                origin: metadata.origin.clone(),
                ownership: metadata.ownership,
                policy: metadata.policy,
            });
        }

        let lineage = crate::profile_branch::ProfileLineage::from_global(target_pin.clone())
            .map_err(|error| error.to_string())?;
        let outcome = self
            .apply_persistent_profile_delta(
                id,
                &crate::profile_branch::ProfileRevisionDelta {
                    lineage,
                    target_revision: Some(target_pin),
                    config_settings: resolved.config_settings,
                    changes,
                },
            )
            .map_err(|error| error.to_string())?;
        match outcome {
            crate::profile_layout_flow::ReconcileOutcome::Ready { .. } => Ok(true),
            crate::profile_layout_flow::ReconcileOutcome::NeedsReconcile { conflicts, .. } => {
                Err(format!("Global update found files that need attention: {}", conflicts.join(", ")))
            },
        }
    }
}

fn validate_game_identity(minecraft: &str, neoforge: &str) -> Result<(), String> {
    let valid_version = |value: &str| {
        !value.is_empty()
            && value.len() <= 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'+'))
    };
    if !valid_version(minecraft) || (!neoforge.is_empty() && !valid_version(neoforge)) {
        return Err("The signed profile has an invalid Minecraft or NeoForge version".into());
    }
    Ok(())
}

#[cfg(test)]
mod trusted_release_key_tests {
    use super::*;

    fn encoded_key(byte: u8) -> String {
        URL_SAFE_NO_PAD.encode([byte; 32])
    }

    #[test]
    fn server_identity_discovery_preserves_pins() {
        let pinned = BTreeMap::from([("legacy".into(), [1; 32])]);
        let catalog = || ServerSigningKeys {
            schema_version: 1,
            keys: BTreeMap::from([("legacy".into(), encoded_key(1)), ("server-new".into(), encoded_key(2))]),
        };
        assert_eq!(catalog().validated(&pinned).unwrap()["server-new"], [2; 32]);
        let mut conflict = catalog();
        conflict.keys.insert("legacy".into(), encoded_key(3));
        assert!(conflict.validated(&pinned).is_err());
        let mut malformed = catalog();
        malformed.keys.insert("server-new".into(), "invalid".into());
        assert!(malformed.validated(&pinned).is_err());
        let mut wrong_schema = catalog();
        wrong_schema.schema_version = 2;
        assert!(wrong_schema.validated(&pinned).is_err());
    }

    #[test]
    fn https_client_can_bootstrap_without_manual_signing_keys() {
        let config = DistributionConfig {
            base_url: "https://example.com:8444".into(),
            release_key_id: String::new(),
            release_public_key_base64url: String::new(),
            additional_release_keys_json: String::new(),
            ..DistributionConfig::default()
        };
        let client = DistributionClient::new(&config).unwrap();
        assert!(client.trusted_keys.is_empty());
    }

    #[test]
    fn legacy_single_key_remains_trusted() {
        let config = DistributionConfig {
            release_key_id: "release-2026".into(),
            release_public_key_base64url: encoded_key(1),
            ..DistributionConfig::default()
        };
        let keys = trusted_release_keys(&config).unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys["release-2026"], [1; 32]);
    }

    #[test]
    fn additional_keys_allow_rotation_without_dropping_history_trust() {
        let config = DistributionConfig {
            release_key_id: "release-2026".into(),
            release_public_key_base64url: encoded_key(1),
            additional_release_keys_json: format!(
                r#"[{{"key_id":"release-2027","public_key_base64url":"{}"}}]"#,
                encoded_key(2)
            ),
            ..DistributionConfig::default()
        };
        let keys = trusted_release_keys(&config).unwrap();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys["release-2026"], [1; 32]);
        assert_eq!(keys["release-2027"], [2; 32]);
    }

    #[test]
    fn duplicate_ids_or_malformed_additional_keys_fail_closed() {
        let duplicate = DistributionConfig {
            release_key_id: "same-key".into(),
            release_public_key_base64url: encoded_key(1),
            additional_release_keys_json: format!(
                r#"[{{"key_id":"same-key","public_key_base64url":"{}"}}]"#,
                encoded_key(2)
            ),
            ..DistributionConfig::default()
        };
        assert!(matches!(trusted_release_keys(&duplicate), Err(DistributionError::InvalidSigningKey)));

        let malformed = DistributionConfig {
            additional_release_keys_json: "not-json".into(),
            ..DistributionConfig::default()
        };
        assert!(matches!(trusted_release_keys(&malformed), Err(DistributionError::InvalidSigningKey)));
    }

    #[test]
    fn player_settings_are_allowed_as_initial_defaults_only() {
        for path in ["options.txt", "OPTIONS.TXT", "servers.dat", "Servers.dat"] {
            check_manifest_path(path).unwrap();
            assert!(is_initial_player_setting(path));
        }
        assert!(!is_initial_player_setting("config/options.txt"));
        assert!(!is_initial_player_setting("servers/servers.dat"));

        for path in ["saves/world/level.dat", "options.txt/../launcher.json", ".pandora-layout-v1/state.json"] {
            assert!(check_manifest_path(path).is_err(), "protected path unexpectedly allowed: {path}");
        }
    }

    #[test]
    fn old_and_replacement_signers_both_verify_while_trusted() {
        use ring::signature::KeyPair;

        let old = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[1; 32]).unwrap();
        let replacement = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[2; 32]).unwrap();
        let keys = BTreeMap::from([
            ("old-key".into(), old.public_key().as_ref().try_into().unwrap()),
            ("replacement-key".into(), replacement.public_key().as_ref().try_into().unwrap()),
        ]);
        let message = b"same profile history, next revision";
        let old_signature = URL_SAFE_NO_PAD.encode(old.sign(message).as_ref());
        let replacement_signature = URL_SAFE_NO_PAD.encode(replacement.sign(message).as_ref());

        verify_release_signature(&keys, "old-key", message, &old_signature).unwrap();
        verify_release_signature(&keys, "replacement-key", message, &replacement_signature).unwrap();
        assert!(matches!(
            verify_release_signature(&keys, "untrusted-key", message, &replacement_signature),
            Err(DistributionError::InvalidSignature)
        ));
    }
}

pub fn profile_names_by_id(profiles: &[GlobalProfileSummary]) -> BTreeMap<String, String> {
    profiles
        .iter()
        .map(|profile| (profile.profile_id.clone(), profile.name.clone()))
        .collect()
}

impl crate::BackendState {
    pub async fn global_profile_save_group_targets(
        &self,
        profile_id: &str,
    ) -> Result<Vec<bridge::message::GlobalProfileSaveGroupTarget>, String> {
        let catalog = self.load_global_catalog().await?;
        let parents = catalog.iter().map(|p| (p.profile_id.clone(), p.parent_profile_id.clone()))
            .collect::<bridge::profile_family::Parents>();
        if !parents.contains_key(profile_id) { return Err("Global profile is no longer published".into()); }
        let instances = self.instance_state.read().instances.iter()
            .map(|instance| (instance.id, instance.name.to_string(), instance.root_path.to_path_buf())).collect::<Vec<_>>();
        let mut targets = BTreeMap::new();
        for (id, name, root) in instances {
            let branch = match crate::profile_layout_flow::read_committed_branch(&root) {
                Ok(Some(branch)) => branch,
                Ok(None) => continue,
                Err(error) => { log::warn!("Ignoring unavailable lineage at {root:?} during family lookup: {error}"); continue; },
            };
            let Some(crate::profile_branch::ProfileParentRef::GlobalRevision { pin }) = branch.lineage.parent.as_ref() else { continue; };
            if !bridge::profile_family::related(profile_id, &pin.profile_id, &parents) { continue; }
            let groups = self.list_save_groups(id)?;
            let selected = groups.iter().find(|group| group.selected);
            let group_id = selected.map(|group| group.id);
            targets.entry(group_id).or_insert(bridge::message::GlobalProfileSaveGroupTarget {
                anchor_id: id,
                group_id,
                name: selected.map(|group| group.name.clone()).unwrap_or_else(|| format!("Create shared group with {name}")),
            });
        }
        // Prefer an existing group; an ungrouped relative need not introduce an extra choice.
        if targets.keys().any(Option::is_some) { targets.remove(&None); }
        Ok(targets.into_values().collect())
    }

    async fn attach_global_profile_save_group(
        self: &Arc<Self>,
        id: bridge::instance::InstanceID,
        profile_id: &str,
        chosen: &bridge::message::GlobalProfileSaveGroupTarget,
    ) -> Result<(), String> {
        let targets = self.global_profile_save_group_targets(profile_id).await?;
        let target = targets.iter().find(|target| match chosen.group_id {
            Some(group_id) => target.group_id == Some(group_id),
            None => target.anchor_id == chosen.anchor_id && target.group_id.is_none(),
        })
            .ok_or("The selected related instance or group changed during installation")?;
        let anchor = target.anchor_id;
        let mut created = false;
        let group_id = match target.group_id {
            Some(group_id) => group_id,
            None => {
                let name = self.instance_state.read().instances.get(anchor).ok_or("Related instance no longer exists")?.name.to_string();
                self.create_save_group(anchor, format!("Worlds of {name}").chars().take(80).collect())?;
                created = true;
                self.list_save_groups(anchor)?.into_iter().find(|group| group.selected)
                    .ok_or("New shared group could not be found")?.id
            },
        };
        let result = self.join_save_group(id, group_id);
        if result.is_err() && created {
            if let Err(error) = self.leave_save_group(anchor) {
                self.send.send_warning(format!("Shared worlds remain in their group after grouping failed: {error}"));
            }
        }
        crate::backend_handler::refresh_instance_saves(self, anchor, true);
        crate::backend_handler::refresh_instance_saves(self, id, true);
        result
    }

    pub async fn load_global_catalog(&self) -> Result<Vec<bridge::message::GlobalProfileSummary>, String> {
        let config = self.config.lock().get().distribution.clone();
        let client = DistributionClient::new(&config).map_err(|e| e.to_string())?;
        let profiles = client.list_profiles().await.map_err(|e| e.to_string())?;
        let mut summaries = Vec::new();
        for profile in profiles {
            let stable = profile.channels.iter().find(|channel| channel.name == "stable");
            let revision = selected_revision(&profile);
            let verified = client.fetch_verified_revision(&profile.profile_id, revision).await.map_err(|e| e.to_string())?;
            let metadata = &verified.manifest.profile;
            let presentation = profile.presentation.as_ref();
            let display_icon = presentation.map(|p| &p.icon).unwrap_or(&metadata.icon);
            let icon_path = if let Some(icon) = display_icon {
                Some(client.fetch_verified_icon(icon, &self.directories.root_launcher_dir.join("distribution-objects")).await.map_err(|e| e.to_string())?)
            } else { None };
            summaries.push(bridge::message::GlobalProfileSummary {
                parent_profile_id: verified.manifest.base.as_ref().map(|base| base.profile_id.clone()),
                profile_id: profile.profile_id.clone(), name: presentation.map(|p| p.name.clone()).unwrap_or_else(|| metadata.name.clone()),
                description: presentation.map(|p| p.description.clone()).unwrap_or_else(|| if metadata.description.is_empty() { verified.manifest.revision.release_notes.clone().unwrap_or_default() } else { metadata.description.clone() }),
                minecraft: verified.manifest.game.minecraft.clone(), neoforge: verified.manifest.game.neoforge.clone(), icon_path,
                latest_revision_id: profile.latest_revision.revision_id, latest_sequence: profile.latest_revision.sequence,
                latest_manifest_sha256: profile.latest_revision.manifest_sha256,
                stable_revision_id: stable.map(|c| c.revision.revision_id.clone()), stable_sequence: stable.map(|c| c.revision.sequence),
                stable_manifest_sha256: stable.map(|c| c.revision.manifest_sha256.clone()),
            });
        }
        Ok(summaries)
    }
}
