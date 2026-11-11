//! Distribution's launcher releases (`GET /v1/launcher/pandora/releases`, BootOptimDistribution
//! `docs/PROFILE_PROTOCOL.md` section 5) and their verification.
//!
//! Shared by the launcher's in-place updater and the Windows setup, so a release that one accepts the
//! other accepts too. Every check here is pure: callers fetch the index, the signing key and the bytes.
use std::collections::BTreeMap;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Release signing key built into the launcher and the setup (`release_key_id` default).
pub const PINNED_RELEASE_KEY_ID: &str = "wachiland-release-2026-09";
pub const PINNED_RELEASE_PUBLIC_KEY: &str = "q8uG8c8IBO-N7tnRWeKKqpl7pSj6G7FvKuRiCycpYwk";
/// The CAS limit of Distribution.
pub const MAX_OBJECT_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// `type` of a signed launcher statement; it cannot be confused with a revision manifest.
pub const STATEMENT_TYPE: &str = "wbombo.launcher-artifact.v1";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LauncherRelease {
    pub version: String,
    pub channel: String,
    pub published_at: String,
    pub notes: String,
    pub artifact: LauncherArtifact,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LauncherArtifact {
    pub platform: String,
    pub file_name: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub statement: String,
    pub signature: LauncherSignature,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LauncherSignature {
    pub key_id: String,
    pub algorithm: String,
    pub value: String,
}

#[derive(Deserialize)]
pub struct ReleaseIndex {
    pub schema_version: u32,
    pub app: String,
    pub latest: Option<LauncherRelease>,
    pub releases: Vec<LauncherRelease>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactStatement {
    #[serde(rename = "type")]
    kind: String,
    app: String,
    version: String,
    platform: String,
    sha256: String,
    size: u64,
    issued_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The index or a version in it is malformed.
    InvalidIndex(&'static str),
    /// The statement, its signature or what it binds does not match.
    InvalidSignature,
    /// A signing key is malformed, duplicated or conflicts with a pinned one.
    InvalidSigningKey,
    /// The downloaded bytes do not have the signed size and SHA-256.
    InvalidObject,
}

pub fn version(value: &str) -> Result<Version, Error> {
    Version::parse(value).map_err(|_| Error::InvalidIndex("invalid launcher SemVer"))
}

pub fn newer(candidate: &str, installed: &str) -> Result<bool, Error> {
    // SemVer build metadata does not contribute to precedence.
    Ok(version(candidate)?.cmp_precedence(&version(installed)?).is_gt())
}

pub fn valid_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

/// Checks the index for `platform`: identity, order (newest first, strictly decreasing), channel and,
/// with `installed`, that every release is newer than it. `player_only` refuses developer releases.
pub fn validate_index(
    index: ReleaseIndex,
    platform: &str,
    player_only: bool,
    installed: Option<&str>,
) -> Result<Vec<LauncherRelease>, Error> {
    if index.schema_version != 1 || index.app != "pandora" || index.latest.as_ref() != index.releases.first() {
        return Err(Error::InvalidIndex("invalid launcher release index"));
    }
    let mut previous: Option<Version> = None;
    for release in &index.releases {
        let current = version(&release.version)?;
        let newer_than_installed = match installed {
            Some(installed) => newer(&release.version, installed)?,
            None => true,
        };
        if !newer_than_installed
            || previous.as_ref().is_some_and(|p| !p.cmp_precedence(&current).is_gt())
            || !matches!(release.channel.as_str(), "player" | "developer")
            || (player_only && release.channel != "player")
            || release.artifact.platform != platform
            || release.notes.len() > 16_384
            || chrono::DateTime::parse_from_rfc3339(&release.published_at).is_err()
        {
            return Err(Error::InvalidIndex("launcher release identity, channel or order mismatch"));
        }
        previous = Some(current);
    }
    Ok(index.releases)
}

/// Verifies the Ed25519 signature over the exact decoded statement bytes (never a reserialized
/// statement) and that the statement binds this app, version, platform, size, SHA-256 and CAS path.
/// With `installed`, the release must be newer than it (never a downgrade).
pub fn verify_signature(
    release: &LauncherRelease,
    platform: &str,
    installed: Option<&str>,
    key: &[u8; 32],
) -> Result<(), Error> {
    let artifact = &release.artifact;
    let newer_than_installed = match installed {
        Some(installed) => newer(&release.version, installed)?,
        None => {
            version(&release.version)?;
            true
        },
    };
    if !newer_than_installed
        || artifact.platform != platform
        || artifact.signature.algorithm != "Ed25519"
        || artifact.statement.len() > 8192
        || artifact.size == 0
        || artifact.size > MAX_OBJECT_BYTES
        || !valid_digest(&artifact.sha256)
        || artifact.url != format!("/v1/objects/sha256/{}", artifact.sha256)
    {
        return Err(Error::InvalidSignature);
    }
    let raw = URL_SAFE_NO_PAD.decode(&artifact.statement).map_err(|_| Error::InvalidSignature)?;
    let signature = URL_SAFE_NO_PAD.decode(&artifact.signature.value).map_err(|_| Error::InvalidSignature)?;
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, key)
        .verify(&raw, &signature)
        .map_err(|_| Error::InvalidSignature)?;
    let statement: ArtifactStatement = serde_json::from_slice(&raw).map_err(|_| Error::InvalidSignature)?;
    if statement.kind != STATEMENT_TYPE
        || statement.app != "pandora"
        || statement.version != release.version
        || statement.platform != platform
        || statement.sha256 != artifact.sha256
        || statement.size != artifact.size
        || chrono::DateTime::parse_from_rfc3339(&statement.issued_at).is_err()
    {
        return Err(Error::InvalidSignature);
    }
    Ok(())
}

/// The downloaded bytes must have the signed size and SHA-256.
pub fn verify_download(artifact: &LauncherArtifact, bytes: &[u8]) -> Result<(), Error> {
    verify_download_digest(artifact, bytes.len() as u64, &hex::encode(Sha256::digest(bytes)))
}

/// Same check for a download hashed while streaming: `sha256` is the lowercase hex digest.
pub fn verify_download_digest(artifact: &LauncherArtifact, size: u64, sha256: &str) -> Result<(), Error> {
    if size != artifact.size || sha256 != artifact.sha256 {
        return Err(Error::InvalidObject);
    }
    Ok(())
}

/// Adds one base64url Ed25519 public key; ids are unique, trimmed and at most 128 bytes.
pub fn insert_trusted_key(
    keys: &mut BTreeMap<String, [u8; 32]>,
    key_id: &str,
    encoded_public: &str,
) -> Result<(), Error> {
    if key_id.is_empty() || key_id.len() > 128 || key_id.trim() != key_id {
        return Err(Error::InvalidSigningKey);
    }
    let key_bytes = URL_SAFE_NO_PAD.decode(encoded_public.trim()).map_err(|_| Error::InvalidSigningKey)?;
    let key_bytes: [u8; 32] = key_bytes.try_into().map_err(|_| Error::InvalidSigningKey)?;
    if keys.insert(key_id.to_owned(), key_bytes).is_some() {
        return Err(Error::InvalidSigningKey);
    }
    Ok(())
}

/// The built-in release key.
pub fn pinned_keys() -> BTreeMap<String, [u8; 32]> {
    let mut keys = BTreeMap::new();
    insert_trusted_key(&mut keys, PINNED_RELEASE_KEY_ID, PINNED_RELEASE_PUBLIC_KEY)
        .expect("valid built-in release key");
    keys
}

/// `GET /v1/signing-keys`, discovered over HTTPS: never accepted when it gives a pinned id another key.
pub fn validate_key_catalog(
    schema_version: u32,
    catalog: BTreeMap<String, String>,
    pinned: &BTreeMap<String, [u8; 32]>,
) -> Result<BTreeMap<String, [u8; 32]>, Error> {
    if schema_version != 1 || catalog.is_empty() || catalog.len() > 128 {
        return Err(Error::InvalidSigningKey);
    }
    let mut keys = BTreeMap::new();
    for (id, encoded) in catalog {
        insert_trusted_key(&mut keys, &id, &encoded)?;
        if pinned.get(&id).is_some_and(|expected| keys.get(&id) != Some(expected)) {
            return Err(Error::InvalidSigningKey);
        }
    }
    Ok(keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_key_parses() {
        assert_eq!(pinned_keys().len(), 1);
    }

    #[test]
    fn a_catalog_cannot_replace_a_pinned_key() {
        let pinned = pinned_keys();
        let other = URL_SAFE_NO_PAD.encode([7u8; 32]);
        let catalog = BTreeMap::from([(PINNED_RELEASE_KEY_ID.to_owned(), other.clone())]);
        assert_eq!(validate_key_catalog(1, catalog, &pinned), Err(Error::InvalidSigningKey));
        let catalog = BTreeMap::from([
            ("server-x".to_owned(), other),
            (PINNED_RELEASE_KEY_ID.to_owned(), PINNED_RELEASE_PUBLIC_KEY.to_owned()),
        ]);
        assert_eq!(validate_key_catalog(1, catalog, &pinned).unwrap().len(), 2);
    }

    #[test]
    fn without_an_installed_version_any_valid_version_is_accepted() {
        let index = ReleaseIndex {
            schema_version: 1,
            app: "pandora".into(),
            latest: None,
            releases: Vec::new(),
        };
        assert!(validate_index(index, "windows-x86_64", true, None).unwrap().is_empty());
    }
}
