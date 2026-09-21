use super::*;
use crate::{asset_probe_context, asset_usn_cache::AssetUsnCacheRuntime};
use rand::{RngCore, rngs::OsRng};
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
fn direct_cache_reuses_unchanged_asset_without_sha1() {
    let objects_dir = temp_objects_dir("reuse");
    std::fs::create_dir_all(&objects_dir).unwrap();
    let bytes = b"bootoptim-direct-usn-reuse";
    let (hash, expected) = sha1(bytes);
    let object_path = objects_dir.join(&hash[..2]).join(&hash);
    std::fs::create_dir_all(object_path.parent().unwrap()).unwrap();
    std::fs::write(&object_path, bytes).unwrap();

    let context = asset_probe_context("direct-usn-reuse");
    let cache = AssetUsnCacheRuntime::new(&objects_dir, "index", &context, vec![hash.clone()]);
    let seed = cache.begin_session().expect("seed session");
    assert!(seed.lookup(&hash).is_none());
    seed.record_verified(&hash, &object_path, expected)
        .expect("record seed");
    seed.finish().expect("publish seed manifest");

    let reuse = cache.begin_session().expect("reuse session");
    let evidence = reuse.lookup(&hash).expect("unchanged asset should hit");
    assert_eq!(evidence.expected_sha1, expected);
    reuse.finish().expect("finish reuse session");
    assert_eq!(cache.take_stats().reused_assets, 1);

    let _ = std::fs::remove_dir_all(objects_dir.parent().unwrap().parent().unwrap());
}

#[test]
fn direct_cache_rejects_same_size_same_mtime_mutation() {
    let objects_dir = temp_objects_dir("same-size-mtime");
    std::fs::create_dir_all(&objects_dir).unwrap();
    let original = b"abcdefghij";
    let replacement = b"0123456789";
    let (hash, expected) = sha1(original);
    let object_path = objects_dir.join(&hash[..2]).join(&hash);
    std::fs::create_dir_all(object_path.parent().unwrap()).unwrap();
    std::fs::write(&object_path, original).unwrap();

    let context = asset_probe_context("direct-usn-mutation");
    let cache = AssetUsnCacheRuntime::new(&objects_dir, "index", &context, vec![hash.clone()]);
    let seed = cache.begin_session().expect("seed session");
    seed.record_verified(&hash, &object_path, expected)
        .expect("record seed");
    seed.finish().expect("publish seed manifest");

    let original_mtime = std::fs::metadata(&object_path).unwrap().modified().unwrap();
    {
        let mut file = OpenOptions::new().write(true).open(&object_path).unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        file.write_all(replacement).unwrap();
        file.flush().unwrap();
    }
    filetime::set_file_mtime(&object_path, filetime::FileTime::from_system_time(original_mtime)).unwrap();

    let mutated = cache.begin_session().expect("mutation session");
    assert!(mutated.lookup(&hash).is_none());
    mutated.finish().expect("finish mutation session");

    let _ = std::fs::remove_dir_all(objects_dir.parent().unwrap().parent().unwrap());
}

#[test]
fn direct_cache_rejects_delete_recreate() {
    let objects_dir = temp_objects_dir("recreate");
    std::fs::create_dir_all(&objects_dir).unwrap();
    let bytes = b"delete-recreate-same-bytes";
    let (hash, expected) = sha1(bytes);
    let object_path = objects_dir.join(&hash[..2]).join(&hash);
    std::fs::create_dir_all(object_path.parent().unwrap()).unwrap();
    std::fs::write(&object_path, bytes).unwrap();

    let context = asset_probe_context("direct-usn-recreate");
    let cache = AssetUsnCacheRuntime::new(&objects_dir, "index", &context, vec![hash.clone()]);
    let seed = cache.begin_session().expect("seed session");
    seed.record_verified(&hash, &object_path, expected)
        .expect("record seed");
    seed.finish().expect("publish seed manifest");

    std::fs::remove_file(&object_path).unwrap();
    std::fs::write(&object_path, bytes).unwrap();

    let recreated = cache.begin_session().expect("recreate session");
    assert!(recreated.lookup(&hash).is_none());
    recreated.finish().expect("finish recreate session");

    let _ = std::fs::remove_dir_all(objects_dir.parent().unwrap().parent().unwrap());
}

#[test]
fn direct_cache_rejects_rename_replacement() {
    let objects_dir = temp_objects_dir("rename");
    std::fs::create_dir_all(&objects_dir).unwrap();
    let bytes = b"rename-replacement-same-bytes";
    let (hash, expected) = sha1(bytes);
    let object_path = objects_dir.join(&hash[..2]).join(&hash);
    std::fs::create_dir_all(object_path.parent().unwrap()).unwrap();
    std::fs::write(&object_path, bytes).unwrap();

    let context = asset_probe_context("direct-usn-rename");
    let cache = AssetUsnCacheRuntime::new(&objects_dir, "index", &context, vec![hash.clone()]);
    let seed = cache.begin_session().expect("seed session");
    seed.record_verified(&hash, &object_path, expected)
        .expect("record seed");
    seed.finish().expect("publish seed manifest");

    let moved = object_path.with_extension("old");
    std::fs::rename(&object_path, &moved).unwrap();
    std::fs::write(&object_path, bytes).unwrap();

    let replaced = cache.begin_session().expect("rename session");
    assert!(replaced.lookup(&hash).is_none());
    replaced.finish().expect("finish rename session");

    let _ = std::fs::remove_dir_all(objects_dir.parent().unwrap().parent().unwrap());
}
