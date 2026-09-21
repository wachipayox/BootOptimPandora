use std::{cell::Cell, path::Path};

thread_local! {
    static HASH_OBSERVED: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn reset_hash_observed() {
    HASH_OBSERVED.with(|slot| slot.set(false));
}

pub(crate) fn take_hash_observed() -> bool {
    HASH_OBSERVED.with(|slot| slot.replace(false))
}

fn mark_hash_observed() {
    HASH_OBSERVED.with(|slot| slot.set(true));
}

pub(crate) fn hash_path_if_active(_path: &Path, _expected: [u8; 20]) -> Option<bool> {
    mark_hash_observed();
    None
}

#[cfg(windows)]
pub(crate) fn hash_open_file_if_active(_file: &mut std::fs::File, _expected: [u8; 20]) -> Option<bool> {
    mark_hash_observed();
    None
}
