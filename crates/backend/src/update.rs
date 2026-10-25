//! Distribution-backed signed launcher updates. Installation never trusts UI metadata.
use std::sync::{Arc, atomic::Ordering};

use crate::{BackendState, distribution::DistributionClient};

struct LauncherTaskGuard(Arc<std::sync::atomic::AtomicUsize>);
impl LauncherTaskGuard {
    fn new(counter: Arc<std::sync::atomic::AtomicUsize>) -> Self {
        counter.fetch_add(1, Ordering::SeqCst);
        Self(counter)
    }
}
impl Drop for LauncherTaskGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn spawn_tracked<F>(counter: Arc<std::sync::atomic::AtomicUsize>, future: F) -> tokio::task::JoinHandle<F::Output>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    let guard = LauncherTaskGuard::new(counter);
    tokio::spawn(async move {
        let _guard = guard;
        future.await
    })
}

impl BackendState {
    /// Register file-changing work before scheduling it, so queued operations
    /// cannot begin after the self-update has passed its idle check.
    pub(crate) fn spawn_launcher_task<F>(&self, future: F) -> tokio::task::JoinHandle<F::Output>
    where
        F: std::future::Future + Send + 'static,
        F::Output: Send + 'static,
    {
        spawn_tracked(self.launcher_tasks.clone(), future)
    }
    pub(crate) fn spawn_launcher_task_blocking<F, T>(&self, action: F) -> tokio::task::JoinHandle<T>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        let guard = LauncherTaskGuard::new(self.launcher_tasks.clone());
        tokio::task::spawn_blocking(move || {
            let _guard = guard;
            action()
        })
    }
}

#[derive(Debug, thiserror::Error)]
enum UpdateBusy {
    #[error("a game is still running or stopping")]
    Game,
    #[error("launcher work is still active")]
    Tasks,
}
impl UpdateBusy {
    fn message(&self) -> &'static str {
        match self {
            Self::Game => "Cierra el juego antes de actualizar el launcher.",
            Self::Tasks => "Espera a que terminen las tareas en curso antes de actualizar el launcher.",
        }
    }
}

fn require_idle(
    statuses: impl Iterator<Item = bridge::instance::InstanceStatus>,
    pending_aot: bool,
) -> anyhow::Result<()> {
    use bridge::instance::InstanceStatus;
    let mut tasks = pending_aot;
    for status in statuses {
        match status {
            InstanceStatus::Running | InstanceStatus::Stopping => return Err(UpdateBusy::Game.into()),
            InstanceStatus::Launching | InstanceStatus::Optimizing => tasks = true,
            InstanceStatus::NotRunning => {},
        }
    }
    if tasks {
        return Err(UpdateBusy::Tasks.into());
    }
    Ok(())
}
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
        let mut guard = backend.config.lock();
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
        let mut guard = backend.config.lock();
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
        let message = if let Some(busy) = error.downcast_ref::<UpdateBusy>() {
            busy.message()
        } else {
            match error.downcast_ref::<crate::distribution::DistributionError>() {
                Some(
                    crate::distribution::DistributionError::InvalidSignature
                    | crate::distribution::DistributionError::InvalidSigningKey
                    | crate::distribution::DistributionError::InvalidObject(_)
                    | crate::distribution::DistributionError::ResponseTooLarge,
                ) => "No se ha podido verificar la actualización.",
                _ => "No se ha podido actualizar el launcher.",
            }
        };
        action.set_finished_with_error(message.into());
        action.set_finished();
        backend.send.send(MessageToFrontend::Refresh);
        return;
    }
    action.set_finished();
    // The replacement helper waits for this process to release its executable
    // and socket. Close windows through the existing coordinator so IPC is
    // cancelled before the backend receives its normal Quit message.
    backend.launcher_update_restarting.store(true, Ordering::SeqCst);
    backend.quit_coordinator.set_can_quit(true);
    backend.send.send(MessageToFrontend::PrepareLauncherRestart);
}

async fn install_inner(backend: &BackendState, update: &UpdatePrompt, action: &ModalAction) -> anyhow::Result<()> {
    // This runs in the serialized message handler. No new launch can begin
    // between the final idle check and handing off to the restart helper.
    let statuses: Vec<_> = backend.instance_state.read().instances.iter().map(|instance| instance.status()).collect();
    let pending_tasks = backend.launcher_tasks.load(Ordering::SeqCst) != 0
        || !backend.aot_create_requests.lock().is_empty()
        || !backend.aot_retries.lock().is_empty();
    require_idle(statuses.into_iter(), pending_tasks)?;
    let _install_guard = backend.content_install_semaphore.try_acquire_many(8).map_err(|_| UpdateBusy::Tasks)?;
    let _login_guard = backend.login_semaphore.try_acquire().map_err(|_| UpdateBusy::Tasks)?;
    let (config, channel) = {
        let mut guard = backend.config.lock();
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

#[cfg(test)]
mod tests {
    use super::*;
    use bridge::instance::InstanceStatus;

    #[test]
    fn update_requires_idle_games_and_no_pending_aot() {
        assert!(require_idle([InstanceStatus::NotRunning].into_iter(), false).is_ok());
        assert!(
            require_idle([InstanceStatus::Launching, InstanceStatus::Running].into_iter(), false)
                .unwrap_err()
                .downcast_ref::<UpdateBusy>()
                .is_some_and(|busy| matches!(busy, UpdateBusy::Game))
        );
        assert!(require_idle([InstanceStatus::Stopping].into_iter(), false).is_err());
        assert!(
            require_idle([InstanceStatus::Launching].into_iter(), false)
                .unwrap_err()
                .downcast_ref::<UpdateBusy>()
                .is_some_and(|busy| matches!(busy, UpdateBusy::Tasks))
        );
        assert!(require_idle([InstanceStatus::Optimizing].into_iter(), false).is_err());
        assert!(require_idle([InstanceStatus::NotRunning].into_iter(), true).is_err());
    }
    #[tokio::test]
    async fn queued_work_is_counted_immediately_and_released_on_cancellation() {
        let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let wait = Arc::new(tokio::sync::Notify::new());
        let task = spawn_tracked(counter.clone(), async move { wait.notified().await });
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        task.abort();
        let _ = task.await;
        assert_eq!(counter.load(Ordering::SeqCst), 0);
        spawn_tracked(counter.clone(), async {}).await.unwrap();
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }
}
