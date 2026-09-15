use std::{cell::RefCell, path::Path, sync::Arc};

use crate::asset_probe::AssetAttributionProbe;

thread_local! {
    static CURRENT: RefCell<Option<Arc<AssetAttributionProbe>>> = const { RefCell::new(None) };
}

pub(crate) fn with_probe<T>(probe: Option<Arc<AssetAttributionProbe>>, f: impl FnOnce() -> T) -> T {
    CURRENT.with(|slot| {
        let previous = slot.replace(probe);
        let result = f();
        slot.replace(previous);
        result
    })
}

pub(crate) fn hash_path_if_active(path: &Path, expected: [u8; 20]) -> Option<bool> {
    CURRENT.with(|slot| slot.borrow().as_ref().map(|probe| probe.hash_path(path, expected)))
}

#[cfg(windows)]
pub(crate) fn hash_open_file_if_active(file: &mut std::fs::File, expected: [u8; 20]) -> Option<bool> {
    CURRENT.with(|slot| slot.borrow().as_ref().map(|probe| probe.hash_open_file(file, expected)))
}
