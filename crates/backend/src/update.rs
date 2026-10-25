//! Distribution-backed signed launcher updates. Installation never trusts UI metadata.
use std::sync::{Arc, atomic::Ordering};

use crate::{BackendState, distribution::DistributionClient};
use bridge::{
    message::MessageToFrontend,
    modal_action::{ModalAction, ProgressTrackerFinishType},
};
use schema::{
    backend_config::{DistributionConfig, LauncherChannel},
    pandora_update::{LauncherUpdateError, UpdatePrompt},
};
use sha2::{Digest, Sha256};

pub fn installed_version() -> &'static str {
    option_env!("PANDORA_RELEASE_VERSION")
        .unwrap_or(env!("CARGO_PKG_VERSION"))
        .trim_start_matches('v')
}

pub fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

fn source(config: &DistributionConfig) -> String {
    hex::encode(Sha256::digest(serde_json::to_vec(config).expect("DistributionConfig serializes")))
}

pub fn spawn_update_check(backend: &Arc<BackendState>, manual: bool) {
    let (distribution, channel, serial) = {
        let guard = backend.config.lock();
        let config = guard.get();
        let serial = backend.launcher_update_check_serial.fetch_add(1, Ordering::SeqCst) + 1;
        backend.send.send(MessageToFrontend::LauncherUpdateChecking { manual });
        (config.distribution.clone(), config.channel, serial)
    };
    let backend = backend.clone();
    tokio::spawn(async move {
        let result = check(&distribution, channel).await;
        // Serialize result publication with configuration changes/check starts.
        // A late developer-channel response cannot overwrite a player check.
        let guard = backend.config.lock();
        let config = guard.get();
        if backend.launcher_update_check_serial.load(Ordering::SeqCst) == serial
            && config.channel == channel
            && config.distribution == distribution
        {
            backend.send.send(MessageToFrontend::LauncherUpdateChecked { manual, result });
        }
    });
}

async fn check(
    config: &DistributionConfig,
    channel: LauncherChannel,
) -> Result<Option<UpdatePrompt>, LauncherUpdateError> {
    let search_error = |error| {
        log::warn!("Launcher update search failed: {error}");
        LauncherUpdateError::Search
    };
    let client = DistributionClient::new(config).map_err(search_error)?;
    let platform = platform();
    let releases = client
        .launcher_releases(&platform, channel, installed_version())
        .await
        .map_err(search_error)?;
    let Some(latest) = releases.first() else {
        return Ok(None);
    };
    client
        .verify_launcher_artifact(latest, &platform, installed_version())
        .await
        .map_err(|error| {
            log::warn!("Launcher update signature verification failed: {error}");
            LauncherUpdateError::Verification
        })?;
    Ok(Some(UpdatePrompt {
        old_version: installed_version().into(),
        new_version: latest.version.clone().into(),
        artifact: latest.artifact.clone(),
        releases,
        channel,
        source: source(config),
    }))
}

pub async fn install_update(backend: Arc<BackendState>, update: UpdatePrompt, action: ModalAction) {
    if let Err(error) = install_inner(&backend, &update, &action).await {
        log::warn!("Launcher update installation failed: {error}");
        action.set_finished_with_error("No se ha podido actualizar el launcher.".into());
        action.set_finished();
        backend.send.send(MessageToFrontend::Refresh);
        return;
    }
    action.set_finished();
    // The replacement helper waits for this process to release its executable
    // and socket. The backend drains/shuts down before it tells GPUI to quit.
    backend.should_quit.store(true, Ordering::SeqCst);
}

async fn install_inner(backend: &BackendState, update: &UpdatePrompt, action: &ModalAction) -> anyhow::Result<()> {
    // This runs in the serialized message handler. No new launch can begin
    // between the final idle check and handing off to the restart helper.
    if backend
        .instance_state
        .read()
        .instances
        .iter()
        .any(|i| i.status() != bridge::instance::InstanceStatus::NotRunning)
        || !backend.aot_create_requests.lock().is_empty()
        || !backend.aot_retries.lock().is_empty()
    {
        anyhow::bail!("a game or AOT operation is active");
    }
    let _install_guard = backend
        .content_install_semaphore
        .try_acquire_many(8)
        .map_err(|_| anyhow::anyhow!("content installation is active"))?;
    let (config, channel) = {
        let guard = backend.config.lock();
        (guard.get().distribution.clone(), guard.get().channel)
    };
    anyhow::ensure!(
        channel == update.channel && source(&config) == update.source,
        "update configuration changed"
    );
    anyhow::ensure!(update.old_version.as_ref() == installed_version(), "installed version changed");
    let client = DistributionClient::new(&config)?;
    let platform = platform();
    let releases = client.launcher_releases(&platform, channel, installed_version()).await?;
    let latest = releases.first().ok_or_else(|| anyhow::anyhow!("offered release was withdrawn"))?;
    anyhow::ensure!(
        latest.version == update.new_version.as_ref() && latest.artifact == update.artifact,
        "offered release changed or is no longer the latest"
    );
    let verified = client.verify_launcher_artifact(latest, &platform, installed_version()).await?;
    let tracker = action.push_tracker("Actualizando".into());
    tracker.set_total(usize::try_from(latest.artifact.size)?);
    let bytes = client.download_launcher_artifact(&verified, |count| tracker.add_count(count)).await?;
    let directory = backend.directories.temp_dir.clone();
    tokio::task::spawn_blocking(move || crate::launcher_install::install(&bytes, &directory)).await??;
    tracker.set_finished(ProgressTrackerFinishType::Normal);
    Ok(())
}
