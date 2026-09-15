use super::*;
use crate::{asset_probe_context, asset_usn_cache::AssetUsnCacheRuntime};
use sha1::{Digest, Sha1};
use std::{
    fs::OpenOptions,
    io::{Seek, SeekFrom, Write},
    path::PathBuf,
    sync::Arc,
};

fn temp_objects_dir(label: &str) -> PathBuf {
    let mut random = [0u8; 8];
    OsRng.fill_bytes(&mut random);
    std::env::temp_dir()
        .join(format!(
            "bootoptim-usn-direct-integration-{label}-{}-{}",
            std::process::id(),
            hex::encode(random)
        ))
        .join("assets")
        .join("objects")
}

fn sha1(bytes: &[u8]) -> (String, [u8; 20]) {
    let digest = Sha1::digest(bytes);
    let mut raw = [0u8; 20];
    raw.copy_from_slice(&digest);
    (hex::encode(raw), raw)
}

#[test]
fn direct_runtime_seed_reuse_and_mutation_fallback_are_real() {
    let objects = temp_objects_dir("seed-reuse");
    let original = b"abcdefgh";
    let mutated = b"ABCDEFGH";
    let (expected_sha1, expected_hash) = sha1(original);
    let asset_dir = objects.join(&expected_sha1[..2]);
    let asset = asset_dir.join(&expected_sha1);
    std::fs::create_dir_all(&asset_dir).unwrap();
    std::fs::write(&asset, original).unwrap();

    let runtime = AssetUsnCacheRuntime::requested_for_test(true);
    let index_sha1 = "0123456789abcdef0123456789abcdef01234567";

    let seed = WindowsSession::begin(
        index_sha1,
        Arc::<Path>::from(objects.as_path()),
        vec![expected_sha1.clone()],
    )
    .unwrap();
    asset_probe_context::reset_hash_observed();
    assert!(seed.verify_existing(
        &runtime,
        AssetVerificationMode::Normal,
        &asset,
        &expected_sha1,
        expected_hash,
    ));
    assert!(asset_probe_context::take_hash_observed());
    seed.finish();
    drop(seed);

    let manifest = objects.join(".bootoptim-usn-assets-v1.json");
    assert!(manifest.is_file(), "seed must publish a complete manifest");

    let reuse = WindowsSession::begin(
        index_sha1,
        Arc::<Path>::from(objects.as_path()),
        vec![expected_sha1.clone()],
    )
    .unwrap();
    asset_probe_context::reset_hash_observed();
    assert!(reuse.verify_existing(
        &runtime,
        AssetVerificationMode::Normal,
        &asset,
        &expected_sha1,
        expected_hash,
    ));
    assert!(
        !asset_probe_context::take_hash_observed(),
        "unchanged verified reuse must not read asset content for SHA-1"
    );
    drop(reuse);

    let modified = std::fs::metadata(&asset).unwrap().modified().unwrap();
    let mut writer = OpenOptions::new().write(true).open(&asset).unwrap();
    writer.seek(SeekFrom::Start(0)).unwrap();
    writer.write_all(mutated).unwrap();
    writer.sync_all().unwrap();
    writer
        .set_times(std::fs::FileTimes::new().set_modified(modified))
        .unwrap();
    drop(writer);

    let changed = WindowsSession::begin(
        index_sha1,
        Arc::<Path>::from(objects.as_path()),
        vec![expected_sha1.clone()],
    )
    .unwrap();
    asset_probe_context::reset_hash_observed();
    assert!(!changed.verify_existing(
        &runtime,
        AssetVerificationMode::Normal,
        &asset,
        &expected_sha1,
        expected_hash,
    ));
    assert!(
        asset_probe_context::take_hash_observed(),
        "same-size restored-mtime mutation must fall back to content SHA-1"
    );
    drop(changed);

    std::fs::remove_file(&asset).unwrap();
    std::fs::write(&asset, original).unwrap();
    let recreated = WindowsSession::begin(
        index_sha1,
        Arc::<Path>::from(objects.as_path()),
        vec![expected_sha1.clone()],
    )
    .unwrap();
    asset_probe_context::reset_hash_observed();
    assert!(recreated.verify_existing(
        &runtime,
        AssetVerificationMode::Normal,
        &asset,
        &expected_sha1,
        expected_hash,
    ));
    assert!(
        asset_probe_context::take_hash_observed(),
        "delete/recreate must not consume the cached FileId as verified reuse"
    );
    drop(recreated);

    let _ = std::fs::remove_dir_all(objects.parent().unwrap().parent().unwrap());
}
