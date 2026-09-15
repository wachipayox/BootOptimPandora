use std::{cell::{Cell, RefCell}, path::Path, sync::Arc};

use crate::asset_probe::AssetAttributionProbe;

thread_local! {
    static CURRENT: RefCell<Option<Arc<AssetAttributionProbe>>> = const { RefCell::new(None) };
    static HASH_OBSERVED: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn with_probe<T>(probe: Option<Arc<AssetAttributionProbe>>, f: impl FnOnce() -> T) -> T {
    CURRENT.with(|slot| {
        let previous = slot.replace(probe);
        let result = f();
        slot.replace(previous);
        result
    })
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

pub(crate) fn hash_path_if_active(path: &Path, expected: [u8; 20]) -> Option<bool> {
    mark_hash_observed();
    CURRENT.with(|slot| slot.borrow().as_ref().map(|probe| probe.hash_path(path, expected)))
}

#[cfg(windows)]
pub(crate) fn hash_open_file_if_active(file: &mut std::fs::File, expected: [u8; 20]) -> Option<bool> {
    mark_hash_observed();
    CURRENT.with(|slot| slot.borrow().as_ref().map(|probe| probe.hash_open_file(file, expected)))
}
