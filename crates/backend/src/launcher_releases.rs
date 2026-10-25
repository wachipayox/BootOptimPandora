//! Launcher artifacts use the same HTTPS origin and signing-key trust as profiles.
use super::*;
use futures::StreamExt;
use schema::{
    backend_config::LauncherChannel,
    launcher_release::{LauncherArtifact, LauncherRelease},
};
use semver::Version;

#[derive(Deserialize)]
struct ReleaseIndex {
    schema_version: u32,
    app: String,
    latest: Option<LauncherRelease>,
    releases: Vec<LauncherRelease>,
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

/// Constructed only after signature, identity and anti-downgrade verification.
/// Download bytes still require their own digest/length check before installation.
pub struct VerifiedLauncherArtifact {
    artifact: LauncherArtifact,
}

fn invalid(reason: &str) -> DistributionError {
    DistributionError::InvalidResponse(reason.into())
}

fn version(value: &str) -> Result<Version, DistributionError> {
    Version::parse(value).map_err(|_| invalid("invalid launcher SemVer"))
}

fn newer(candidate: &str, installed: &str) -> Result<bool, DistributionError> {
    // SemVer build metadata does not contribute to precedence.
    Ok(version(candidate)?.cmp_precedence(&version(installed)?).is_gt())
}

fn without_url(error: DistributionError) -> DistributionError {
    match error {
        DistributionError::Request(error) => DistributionError::Request(error.without_url()),
        other => other,
    }
}

impl DistributionClient {
    pub async fn launcher_releases(
        &self,
        platform: &str,
        channel: LauncherChannel,
        installed: &str,
    ) -> Result<Vec<LauncherRelease>, DistributionError> {
        version(installed)?;
        if platform.is_empty()
            || platform.len() > 32
            || !platform
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' || c == b'_')
        {
            return Err(invalid("invalid launcher platform"));
        }
        let mut url = self.join("/v1/launcher/pandora/releases")?;
        url.query_pairs_mut()
            .append_pair("platform", platform)
            .append_pair(
                "channel",
                if channel == LauncherChannel::Developer {
                    "developer"
                } else {
                    "player"
                },
            )
            .append_pair("after", installed);
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| DistributionError::Request(e.without_url()))?;
        let index: ReleaseIndex = decode_json_response(response).await.map_err(without_url)?;
        validate_index(index, platform, channel, installed)
    }

    pub async fn verify_launcher_artifact(
        &self,
        release: &LauncherRelease,
        platform: &str,
        installed: &str,
    ) -> Result<VerifiedLauncherArtifact, DistributionError> {
        let key = self.launcher_signing_key(&release.artifact.signature.key_id).await.map_err(without_url)?;
        verify_signature(release, platform, installed, &key)
    }

    async fn launcher_signing_key(&self, id: &str) -> Result<[u8; 32], DistributionError> {
        if let Some(key) = self.trusted_keys.get(id) {
            return Ok(*key);
        }
        let discovered = self
            .server_keys
            .get_or_try_init(|| async {
                let response: ServerSigningKeys = self.get_json("/v1/signing-keys").await?;
                response.validated(&self.trusted_keys)
            })
            .await?;
        discovered.get(id).copied().ok_or(DistributionError::InvalidSigningKey)
    }

    pub async fn download_launcher_artifact(
        &self,
        verified: &VerifiedLauncherArtifact,
        mut progress: impl FnMut(usize),
    ) -> Result<Vec<u8>, DistributionError> {
        let artifact = &verified.artifact;
        // Verification requires the URL to identify the signed CAS object. It
        // remains on the configured HTTPS origin with redirects disabled.
        let response = self
            .client
            .get(self.join(&artifact.url)?)
            .send()
            .await
            .map_err(|e| DistributionError::Request(e.without_url()))?;
        if response.status() != reqwest::StatusCode::OK {
            return Err(DistributionError::HttpStatus(response.status()));
        }
        if response.content_length().is_some_and(|length| length != artifact.size) {
            return Err(invalid("launcher download length mismatch"));
        }
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| DistributionError::Request(e.without_url()))?;
            if (bytes.len() as u64).saturating_add(chunk.len() as u64) > artifact.size {
                return Err(DistributionError::ResponseTooLarge);
            }
            bytes.extend_from_slice(&chunk);
            progress(chunk.len());
        }
        verify_download(artifact, &bytes)?;
        Ok(bytes)
    }
}

