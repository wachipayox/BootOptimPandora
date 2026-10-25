//! Distribution's launcher release index and signed artifact envelope.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LauncherRelease {
    pub version: String,
    pub channel: String,
    pub published_at: String,
    pub notes: String,
    pub artifact: LauncherArtifact,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LauncherArtifact {
    pub platform: String,
    pub file_name: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub statement: String,
    pub signature: LauncherSignature,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LauncherSignature {
    pub key_id: String,
    pub algorithm: String,
    pub value: String,
}
