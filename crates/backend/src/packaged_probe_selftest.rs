#[cfg(windows)]
use std::{
    ffi::OsString,
    fs::OpenOptions,
    io::{Error, ErrorKind, Write},
    path::PathBuf,
};

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

    const MODAL: usize = 170;
    const PARENT: usize = 1700;
    const ASSETS: usize = 1701;
    const LIBRARIES: usize = 1702;
    bridge::launch_probe::request(MODAL);
    bridge::launch_probe::backend_dispatch(MODAL);
    bridge::launch_probe::instance_config_loaded();
    bridge::launch_probe::modal_clear(MODAL);
    bridge::launch_probe::modal_clear(MODAL);
    bridge::launch_probe::tracker_created(MODAL, PARENT, "Launching");
    bridge::launch_probe::tracker_created(MODAL, ASSETS, "Verifying integrity of game assets");

    let fixture = sidecar_path.with_extension("asset-fixture");
    let fixture_bytes = b"agent170-asset-fixture";
    let mut fixture_file = OpenOptions::new().write(true).create_new(true).open(&fixture)?;
    fixture_file.write_all(fixture_bytes)?;
    drop(fixture_file);

    let probe = crate::asset_probe::AssetAttributionProbe::for_launch(1, fixture_bytes.len() as u64)
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "asset attribution probe did not arm"))?;
    let expected: [u8; 20] = Sha1::digest(fixture_bytes).into();
    if !probe.hash_path(&fixture, expected) {
        return Err(Error::new(ErrorKind::InvalidData, "fixture SHA-1 unexpectedly failed"));
    }
    probe.finish("ok");
    bridge::launch_probe::tracker_finished(MODAL, ASSETS, false);
    bridge::launch_probe::tracker_created(MODAL, LIBRARIES, "Verifying integrity of game libraries");
    bridge::launch_probe::tracker_finished(MODAL, LIBRARIES, false);

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
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

    let _ = std::fs::remove_file(fixture);
    spawn_result
}
