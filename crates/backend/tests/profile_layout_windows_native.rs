#![cfg(windows)]

use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use backend::profile_layout_flow::PersistentProfileLayout;

fn unique_root(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "pandora-agent190-{label}-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&root).unwrap();
    fs::create_dir(root.join(".minecraft")).unwrap();
    fs::create_dir(root.join(".minecraft/mods")).unwrap();
    root
}

fn seed_existing_identity(root: &Path) {
    let control = root.join(".pandora-layout-v1");
    fs::create_dir(&control).unwrap();
    fs::write(
        control.join("identity.json"),
        br#"{"schema":1,"profile_uuid":"11111111-1111-4111-8111-111111111111"}"#
            .iter()
            .copied()
            .filter(|byte| *byte != b'\\')
            .collect::<Vec<_>>(),
    )
    .unwrap();
}

#[test]
fn corrupt_journal_on_existing_identity_preserves_live_tree() {
    let root = unique_root("corrupt-journal");
    seed_existing_identity(&root);
    let sentinel = root.join(".minecraft/mods/live-sentinel.txt");
    fs::write(&sentinel, b"live-safe").unwrap();

    let layout = PersistentProfileLayout::open(&root).unwrap();
    fs::write(layout.journal_path(), b"not-json").unwrap();
    drop(layout);

    assert!(PersistentProfileLayout::open(&root).is_err());
    assert_eq!(fs::read(&sentinel).unwrap(), b"live-safe");
}

#[test]
fn journal_junction_on_existing_identity_is_rejected_before_live_mutation() {
    let root = unique_root("journal-junction");
    seed_existing_identity(&root);
    let sentinel = root.join(".minecraft/mods/live-sentinel.txt");
    fs::write(&sentinel, b"live-safe").unwrap();

    let layout = PersistentProfileLayout::open(&root).unwrap();
    let journal = layout.journal_path();
    drop(layout);

    let outside = unique_root("journal-junction-target");
    junction::create(&outside, &journal).unwrap();

    assert!(PersistentProfileLayout::open(&root).is_err());
    assert_eq!(fs::read(&sentinel).unwrap(), b"live-safe");
    assert!(!outside.join("manifest.json").exists());
}