fn validate_index(
    index: ReleaseIndex,
    platform: &str,
    channel: LauncherChannel,
    installed: &str,
) -> Result<Vec<LauncherRelease>, DistributionError> {
    if index.schema_version != 1 || index.app != "pandora" || index.latest.as_ref() != index.releases.first() {
        return Err(invalid("invalid launcher release index"));
    }
    let mut previous: Option<Version> = None;
    for release in &index.releases {
        let current = version(&release.version)?;
        if !newer(&release.version, installed)?
            || previous.as_ref().is_some_and(|p| !p.cmp_precedence(&current).is_gt())
            || !matches!(release.channel.as_str(), "player" | "developer")
            || (channel == LauncherChannel::Player && release.channel != "player")
            || release.artifact.platform != platform
            || release.notes.len() > 16_384
            || chrono::DateTime::parse_from_rfc3339(&release.published_at).is_err()
        {
            return Err(invalid("launcher release identity, channel or order mismatch"));
        }
        previous = Some(current);
    }
    Ok(index.releases)
}

fn verify_signature(
    release: &LauncherRelease,
    platform: &str,
    installed: &str,
    key: &[u8; 32],
) -> Result<VerifiedLauncherArtifact, DistributionError> {
    let artifact = &release.artifact;
    if !newer(&release.version, installed)?
        || artifact.platform != platform
        || artifact.signature.algorithm != "Ed25519"
        || artifact.statement.len() > 8192
        || artifact.size == 0
        || artifact.size > MAX_OBJECT_BYTES
        || !valid_digest(&artifact.sha256)
        || artifact.url != format!("/v1/objects/sha256/{}", artifact.sha256)
    {
        return Err(DistributionError::InvalidSignature);
    }
    let raw = URL_SAFE_NO_PAD
        .decode(&artifact.statement)
        .map_err(|_| DistributionError::InvalidSignature)?;
    let signature = URL_SAFE_NO_PAD
        .decode(&artifact.signature.value)
        .map_err(|_| DistributionError::InvalidSignature)?;
    // Verify the original decoded bytes, never a reserialized statement.
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, key)
        .verify(&raw, &signature)
        .map_err(|_| DistributionError::InvalidSignature)?;
    let statement: ArtifactStatement = serde_json::from_slice(&raw).map_err(|_| DistributionError::InvalidSignature)?;
    if statement.kind != "wbombo.launcher-artifact.v1"
        || statement.app != "pandora"
        || statement.version != release.version
        || statement.platform != platform
        || statement.sha256 != artifact.sha256
        || statement.size != artifact.size
        || chrono::DateTime::parse_from_rfc3339(&statement.issued_at).is_err()
    {
        return Err(DistributionError::InvalidSignature);
    }
    Ok(VerifiedLauncherArtifact {
        artifact: artifact.clone(),
    })
}

