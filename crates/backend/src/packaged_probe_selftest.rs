#[cfg(windows)]
use std::{
    ffi::OsString,
    io::{Error, ErrorKind},
    path::{Path, PathBuf},
    sync::Arc,
};

#[cfg(windows)]
use bridge::modal_action::{ModalAction, ProgressTrackerFinishType};
#[cfg(windows)]
use command::{PandoraCommand, PandoraStdioReadMode, PandoraStdioWriteMode};
#[cfg(windows)]
use sha1::{Digest, Sha1};

#[cfg(windows)]
pub fn run_packaged_probe_composition_selftest(args: Vec<OsString>) -> std::io::Result<()> {
    if args.len() != 3 {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "composition selftest expects trace path, asset sidecar path, and fake java path",
        ));
    }

    if std::env::var_os(crate::asset_usn_cache::ASSET_USN_CACHE_ENV).is_some() {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "composition selftest requires BOOTOPTIM_ASSET_USN_CACHE to be absent",
        ));
    }

    let trace_path = PathBuf::from(&args[0]);
    let sidecar_path = PathBuf::from(&args[1]);
    let fake_java = args[2].clone();
    let configured_trace = std::env::var_os("BOOTOPTIM_LAUNCH_PROBE").map(PathBuf::from)
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "BOOTOPTIM_LAUNCH_PROBE is missing"))?;
    let configured_sidecar = std::env::var_os(crate::asset_probe::ENV_NAME).map(PathBuf::from)
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "BOOTOPTIM_ASSET_ATTRIBUTION is missing"))?;
    if configured_trace != trace_path || configured_sidecar != sidecar_path {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "composition selftest output paths do not match configured probe paths",
        ));
    }
    if trace_path.exists() || sidecar_path.exists() {
        return Err(Error::new(
            ErrorKind::AlreadyExists,
            "composition selftest requires fresh root and sidecar paths",
        ));
    }

    let parent = trace_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "trace path needs a parent directory"))?
        .to_path_buf();
    std::fs::create_dir_all(&parent)?;

    let fixture_bytes = b"agent170-real-asset-loop-fixture";
    let expected: [u8; 20] = Sha1::digest(fixture_bytes).into();
    let expected_hex = hex::encode(expected);
    let asset_root = sidecar_path.with_extension("asset-root");
    let objects_path = asset_root.join("assets").join("objects");
    let prefix_path = objects_path.join(&expected_hex[..2]);
    std::fs::create_dir_all(&prefix_path)?;
    let asset_path = prefix_path.join(&expected_hex);
    std::fs::write(&asset_path, fixture_bytes)?;

    let index_json = format!(
        r#"{{"objects":{{"bootoptim/fixture":{{"hash":"{expected_hex}","size":{}}}}}}}"#,
        fixture_bytes.len()
    );
    let assets_index: schema::assets_index::AssetsIndex = serde_json::from_str(&index_json)
        .map_err(|error| Error::new(ErrorKind::InvalidData, error))?;

    let modal = ModalAction::normal_launch();
    let modal_key = modal.probe_key();
    bridge::launch_probe::request(modal_key);
    bridge::launch_probe::backend_dispatch(modal_key);
    bridge::launch_probe::instance_config_loaded();
    modal.clear_trackers();
    modal.clear_trackers();
    let _parent_tracker = modal.push_tracker(Arc::from("Launching"));
    let assets_tracker = modal.push_tracker(Arc::from("Verifying integrity of game assets"));

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let assets_objects_dir: Arc<Path> = Arc::from(objects_path.clone().into_boxed_path());
    let client = reqwest::Client::new();
    let asset_result = runtime.block_on(crate::launch::do_asset_objects_load(
        &client,
        Arc::new(assets_index),
        assets_objects_dir,
        "0000000000000000000000000000000000000000",
        &assets_tracker,
    ));
    assets_tracker.set_finished(ProgressTrackerFinishType::from_err(asset_result.is_err()));
    asset_result.map_err(|error| Error::new(
        ErrorKind::Other,
        format!("real asset-loop selftest failed: {error}"),
    ))?;

    let spawn_result = runtime.block_on(async move {
        let mut command = PandoraCommand::new(fake_java);
        command.arg("com.moulberry.pandora.LaunchWrapper");
        command.current_dir(&parent);
        command.stdin(PandoraStdioWriteMode::Null);
        command.stdout(PandoraStdioReadMode::Null);
        command.stderr(PandoraStdioReadMode::Null);
        let _child = command.spawn().await?;
        Ok::<(), std::io::Error>(())
    });

    let _ = std::fs::remove_dir_all(asset_root);
    spawn_result
}
