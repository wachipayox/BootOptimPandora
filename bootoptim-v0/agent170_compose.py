from pathlib import Path

launch = Path("crates/backend/src/launch/mod.rs")
text = launch.read_text()
start = text.index("async fn do_asset_objects_load(")
end = text.index("\n#[derive(thiserror::Error, Debug)]\npub enum LoadLibrariesError", start)
replacement = r'''pub(crate) async fn do_asset_objects_load(
    http_client: &reqwest::Client,
    assets_index: Arc<AssetsIndex>,
    assets_objects_dir: Arc<Path>,
    asset_index_sha1: &str,
    assets_tracker: &ProgressTracker,
) -> Result<(), LoadAssetObjectsError> {
    // Limit max concurrent connections to 8 to avoid ratelimiting issues
    let download_semaphore = tokio::sync::Semaphore::new(8);
    let disk_semaphore = tokio::sync::Semaphore::new(32);
    let started_downloading = AtomicBool::new(false);

    let planned_objects = assets_index.objects.len() as u64;
    let planned_bytes = assets_index.objects.iter()
        .map(|(_, asset)| u64::from(asset.size))
        .sum::<u64>();
    let attribution_probe = crate::asset_probe::AssetAttributionProbe::for_launch(
        planned_objects,
        planned_bytes,
    );

    let mut total_size = 0;
    let mut tasks = Vec::new();

    let _ = std::fs::create_dir_all(&assets_objects_dir);

    let verification_mode = assets_tracker.asset_verification_mode();
    let expected_hashes = assets_index.objects.iter()
        .map(|(_, asset)| asset.hash.to_string())
        .collect::<Vec<_>>();
    let cache_root = Arc::clone(&assets_objects_dir);
    let cache_index_sha1 = asset_index_sha1.to_owned();
    let cache_session = tokio::task::spawn_blocking(move || {
        crate::asset_usn_cache::AssetUsnCacheSession::begin(
            verification_mode,
            &cache_index_sha1,
            cache_root,
            expected_hashes,
        )
    }).await.unwrap_or_else(|_| {
        crate::asset_usn_cache::AssetUsnCacheSession::begin(
            bridge::modal_action::AssetVerificationMode::FullVerification,
            "",
            Arc::clone(&assets_objects_dir),
            Vec::new(),
        )
    });
    let cache_session = Arc::new(cache_session);

    for (_, asset) in &assets_index.objects {
        let mut expected_hash = [0u8; 20];
        let Ok(_) = hex::decode_to_slice(asset.hash.as_str(), &mut expected_hash) else {
            if let Some(probe) = &attribution_probe {
                probe.finish("error");
            }
            return Err(LoadAssetObjectsError::InvalidHash(asset.hash));
        };

        let mut path = assets_objects_dir.join(&asset.hash[..2]);
        let _ = std::fs::create_dir(&path);
        path.push(asset.hash.as_str());

        total_size += asset.size;

        let started_downloading = &started_downloading;
        let download_semaphore = &download_semaphore;
        let disk_semaphore = &disk_semaphore;
        let cache_session = Arc::clone(&cache_session);
        let expected_sha1 = asset.hash.to_string();
        let attribution_probe = attribution_probe.clone();

        let url = format!("https://resources.download.minecraft.net/{}/{}", &asset.hash[..2], &asset.hash);

        let task = async move {
            let valid_hash_on_disk = {
                let verify_path = path.clone();
                let stock_path = path.clone();
                let permit = disk_semaphore.acquire().await.unwrap();
                let scoped_probe = attribution_probe.clone();
                let result = match tokio::task::spawn_blocking(move || {
                    crate::asset_probe_context::with_probe(scoped_probe, || {
                        cache_session.verify_existing(&verify_path, &expected_sha1, expected_hash)
                    })
                }).await {
                    Ok(result) => result,
                    Err(_) => {
                        let fallback_probe = attribution_probe.clone();
                        tokio::task::spawn_blocking(move || {
                            fallback_probe.as_ref()
                                .map(|probe| probe.hash_path(&stock_path, expected_hash))
                                .unwrap_or_else(|| crate::fs::check_sha1_hash(&stock_path, expected_hash).unwrap_or(false))
                        }).await.unwrap_or(false)
                    },
                };
                drop(permit);
                result
            };

            if valid_hash_on_disk {
                assets_tracker.add_count(asset.size as usize);
                return Ok(());
            }

            let was_downloading = started_downloading.swap(true, std::sync::atomic::Ordering::Relaxed);
            if !was_downloading {
                assets_tracker.set_title(Arc::from("Downloading game assets"));
            }

            let permit = download_semaphore.acquire().await.unwrap();
            let download_guard = attribution_probe.as_ref().map(|probe| probe.begin_download());
            let response = match http_client.get(&url).send().await {
                Ok(response) => response,
                Err(error) => {
                    if let Some(probe) = &attribution_probe {
                        probe.record_network_error();
                    }
                    return Err(LoadAssetObjectsError::Reqwest(error));
                },
            };
            let bytes = match response.bytes().await {
                Ok(bytes) => Arc::new(bytes),
                Err(error) => {
                    if let Some(probe) = &attribution_probe {
                        probe.record_network_error();
                    }
                    return Err(LoadAssetObjectsError::Reqwest(error));
                },
            };
            drop(download_guard);
            drop(permit);

            if let Some(probe) = &attribution_probe {
                probe.record_downloaded_body(bytes.len());
            }
            if bytes.len() != asset.size as usize {
                if let Some(probe) = &attribution_probe {
                    probe.record_size_failure();
                }
                return Err(LoadAssetObjectsError::WrongResponseSize(asset.size as usize, bytes.len()));
            }

            let correct_hash = {
                let bytes = Arc::clone(&bytes);
                tokio::task::spawn_blocking(move || {
                    let mut hasher = Sha1::new();
                    hasher.update(&*bytes);
                    let actual_hash = hasher.finalize();
                    expected_hash == *actual_hash
                }).await.unwrap()
            };

            if !correct_hash {
                if let Some(probe) = &attribution_probe {
                    probe.record_download_hash_failure();
                }
                return Err(LoadAssetObjectsError::WrongHash);
            }

            tokio::fs::write(path.clone(), &*bytes).await?;
            assets_tracker.add_count(asset.size as usize);
            Ok(())
        };
        tasks.push(task);
    }

    assets_tracker.set_total(total_size as usize);
    let result = futures::future::try_join_all(tasks).await.map(|_| ());

    if result.is_ok() {
        let cache_session = Arc::clone(&cache_session);
        let finish_probe = attribution_probe.clone();
        let _ = tokio::task::spawn_blocking(move || {
            crate::asset_probe_context::with_probe(finish_probe, || cache_session.finish())
        }).await;
    }

    if let Some(probe) = &attribution_probe {
        probe.finish(if result.is_ok() { "ok" } else { "error" });
    }

    result
}
'''
launch.write_text(text[:start] + replacement + text[end:])

