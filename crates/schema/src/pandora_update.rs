use crate::{
    backend_config::LauncherChannel,
    launcher_release::{LauncherArtifact, LauncherRelease},
};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct UpdatePrompt {
    pub old_version: Arc<str>,
    pub new_version: Arc<str>,
    pub artifact: LauncherArtifact,
    pub releases: Vec<LauncherRelease>,
    pub channel: LauncherChannel,
    /// Hash of the configuration used to offer this update; no endpoint is exposed.
    pub source: String,
}

#[derive(Debug, Clone, Copy)]
pub enum LauncherUpdateError {
    Search,
    Verification,
}

impl LauncherUpdateError {
    pub fn message(self) -> &'static str {
        match self {
            Self::Search => "No se ha podido buscar la actualización.",
            Self::Verification => "No se ha podido verificar la actualización.",
        }
    }
}
