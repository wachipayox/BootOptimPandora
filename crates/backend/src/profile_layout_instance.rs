use crate::{instance::Instance, profile_layout::ProfileLayout};

impl Instance {
    /// Load this Instance's durable profile-layout identity and transaction state.
    /// This is intentionally not called by Start yet; Agent 185 only establishes
    /// the backend boundary for later reconciliation/fast-path work.
    pub fn load_profile_layout(&self) -> ProfileLayout {
        ProfileLayout::load(&self.root_path)
    }
}
