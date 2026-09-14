use std::{collections::HashSet, io::ErrorKind, path::{Path, PathBuf}};

use ustr::Ustr;

use super::LoadAssetObjectsError;

#[derive(Default)]
pub(super) struct AssetPrefixDirectories {
    prepared: HashSet<[u8; 2]>,
}

impl AssetPrefixDirectories {
    pub(super) fn object_path(
        &mut self,
        assets_objects_dir: &Path,
        hash: Ustr,
    ) -> Result<([u8; 20], PathBuf), LoadAssetObjectsError> {
        let mut expected_hash = [0u8; 20];
        if hex::decode_to_slice(hash.as_str(), &mut expected_hash).is_err() {
            return Err(LoadAssetObjectsError::InvalidHash(hash));
        }

        // decode_to_slice above proves an exact 40-byte ASCII hexadecimal SHA-1,
        // so this is the same safe two-byte string prefix stock Pandora used.
        let prefix = [hash.as_bytes()[0], hash.as_bytes()[1]];
        let prefix_path = assets_objects_dir.join(&hash.as_str()[..2]);
        self.prepare_prefix(prefix, &prefix_path);

        Ok((expected_hash, prefix_path.join(hash.as_str())))
    }

    fn prepare_prefix(&mut self, prefix: [u8; 2], prefix_path: &Path) {
        prepare_prefix_with(
            &mut self.prepared,
            prefix,
            prefix_path,
            |path| std::fs::create_dir(path),
            |path| path.is_dir(),
        );
    }
}

fn prepare_prefix_with<CreateDir, IsDir>(
    prepared: &mut HashSet<[u8; 2]>,
    prefix: [u8; 2],
    prefix_path: &Path,
    create_dir: CreateDir,
    is_dir: IsDir,
) where
    CreateDir: FnOnce(&Path) -> std::io::Result<()>,
    IsDir: FnOnce(&Path) -> bool,
{
    if prepared.contains(&prefix) {
        return;
    }

    let ready = match create_dir(prefix_path) {
        Ok(()) => true,
        Err(error) if error.kind() == ErrorKind::AlreadyExists => is_dir(prefix_path),
        // Stock Pandora ignores this error and tries create_dir again for the
        // next object. Do not cache failure: retaining that retry opportunity
        // preserves recovery from transient creation/permission conditions.
        Err(_) => false,
    };

    if ready {
        prepared.insert(prefix);
    }
}

#[cfg(test)]
mod bootoptim_asset_prefix_directory_tests {
    use std::{
        fs,
        io,
        sync::atomic::{AtomicU64, Ordering},
    };

    use schema::assets_index::AssetsIndex;

    use super::*;

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_root(label: &str) -> PathBuf {
        let id = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "bootoptim-asset-prefix-{label}-{}-{id}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn empty_directory_creates_only_required_prefix() {
        let root = temp_root("empty");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);

        let hash = Ustr::from("ab00000000000000000000000000000000000000");
        let mut prefixes = AssetPrefixDirectories::default();
        let (_, object_path) = prefixes.object_path(&root, hash).unwrap();

        assert!(root.join("ab").is_dir());
        assert_eq!(object_path, root.join("ab").join(hash.as_str()));
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn repeated_prefix_is_created_once_after_success() {
        let mut prepared = HashSet::new();
        let mut create_calls = 0;
        let path = Path::new("ab");

        for _ in 0..3 {
            prepare_prefix_with(
                &mut prepared,
                *b"ab",
                path,
                |_| {
                    create_calls += 1;
                    Ok(())
                },
                |_| false,
            );
        }

        assert_eq!(create_calls, 1);
        assert!(prepared.contains(b"ab"));
    }

    #[test]
    fn already_existing_directory_is_confirmed_then_deduplicated() {
        let root = temp_root("existing");
        fs::create_dir(root.join("cd")).unwrap();
        let mut prefixes = AssetPrefixDirectories::default();
        let first_hash = Ustr::from("cd00000000000000000000000000000000000000");
        let second_hash = Ustr::from("cd11111111111111111111111111111111111111");

        let (_, first_path) = prefixes.object_path(&root, first_hash).unwrap();
        let (_, second_path) = prefixes.object_path(&root, second_hash).unwrap();

        assert_eq!(first_path, root.join("cd").join(first_hash.as_str()));
        assert_eq!(second_path, root.join("cd").join(second_hash.as_str()));
        assert!(prefixes.prepared.contains(b"cd"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn permission_failure_is_ignored_and_retried() {
        let mut prepared = HashSet::new();
        let mut create_calls = 0;
        let path = Path::new("ef");

        for _ in 0..2 {
            prepare_prefix_with(
                &mut prepared,
                *b"ef",
                path,
                |_| {
                    create_calls += 1;
                    Err(io::Error::from(ErrorKind::PermissionDenied))
                },
                |_| panic!("is_dir must not run for PermissionDenied"),
            );
        }

        assert_eq!(create_calls, 2);
        assert!(!prepared.contains(b"ef"));
    }

    #[test]
    fn prefix_identity_preserves_hash_case() {
        let mut prepared = HashSet::new();
        let mut create_calls = 0;

        prepare_prefix_with(
            &mut prepared,
            *b"ab",
            Path::new("ab"),
            |_| {
                create_calls += 1;
                Ok(())
            },
            |_| false,
        );
        prepare_prefix_with(
            &mut prepared,
            *b"AB",
            Path::new("AB"),
            |_| {
                create_calls += 1;
                Ok(())
            },
            |_| false,
        );

        assert_eq!(create_calls, 2);
        assert!(prepared.contains(b"ab"));
        assert!(prepared.contains(b"AB"));
    }

    #[test]
    fn invalid_hash_map_preserves_stock_error_and_prior_prefix_side_effect() {
        let index: AssetsIndex = serde_json::from_str(
            r#"{
                "objects": {
                    "valid": {"hash":"ab00000000000000000000000000000000000000","size":1},
                    "invalid": {"hash":"not-a-sha1","size":1}
                }
            }"#,
        ).unwrap();
        let root = temp_root("invalid-hash");
        let mut prefixes = AssetPrefixDirectories::default();
        let mut error = None;

        for asset in index.objects.values() {
            if let Err(err) = prefixes.object_path(&root, asset.hash) {
                error = Some(err);
                break;
            }
        }

        assert!(root.join("ab").is_dir());
        assert!(matches!(
            error,
            Some(LoadAssetObjectsError::InvalidHash(hash)) if hash.as_str() == "not-a-sha1"
        ));

        fs::remove_dir_all(root).unwrap();
    }
}
