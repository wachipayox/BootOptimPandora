use super::*;
use crate::{asset_probe_context, asset_usn_cache::AssetUsnCacheRuntime};
use sha1::{Digest, Sha1};
use std::{
    fs::OpenOptions,
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
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

fn cleanup(objects: &Path) {
    let _ = std::fs::remove_dir_all(objects.parent().unwrap().parent().unwrap());
}

#[test]
fn corrupt_manifest_fast_rebootstrap_reads_no_asset_content() {
    let objects = temp_objects_dir("corrupt-rebootstrap");
    let original = b"abcdefgh";
    let (expected_sha1, _) = sha1(original);
    let asset_dir = objects.join(&expected_sha1[..2]);
    let asset = asset_dir.join(&expected_sha1);
    std::fs::create_dir_all(&asset_dir).unwrap();
    std::fs::write(&asset, original).unwrap();
    std::fs::write(objects.join(".bootoptim-usn-assets-v1.json"), b"{").unwrap();

    let runtime = AssetUsnCacheRuntime::requested_for_test(true);
    let index_sha1 = "0123456789abcdef0123456789abcdef01234567";
    let session =
        WindowsSession::begin(index_sha1, Arc::<Path>::from(objects.as_path()), vec![expected_sha1.clone()]).unwrap();

    asset_probe_context::reset_hash_observed();
    assert_eq!(
        session.verify_existing_fast(&runtime, &asset, &expected_sha1),
        FastVerifyResult::FastRebootstrap
    );
    assert!(
        !asset_probe_context::take_hash_observed(),
        "fast rebootstrap must not execute the content SHA-1 reader"
    );
    session.finish_fast();
    drop(session);

    let manifest = objects.join(".bootoptim-usn-assets-v1.json");
    let parsed = parse_manifest(&std::fs::read(manifest).unwrap()).unwrap();
    assert_eq!(parsed.asset_count, 1);
    assert_eq!(parsed.assets[0].expected_sha1, expected_sha1);
    cleanup(&objects);
}

#[test]
fn mutation_after_fast_rebootstrap_is_detected_by_usn_next_run() {
    let objects = temp_objects_dir("post-rebootstrap-mutation");
    let original = b"abcdefgh";
    let mutated = b"ABCDEFGH";
    let (expected_sha1, _) = sha1(original);
    let asset_dir = objects.join(&expected_sha1[..2]);
    let asset = asset_dir.join(&expected_sha1);
    std::fs::create_dir_all(&asset_dir).unwrap();
    std::fs::write(&asset, original).unwrap();

    let runtime = AssetUsnCacheRuntime::requested_for_test(true);
    let index_sha1 = "0123456789abcdef0123456789abcdef01234567";

    let bootstrap =
        WindowsSession::begin(index_sha1, Arc::<Path>::from(objects.as_path()), vec![expected_sha1.clone()]).unwrap();
    asset_probe_context::reset_hash_observed();
    assert_eq!(
        bootstrap.verify_existing_fast(&runtime, &asset, &expected_sha1),
        FastVerifyResult::FastRebootstrap
    );
    assert!(!asset_probe_context::take_hash_observed());
    bootstrap.finish_fast();
    drop(bootstrap);

    let unchanged =
        WindowsSession::begin(index_sha1, Arc::<Path>::from(objects.as_path()), vec![expected_sha1.clone()]).unwrap();
    asset_probe_context::reset_hash_observed();
    assert_eq!(
        unchanged.verify_existing_fast(&runtime, &asset, &expected_sha1),
        FastVerifyResult::VerifiedReuse
    );
    assert!(!asset_probe_context::take_hash_observed());
    drop(unchanged);

    let modified = std::fs::metadata(&asset).unwrap().modified().unwrap();
    let mut writer = OpenOptions::new().write(true).open(&asset).unwrap();
    writer.seek(SeekFrom::Start(0)).unwrap();
    writer.write_all(mutated).unwrap();
    writer.sync_all().unwrap();
    writer.set_times(std::fs::FileTimes::new().set_modified(modified)).unwrap();
    drop(writer);

    let changed =
        WindowsSession::begin(index_sha1, Arc::<Path>::from(objects.as_path()), vec![expected_sha1.clone()]).unwrap();
    asset_probe_context::reset_hash_observed();
    assert_eq!(
        changed.verify_existing_fast(&runtime, &asset, &expected_sha1),
        FastVerifyResult::IndividualRepairVerification
    );
    assert!(
        !asset_probe_context::take_hash_observed(),
        "USN invalidation must request only the individual repair; it must not hash the existing object"
    );
    drop(changed);

    cleanup(&objects);
}

#[test]
fn delete_recreate_after_rebootstrap_requests_individual_repair() {
    let objects = temp_objects_dir("post-rebootstrap-recreate");
    let original = b"same bytes";
    let (expected_sha1, _) = sha1(original);
    let asset_dir = objects.join(&expected_sha1[..2]);
    let asset = asset_dir.join(&expected_sha1);
    std::fs::create_dir_all(&asset_dir).unwrap();
    std::fs::write(&asset, original).unwrap();

    let runtime = AssetUsnCacheRuntime::requested_for_test(true);
    let index_sha1 = "0123456789abcdef0123456789abcdef01234567";
    let bootstrap =
        WindowsSession::begin(index_sha1, Arc::<Path>::from(objects.as_path()), vec![expected_sha1.clone()]).unwrap();
    assert_eq!(
        bootstrap.verify_existing_fast(&runtime, &asset, &expected_sha1),
        FastVerifyResult::FastRebootstrap
    );
    bootstrap.finish_fast();
    drop(bootstrap);

    std::fs::remove_file(&asset).unwrap();
    std::fs::write(&asset, original).unwrap();

    let recreated =
        WindowsSession::begin(index_sha1, Arc::<Path>::from(objects.as_path()), vec![expected_sha1.clone()]).unwrap();
    assert_eq!(
        recreated.verify_existing_fast(&runtime, &asset, &expected_sha1),
        FastVerifyResult::IndividualRepairVerification
    );
    drop(recreated);
    cleanup(&objects);
}
