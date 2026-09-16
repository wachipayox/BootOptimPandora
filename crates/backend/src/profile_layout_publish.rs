use std::fs;

use uuid::Uuid;

use crate::profile_layout::{
    ProfileLayout, ProfileLayoutError, ProfileLayoutManifest, ProfileLayoutState,
};

impl ProfileLayout {
    /// Idempotent publication entry point. The first call executes the prepared
    /// journal transaction. A retry after the manifest commit/cleanup succeeds
    /// only when the committed manifest proves the same transaction identity.
    pub fn publish_prepared_idempotent(
        &mut self,
        transaction_id: Uuid,
    ) -> Result<(), ProfileLayoutError> {
        if self.journal_path().exists() {
            return self.publish_prepared(transaction_id);
        }

        let path = self.manifest_path();
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(ProfileLayoutError::UnsafeFilesystem(path));
        }
        let manifest: ProfileLayoutManifest = serde_json::from_slice(&fs::read(&path)?)?;
        if manifest.schema != 1
            || manifest.profile_uuid != self.profile_uuid()
            || manifest.transaction_id != Some(transaction_id)
            || manifest.state != ProfileLayoutState::Ready
        {
            return Err(ProfileLayoutError::MissingTransaction);
        }

        self.status.state = ProfileLayoutState::Ready;
        self.status.generation = Some(manifest.generation);
        self.status.stock_fallback = false;
        self.status.fallback_reason = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "pandora-profile-layout-idempotent-{}",
                Uuid::from_bytes(rand::random())
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn publication_retry_after_cleanup_is_idempotent() {
        let root = TestRoot::new();
        let mut layout = ProfileLayout::load(&root.0);
        layout
            .commit_initial_manifest(ProfileLayoutManifest::new_ready(layout.profile_uuid(), 1))
            .unwrap();
        let transaction_id = layout
            .prepare_manifest(ProfileLayoutManifest::new_ready(layout.profile_uuid(), 2))
            .unwrap();

        layout
            .publish_prepared_idempotent(transaction_id)
            .unwrap();
        assert!(!layout.journal_path().exists());
        layout
            .publish_prepared_idempotent(transaction_id)
            .unwrap();
        assert_eq!(layout.status.state, ProfileLayoutState::Ready);
        assert_eq!(layout.status.generation, Some(2));
    }
}