Path("crates/backend/src/packaged_probe_selftest.rs").write_text(r'''#[cfg(windows)]
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
''')

main = Path("crates/pandora_launcher/src/main.rs")
text = main.read_text()
marker = '''    /// Internal function to set traversable ACLs in an elevated context
    #[cfg(windows)]
    #[arg(long, hide = false, num_args = 2..)]
    internal_set_traverse_acls: Option<Vec<std::ffi::OsString>>,
}'''
insert = '''    /// Internal function to set traversable ACLs in an elevated context
    #[cfg(windows)]
    #[arg(long, hide = false, num_args = 2..)]
    internal_set_traverse_acls: Option<Vec<std::ffi::OsString>>,
    /// Internal packaged behavior gate for BootOptim root + asset attribution composition
    #[cfg(windows)]
    #[arg(long, hide = true, num_args = 3)]
    internal_bootoptim_probe_composition_selftest: Option<Vec<std::ffi::OsString>>,
}'''
if marker not in text:
    raise SystemExit("Cli marker not found")
text = text.replace(marker, insert, 1)
marker2 = '''    #[cfg(windows)]
    if let Some(internal_set_traverse_acls) = cli.internal_set_traverse_acls {'''
insert2 = '''    #[cfg(windows)]
    if let Some(args) = cli.internal_bootoptim_probe_composition_selftest {
        if let Err(err) = backend::run_packaged_probe_composition_selftest(args) {
            eprintln!("BootOptim composition selftest failed: {err}");
            std::process::exit(1);
        } else {
            std::process::exit(0);
        }
    }

    #[cfg(windows)]
    if let Some(internal_set_traverse_acls) = cli.internal_set_traverse_acls {'''
if marker2 not in text:
    raise SystemExit("main selftest marker not found")
main.write_text(text.replace(marker2, insert2, 1))

workflow = Path(".github/workflows/bootoptim-v0.yml")
text = workflow.read_text()
text = text.replace('          $interposer = (Resolve-Path "bootoptim-v0/interposer/target/x86_64-pc-windows-msvc/release/bootoptim-launch-interposer.exe").Path\n', '')
text = text.replace('          $env:BOOTOPTIM_LAUNCH_INTERPOSER = $interposer\n', '')
text = text.replace('          $env:BOOTOPTIM_APPCDS_MODE = "plan"\n', '')
text = text.replace('          Remove-Item Env:BOOTOPTIM_LAUNCH_INTERPOSER -ErrorAction SilentlyContinue\n', '')
text = text.replace('          Remove-Item Env:BOOTOPTIM_APPCDS_MODE -ErrorAction SilentlyContinue\n', '')
marker = '      - name: Verify packaged Pandora probe linkage\n'
if marker in text:
    start = text.index(marker)
    end = text.index('      - name: Assemble reproducible Windows artifact\n', start)
    text = text[:start] + text[end:]
workflow.write_text(text)

for path in [
    Path(".github/workflows/agent170-compose.yml"),
    Path(".github/workflows/agent170-compose-v2.yml"),
    Path("bootoptim-v0/agent170_compose.py"),
]:
    if path.exists():
        path.unlink()