fn verify_download(artifact: &LauncherArtifact, bytes: &[u8]) -> Result<(), DistributionError> {
    if bytes.len() as u64 != artifact.size || hex::encode(Sha256::digest(bytes)) != artifact.sha256 {
        return Err(DistributionError::InvalidObject(artifact.sha256.clone()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use schema::launcher_release::LauncherSignature;

    const PLATFORM: &str = "windows-x86_64";
    const TIME: &str = "2026-10-23T10:00:00Z";
    const PAYLOAD: &[u8] = b"synthetic launcher artifact, never executed";

    fn fixture() -> (LauncherRelease, [u8; 32]) {
        let key = Ed25519KeyPair::from_seed_unchecked(&[73; 32]).unwrap();
        let digest = hex::encode(Sha256::digest(PAYLOAD));
        let statement = serde_json::json!({
            "type":"wbombo.launcher-artifact.v1", "app":"pandora", "version":"5.4.0",
            "platform":PLATFORM, "sha256":digest, "size":PAYLOAD.len(), "issued_at":TIME,
        });
        let raw = serde_json::to_vec(&statement).unwrap();
        let public = key.public_key().as_ref().try_into().unwrap();
        (
            LauncherRelease {
                version: "5.4.0".into(),
                channel: "player".into(),
                published_at: TIME.into(),
                notes: "Synthetic notes".into(),
                artifact: LauncherArtifact {
                    platform: PLATFORM.into(),
                    file_name: "launcher.exe".into(),
                    url: format!("/v1/objects/sha256/{digest}"),
                    sha256: digest,
                    size: PAYLOAD.len() as u64,
                    statement: URL_SAFE_NO_PAD.encode(&raw),
                    signature: LauncherSignature {
                        key_id: "test".into(),
                        algorithm: "Ed25519".into(),
                        value: URL_SAFE_NO_PAD.encode(key.sign(&raw).as_ref()),
                    },
                },
            },
            public,
        )
    }

    #[test]
    fn verifies_exact_statement_and_download() {
        let (release, key) = fixture();
        let verified = verify_signature(&release, PLATFORM, "5.3.1", &key).unwrap();
        verify_download(&verified.artifact, PAYLOAD).unwrap();
        let mut changed = PAYLOAD.to_vec();
        changed[0] ^= 1;
        assert!(verify_download(&verified.artifact, &changed).is_err());
        assert!(verify_download(&verified.artifact, &PAYLOAD[..PAYLOAD.len() - 1]).is_err());
    }

    #[test]
    fn rejects_tampering_and_wrong_key() {
        let (release, key) = fixture();
        let mut changed = release.clone();
        let mut raw = URL_SAFE_NO_PAD.decode(&changed.artifact.statement).unwrap();
        raw[0] ^= 1;
        changed.artifact.statement = URL_SAFE_NO_PAD.encode(raw);
        assert!(verify_signature(&changed, PLATFORM, "5.3.1", &key).is_err());
        assert!(verify_signature(&release, PLATFORM, "5.3.1", &[0; 32]).is_err());
        changed = release.clone();
        changed.artifact.signature.algorithm = "other".into();
        assert!(verify_signature(&changed, PLATFORM, "5.3.1", &key).is_err());
        changed = release;
        changed.artifact.signature.value.push('=');
        assert!(verify_signature(&changed, PLATFORM, "5.3.1", &key).is_err());
    }

    #[test]
    fn binds_identity_size_hash_and_cas_path() {
        let (release, key) = fixture();
        let mut cases = Vec::new();
        let mut r = release.clone();
        r.version = "5.5.0".into();
        cases.push(r);
        let mut r = release.clone();
        r.artifact.size += 1;
        cases.push(r);
        let mut r = release.clone();
        r.artifact.sha256 = "a".repeat(64);
        cases.push(r);
        let mut r = release.clone();
        r.artifact.platform = "linux-x86_64".into();
        cases.push(r);
        let mut r = release.clone();
        r.artifact.url = "https://example.invalid/other".into();
        cases.push(r);
        for r in cases {
            assert!(verify_signature(&r, PLATFORM, "5.3.1", &key).is_err());
        }
        assert!(verify_signature(&release, "linux-x86_64", "5.3.1", &key).is_err());
    }

    #[test]
    fn rejects_wrong_signed_type_app_and_unknown_fields() {
        let (release, public) = fixture();
        let key = Ed25519KeyPair::from_seed_unchecked(&[73; 32]).unwrap();
        for (field, value) in [
            ("type", "wbombo.revision.v1"),
            ("app", "android"),
            ("extra", "unexpected"),
        ] {
            let mut changed = release.clone();
            let raw = URL_SAFE_NO_PAD.decode(&changed.artifact.statement).unwrap();
            let mut statement: serde_json::Value = serde_json::from_slice(&raw).unwrap();
            statement[field] = value.into();
            let raw = serde_json::to_vec(&statement).unwrap();
            changed.artifact.statement = URL_SAFE_NO_PAD.encode(&raw);
            changed.artifact.signature.value = URL_SAFE_NO_PAD.encode(key.sign(&raw).as_ref());
            assert!(verify_signature(&changed, PLATFORM, "5.3.1", &public).is_err());
        }
    }

    #[test]
    fn semver_precedence_blocks_downgrades_and_metadata_only_changes() {
        assert!(!newer("1.0.0+new", "1.0.0+old").unwrap());
        assert!(!newer("1.0.0-rc.1", "1.0.0").unwrap());
        assert!(newer("1.0.0", "1.0.0-rc.1").unwrap());
        assert!(newer("1.0.0-rc.10", "1.0.0-rc.2").unwrap());
        assert!(!newer("0.9.9", "1.0.0").unwrap());
        assert!(newer("not-semver", "1.0.0").is_err());
        let (release, key) = fixture();
        assert!(verify_signature(&release, PLATFORM, "5.4.0", &key).is_err());
        assert!(verify_signature(&release, PLATFORM, "6.0.0", &key).is_err());
    }

    fn index(releases: Vec<LauncherRelease>) -> ReleaseIndex {
        ReleaseIndex {
            schema_version: 1,
            app: "pandora".into(),
            latest: releases.first().cloned(),
            releases,
        }
    }

    #[test]
    fn validates_skipped_release_order_and_player_channel() {
        let (release, _) = fixture();
        let mut older = release.clone();
        older.version = "5.3.2".into();
        assert_eq!(
            validate_index(index(vec![release.clone(), older.clone()]), PLATFORM, LauncherChannel::Player, "5.3.1")
                .unwrap()
                .len(),
            2
        );
        assert!(
            validate_index(index(vec![older, release.clone()]), PLATFORM, LauncherChannel::Player, "5.3.1").is_err()
        );
        assert!(
            validate_index(index(vec![release.clone(), release.clone()]), PLATFORM, LauncherChannel::Player, "5.3.1")
                .is_err()
        );
        let mut dev = release;
        dev.channel = "developer".into();
        assert!(validate_index(index(vec![dev.clone()]), PLATFORM, LauncherChannel::Player, "5.3.1").is_err());
        assert!(validate_index(index(vec![dev]), PLATFORM, LauncherChannel::Developer, "5.3.1").is_ok());
        assert!(
            validate_index(index(vec![]), PLATFORM, LauncherChannel::Player, "5.3.1")
                .unwrap()
                .is_empty()
        );
    }
}
