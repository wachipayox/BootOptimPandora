//! Per-instance lock for inherited profile update operations.
//!
//! Shared by the GUI (Start check, "Update and start", "Update inherited files") and the
//! command-line `--update` gate so the same instance is never updated twice at once.

use std::{collections::HashSet, sync::LazyLock};

use parking_lot::Mutex;

use crate::instance::InstanceID;

static ACTIVE: LazyLock<Mutex<HashSet<InstanceID>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// Returns false when an inherited operation is already running for `id`.
pub fn try_begin(id: InstanceID) -> bool {
    ACTIVE.lock().insert(id)
}

pub fn finish(id: InstanceID) {
    ACTIVE.lock().remove(&id);
}

pub fn is_active(id: InstanceID) -> bool {
    ACTIVE.lock().contains(&id)
}
