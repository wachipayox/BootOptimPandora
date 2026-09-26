//! Secure read-side client for the private BootOptim Distribution service.
//!
//! The service is an untrusted transport from the launcher's point of view: manifests and
//! content objects are independently verified here before any caller can use them.

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
    #[error("Distribution release signing key is missing or invalid")]
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
    pub profile_id: String,
    pub name: String,
    pub latest_revision: RevisionRef,
    #[serde(default)]
    pub channels: Vec<ChannelRef>,
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
    pub id: String,
    pub name: String,
    pub official: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
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

#[derive(Clone, Debug, Deserialize)]
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
    pub pin: GlobalRevisionPin,
    pub name: String,
    pub minecraft_version: String,
    pub neoforge_version: String,
    pub entries: Vec<EffectiveProfileEntry>,
}

#[derive(Clone)]
pub struct DistributionClient {
    client: Client,
    base_url: Url,
    key_id: String,
    verifying_key: [u8; 32],
}

impl DistributionClient {
    pub fn new(config: &DistributionConfig) -> Result<Self, DistributionError> {
        let base_url = Url::parse(config.base_url.trim()).map_err(|_| DistributionError::InvalidBaseUrl)?;
        if base_url.scheme() != "https"
            || base_url.host_str().is_none()
            || base_url.username() != ""
            || base_url.password().is_some()
            || base_url.query().is_some()
            || base_url.fragment().is_some()
        {
            return Err(DistributionError::InvalidBaseUrl);
        }

        let key_bytes = URL_SAFE_NO_PAD
            .decode(config.release_public_key_base64url.trim())
            .map_err(|_| DistributionError::InvalidSigningKey)?;
        let key_bytes: [u8; 32] = key_bytes.try_into().map_err(|_| DistributionError::InvalidSigningKey)?;
        let verifying_key = key_bytes;
        if config.release_key_id.trim().is_empty() {
            return Err(DistributionError::InvalidSigningKey);
        }

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
        let client = builder.build()?;

        Ok(Self {
            client,
            base_url,
            key_id: config.release_key_id.trim().to_owned(),
            verifying_key,
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
            || envelope.signature.key_id != self.key_id
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
        let signature_bytes = URL_SAFE_NO_PAD
            .decode(envelope.signature.value)
            .map_err(|_| DistributionError::InvalidSignature)?;
        ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, &self.verifying_key)
            .verify(&canonical, &signature_bytes)
            .map_err(|_| DistributionError::InvalidSignature)?;
        if manifest.schema_version != 1
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
        let minecraft_version = target.manifest.game.minecraft.clone();
        let neoforge_version = target.manifest.game.neoforge.clone();

        let mut files = BTreeMap::<String, EffectiveProfileEntry>::new();
        let mut mod_paths = BTreeMap::<String, String>::new();
        let mut object_paths = BTreeMap::<String, String>::new();
        let mut object_refs = BTreeMap::<(String, String, String), &ManifestObjectRef>::new();
        for revision in chain.iter().rev() {
            let pin = GlobalRevisionPin::new(
                revision.manifest.profile.id.clone(),
                revision.manifest.revision.id.clone(),
                revision.manifest_sha256.clone(),
            )
            .map_err(|error| DistributionError::InvalidResponse(error.to_string()))?;
            let manifest = &revision.manifest;
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
                    ProfileFilePolicy::Enforced,
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
                let policy = match item.policy.as_str() {
                    "enforced" => ProfileFilePolicy::Enforced,
                    "default_once" => ProfileFilePolicy::DefaultOnce,
                    _ => return Err(DistributionError::InvalidResponse("unsupported config policy".into())),
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
                insert_manifest_entry(
                    &mut files,
                    &item.path,
                    format!("object:{}", item.id),
                    &item.object,
                    &pin,
                    ProfileFilePolicy::Enforced,
                )?;
            }
        }

        let mut entries = Vec::with_capacity(files.len());
        for (_, mut entry) in files {
            if skip_paths.contains(&entry.path)
                || reusable_entries.get(&entry.path).is_some_and(|existing| {
                    existing.logical_identity == entry.metadata.logical_identity
                        && existing.source_sha256 == entry.metadata.source_sha256
                        && existing.policy == entry.metadata.policy
                })
            {
                entries.push(entry);
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
            entry.source = self.fetch_verified_object(object, cache_root).await?;
            entries.push(entry);
        }
        Ok(ResolvedGlobalProfile {
            pin: target_pin,
            name: target_name,
            minecraft_version,
            neoforge_version,
            entries,
        })
    }

    pub async fn fetch_verified_object(
        &self,
        object: &ManifestObjectRef,
        cache_root: &Path,
    ) -> Result<PathBuf, DistributionError> {
        if !valid_digest(&object.sha256) || object.size > MAX_OBJECT_BYTES {
            return Err(DistributionError::InvalidResponse("invalid or oversized object reference".into()));
        }
        let directory = cache_root.join("sha256").join(&object.sha256[..2]);
        fs::create_dir_all(&directory)?;
        let destination = directory.join(&object.sha256);
        match fs::symlink_metadata(&destination) {
            Ok(metadata) if metadata.file_type().is_file() => {
                if verify_file(&destination, &object.sha256, object.size)? {
                    return Ok(destination);
                }
                fs::remove_file(&destination)?;
            },
            Ok(_) => return Err(DistributionError::InvalidObject(object.sha256.clone())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {},
            Err(error) => return Err(error.into()),
        }

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
        let url = self.join(path)?;
        let response = self.client.get(url).send().await?;
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

    fn object_url(&self, digest: &str) -> Result<Url, DistributionError> {
        if !valid_digest(digest) {
            return Err(DistributionError::InvalidResponse("invalid object digest".into()));
        }
        self.join(&format!("/v1/objects/sha256/{digest}"))
    }

    fn join(&self, path: &str) -> Result<Url, DistributionError> {
        let base = format!("{}/", self.base_url.as_str().trim_end_matches('/'));
        let base = Url::parse(&base).map_err(|_| DistributionError::InvalidBaseUrl)?;
        let relative = path.trim_start_matches('/');
        base.join(relative).map_err(|_| DistributionError::InvalidBaseUrl)
    }
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
        "servers.dat" | "options.txt" | "usercache.json" | "usernamecache.json" | "realms_persistence.json"
    ) || first == ".pandora-layout-v1"
    {
        return Err(DistributionError::InvalidResponse(
            "manifest attempts to manage player data or launcher state".into(),
        ));
    }
    Ok(())
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
    ) -> Result<(), String> {
        let config = self.config.lock().get().distribution.clone();
        let client = DistributionClient::new(&config).map_err(|error| error.to_string())?;
        let cache_root = self.directories.root_launcher_dir.join("distribution-objects");
        let resolved = client
            .resolve_profile(profile_id, revision, &cache_root)
            .await
            .map_err(|error| error.to_string())?;
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
            return Err("Pandora could not create the local instance".to_owned());
        };

        // Keep the normal file watcher path, but also load synchronously so the profile transaction
        // can acquire the new instance ID before the game-files publication guard is released.
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
            .ok_or_else(|| "Pandora created the instance folder but could not load it".to_owned())?;

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
            if metadata.ownership == ProfileEntryOwnership::Inherited {
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
                continue;
            }
            if current.is_some_and(|metadata| {
                metadata.logical_identity == entry.metadata.logical_identity
                    && metadata.source_sha256 == entry.metadata.source_sha256
                    && metadata.policy == entry.metadata.policy
            }) {
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

pub fn profile_names_by_id(profiles: &[GlobalProfileSummary]) -> BTreeMap<String, String> {
    profiles
        .iter()
        .map(|profile| (profile.profile_id.clone(), profile.name.clone()))
        .collect()
}
