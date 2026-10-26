#![deny(unused_must_use)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::ffi::OsString;
use std::fmt::Write;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use bridge::handle::{BackendHandle, FrontendHandle};
use bridge::message::{MessageToBackend, MessageToFrontend};
use bridge::modal_action::ModalAction;
use bridge::quit::QuitCoordinator;
use clap::Parser;
#[cfg(windows)]
use clap::ValueEnum;
use fern::colors::ColoredLevelConfig;
use native_dialog::DialogBuilder;
use parking_lot::RwLock;

#[cfg(windows)]
#[derive(Clone, Copy, Debug, ValueEnum)]
enum InternalDefenderProcessAction {
    Enable,
    Remove,
}

#[cfg(windows)]
impl From<InternalDefenderProcessAction> for command::DefenderProcessAction {
    fn from(value: InternalDefenderProcessAction) -> Self {
        match value {
            InternalDefenderProcessAction::Enable => Self::Enable,
            InternalDefenderProcessAction::Remove => Self::Remove,
        }
    }
}

#[derive(Parser, Debug)]
#[command(group = clap::ArgGroup::new("start_request").args(["run_instance", "run_instance_normal"]))]
#[command(
    group = clap::ArgGroup::new("instance_command")
        .args(["list_instances", "instance_info", "create_instance", "update_instance", "delete_instance"])
        .conflicts_with("start_request")
)]
#[command(group = clap::ArgGroup::new("revision_target").args(["create_instance", "update_instance"]))]
#[command(group = clap::ArgGroup::new("json_target").args(["list_instances", "instance_info"]))]
struct Cli {
    /// Open the launcher and launch an instance with full asset verification
    #[arg(long, conflicts_with = "run_instance_normal")]
    run_instance: Option<String>,
    /// Open the launcher and launch an instance using the GUI Start asset verification mode
    #[arg(long, conflicts_with = "run_instance")]
    run_instance_normal: Option<String>,
    /// Open or move the launcher to a one-based display number (for example, --monitor 3)
    #[arg(long, value_name = "NUMBER", value_parser = parse_monitor_number)]
    monitor: Option<usize>,
    /// With --run-instance*: apply a pending inherited profile update (like "Update and start"), then start
    #[arg(long, conflicts_with = "no_update", requires = "start_request")]
    update: bool,
    /// With --run-instance*: start unchanged even if an inherited profile update is pending
    #[arg(long, conflicts_with = "update", requires = "start_request")]
    no_update: bool,
    /// Print every instance (name, profile, revision, loader, Minecraft version, .minecraft path) and exit
    #[arg(long)]
    list_instances: bool,
    /// Print one instance and exit
    #[arg(long, value_name = "NAME")]
    instance_info: Option<String>,
    /// Create an instance of a global profile (needs --from-profile) and exit
    #[arg(long, value_name = "NAME", requires = "from_profile")]
    create_instance: Option<String>,
    /// With --create-instance: the global profile id, for example profile_wachiland-elite-beta
    #[arg(long, value_name = "PROFILE_ID", requires = "create_instance")]
    from_profile: Option<String>,
    /// Update an instance without starting it, like "Update and start" without the start, and exit
    #[arg(long, value_name = "NAME")]
    update_instance: Option<String>,
    /// Delete an instance made by --create-instance or an instance of a profile_test-* profile (needs --confirm)
    #[arg(long, value_name = "NAME", requires = "confirm")]
    delete_instance: Option<String>,
    /// With --delete-instance: the instance name again
    #[arg(long, value_name = "NAME", requires = "delete_instance")]
    confirm: Option<String>,
    /// With --create-instance or --update-instance: `latest` or an exact revision id (default: what the GUI uses)
    #[arg(long, value_name = "REVISION", value_parser = parse_revision, requires = "revision_target")]
    revision: Option<bridge::message::CliRevision>,
    /// With --list-instances or --instance-info: print JSON on stdout only
    #[arg(long, requires = "json_target")]
    json: bool,
    /// Internal function to set traversable ACLs in an elevated context
    #[cfg(windows)]
    #[arg(long, hide = false, num_args = 2..)]
    internal_set_traverse_acls: Option<Vec<std::ffi::OsString>>,
    /// Internal fixed operation for the UAC-scoped Defender process helper.
    #[cfg(windows)]
    #[arg(long, hide = true, value_enum)]
    internal_defender_process_exclusion: Option<InternalDefenderProcessAction>,
}

pub mod panic;

fn main() {
    let cli = Cli::parse();
    // Instance management never opens a window; the launcher process that runs it exits when it is done.
    let instance_command = cli_instance_command(&cli).is_some();
    let json_output = cli.json;

    if cli_start_request(&cli).is_some() || instance_command {
        attach_parent_console();
    }

    #[cfg(windows)]
    if let Some(internal_set_traverse_acls) = cli.internal_set_traverse_acls {
        if let Err(err) = command::set_traverse_acls(internal_set_traverse_acls) {
            eprintln!("Unable to set traverse ACLs: {err}");
            std::process::exit(1);
        } else {
            std::process::exit(0);
        }
    }

    #[cfg(windows)]
    if let Some(action) = cli.internal_defender_process_exclusion {
        let code = command::run_defender_process_action_elevated(action.into());
        std::process::exit(code as i32);
    }

    let data_dir = if let Some(portable_dir) = get_portable_dir() {
        portable_dir
    } else {
        let base_dirs = directories::BaseDirs::new().unwrap();
        base_dirs.data_dir().into()
    };

    let launcher_dir = data_dir.join("WachilandLauncher");
    let existing_install = launcher_dir.join("config.json").is_file() || launcher_dir.join("interface.json").is_file();
    let allow_startup = !instance_command && cli.run_instance.is_none() && cli.run_instance_normal.is_none();
    _ = std::fs::create_dir_all(&launcher_dir);
    _ = std::env::set_current_dir(&launcher_dir);

    let socket = launcher_dir.join("launcher.sock");

    let lockfile_path = launcher_dir.join("launcher.lock");
    let lockfile = match OpenOptions::new().read(true).write(true).create(true).open(&lockfile_path) {
        Ok(lockfile) => lockfile,
        Err(err) => {
            show_error_eprintln(format!("Unable to open launcher.lock file: {err}"));
            return;
        },
    };

    if lockfile.try_lock().is_ok() {
        setup_launcher_logging(&launcher_dir, !instance_command);

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("Failed to initialize Tokio runtime");

        _ = std::fs::remove_file(&socket);

        log::info!("Starting local socket: {socket:?}");
        let enter_guard = runtime.enter();

        #[cfg(unix)]
        let bind = PlatformListener::bind(&socket);
        #[cfg(windows)]
        let bind = PlatformListener::bind(std::ffi::OsStr::new("pandora-launcher-socket"));

        let mut listener = match bind {
            Ok(listener) => listener,
            Err(err) => {
                show_error(format!("Unable to start listener: {err}"));
                return;
            },
        };
        drop(enter_guard);

        let panic_message = Arc::new(RwLock::new(None));
        let deadlock_message = Arc::new(RwLock::new(None));

        let (backend_recv, backend_handle, frontend_recv, frontend_handle) = bridge::handle::create_pair();

        crate::panic::install_hook(panic_message.clone(), frontend_handle.clone());
        start_deadlock_detection(&deadlock_message, &frontend_handle);

        let listen_cancel = tokio_util::sync::CancellationToken::new();
        let listen_cancel_for_cli = listen_cancel.clone();

        // note: there are many possible race conditions with the whole single-process architecture
        // it's possible for a command to be sent to the main process while it is shutting down
        // it's possible for the socket to be dropped while the file lock is still present
        // it's possible for the file lock to be locked and the socket hasn't started yet
        // most of these can be fixed by implementing some sort of retry logic on the calling process
        // we might also need a semaphore between the listening logic and the shutdown logic, and to
        // potentially cancel the shutdown if we receive a command that results in the shutdown no longer
        // being necessary

        runtime.spawn({
            let frontend_handle = frontend_handle.clone();
            let backend_handle = backend_handle.clone();
            let listen_cancel = listen_cancel.clone();
            let mut args = Vec::new();

            async move {
                'listen: loop {
                    tokio::select! {
                        conn = listener.accept() => {
                            let conn = match conn {
                                Ok(conn) => conn,
                                Err(err) => {
                                    log::error!("An error occurred trying to handle an incoming connection: {err}");
                                    continue;
                                },
                            };
                            let mut conn = tokio::io::BufReader::new(conn);

                            use tokio::io::AsyncReadExt;
                            use tokio::io::AsyncBufReadExt;

                            let mut argc = [0; 1];
                            if let Err(err) = conn.read_exact(&mut argc).await {
                                log::error!("Error reading data from listener: {err}");
                                continue;
                            }

                            args.clear();
                            for _ in 0..argc[0] {
                                let mut buf = Vec::new();
                                if let Err(err) = conn.read_until(b'\0', &mut buf).await {
                                    log::error!("Error reading data from listener: {err}");
                                    continue 'listen;
                                }

                                if buf.last().copied() != Some(0) {
                                    log::error!("Error reading data from listener: expected last byte to be NUL byte");
                                    continue 'listen;
                                }

                                buf.truncate(buf.len() - 1);
                                args.push(unsafe { OsString::from_encoded_bytes_unchecked(buf) });
                            }

                            match Cli::try_parse_from(&args) {
                                Ok(cli) => {
                                    if let Some(outcome) = run_cli(cli, &frontend_handle, &backend_handle) {
                                        // Reply to the forwarding process: exit code byte, then the lines.
                                        tokio::spawn(async move {
                                            use tokio::io::AsyncWriteExt;
                                            let Ok(outcome) = outcome.await else { return };
                                            let mut reply = vec![outcome.exit_code];
                                            for line in &outcome.lines {
                                                reply.extend_from_slice(line.as_bytes());
                                                reply.push(b'\n');
                                            }
                                            let mut conn = conn.into_inner();
                                            _ = conn.write_all(&reply).await;
                                            _ = conn.shutdown().await;
                                        });
                                    }
                                },
                                Err(err) => {
                                    log::error!("Error while parsing received arguments: {err}");
                                    continue 'listen;
                                },
                            }
                        },
                        _ = listen_cancel.cancelled() => {
                            break;
                        }
                    }
                }

                drop(listener);
                _ = std::fs::remove_file(&socket);
                drop(lockfile);
            }
        });

        let quit_handler = {
            let backend_handle = backend_handle.clone();
            QuitCoordinator::new(Box::new(move || {
                listen_cancel.cancel();
                backend_handle.send(MessageToBackend::Quit);
                // backend will send Quit to frontend when done
                // when frontend is done, frontend::start will be unblocked and program will exit
            }))
        };

        static CLI_EXIT_CODE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
        if let Some(outcome) = run_cli(cli, &frontend_handle, &backend_handle) {
            let backend_handle = backend_handle.clone();
            let listen_cancel = listen_cancel_for_cli.clone();
            runtime.spawn(async move {
                let Ok(outcome) = outcome.await else { return };
                print_cli_lines(&outcome.lines, json_output);
                if outcome.exit_code != 0 || instance_command {
                    // Nothing was started: shut the launcher down cleanly and report the code.
                    CLI_EXIT_CODE.store(outcome.exit_code, std::sync::atomic::Ordering::SeqCst);
                    listen_cancel.cancel();
                    backend_handle.send(MessageToBackend::Quit);
                }
            });
        }

        backend::start(
            runtime,
            launcher_dir.clone(),
            frontend_handle,
            backend_handle.clone(),
            backend_recv,
            quit_handler.fork(),
        );
        frontend::start(
            launcher_dir.clone(),
            existing_install,
            allow_startup,
            panic_message,
            deadlock_message,
            backend_handle,
            frontend_recv,
            quit_handler,
        );
        log::info!("Quitting...");
        let code = CLI_EXIT_CODE.load(std::sync::atomic::Ordering::SeqCst);
        if code != 0 {
            std::process::exit(code as i32);
        }
    } else {
        eprintln!("Connecting to existing local socket: {socket:?}");

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("Failed to initialize Tokio runtime");

        runtime.block_on(async {
            #[cfg(unix)]
            let connect = PlatformClientStream::connect(&socket).await;
            #[cfg(windows)]
            let connect = PlatformClientStream::connect(std::ffi::OsStr::new("pandora-launcher-socket")).await;

            let mut conn = match connect {
                Ok(conn) => conn,
                Err(err) => {
                    show_error_eprintln(format!("Error connecting to local socket: {err}"));
                    return;
                },
            };

            let argc = std::env::args_os().len();
            if argc >= u8::MAX as usize {
                show_error_eprintln(format!("Too many arguments"));
                return;
            }

            let mut bytes = Vec::new();
            bytes.push(argc as u8);
            for arg in std::env::args_os() {
                bytes.extend(arg.as_encoded_bytes());
                bytes.push(0);
            }

            use tokio::io::AsyncWriteExt;
            if let Err(err) = conn.write_all(&bytes).await {
                show_error_eprintln(format!("Error sending request to local socket: {err}"));
                return;
            }

            // A gated --run-instance* request gets a reply; other requests (and older
            // launchers) just close the connection, which keeps exit code 0.
            use tokio::io::AsyncReadExt;
            let mut reply = Vec::new();
            if conn.read_to_end(&mut reply).await.is_ok()
                && let Some((&code, text)) = reply.split_first()
            {
                let text = String::from_utf8_lossy(text);
                print_cli_lines(&text.lines().map(str::to_owned).collect::<Vec<_>>(), json_output);
                if code != 0 {
                    std::process::exit(code as i32);
                }
            }
        });
    }
}

struct PlatformListener {
    #[cfg(unix)]
    listener: tokio::net::UnixListener,
    #[cfg(windows)]
    pipe_name: std::ffi::OsString,
    #[cfg(windows)]
    pipe: tokio::net::windows::named_pipe::NamedPipeServer,
}

struct PlatformServerStream {
    #[cfg(unix)]
    stream: tokio::net::UnixStream,
    #[cfg(windows)]
    server: tokio::net::windows::named_pipe::NamedPipeServer,
}

struct PlatformClientStream {
    #[cfg(unix)]
    stream: tokio::net::UnixStream,
    #[cfg(windows)]
    client: tokio::net::windows::named_pipe::NamedPipeClient,
}

#[cfg(unix)]
impl PlatformListener {
    fn bind(local_path: &Path) -> std::io::Result<Self> {
        Ok(Self {
            listener: tokio::net::UnixListener::bind(local_path)?,
        })
    }

    async fn accept(&mut self) -> std::io::Result<PlatformServerStream> {
        let (stream, _) = self.listener.accept().await?;
        Ok(PlatformServerStream { stream })
    }
}

#[cfg(windows)]
impl PlatformListener {
    fn bind(global_name: &std::ffi::OsStr) -> std::io::Result<Self> {
        let mut pipe_name = std::ffi::OsString::new();
        pipe_name.push(r"\\.\pipe\");
        pipe_name.push(global_name);

        let pipe = tokio::net::windows::named_pipe::ServerOptions::new()
            .access_outbound(true)
            .first_pipe_instance(true)
            .create(&pipe_name)?;

        Ok(Self { pipe_name, pipe })
    }

    async fn accept(&mut self) -> std::io::Result<PlatformServerStream> {
        self.pipe.connect().await?;
        let old_pipe = std::mem::replace(
            &mut self.pipe,
            tokio::net::windows::named_pipe::ServerOptions::new()
                .access_outbound(true)
                .create(&self.pipe_name)?,
        );
        Ok(PlatformServerStream { server: old_pipe })
    }
}

impl PlatformServerStream {
    #[cfg(unix)]
    fn project(self: std::pin::Pin<&mut Self>) -> std::pin::Pin<&mut tokio::net::UnixStream> {
        unsafe { self.map_unchecked_mut(|s| &mut s.stream) }
    }

    #[cfg(windows)]
    fn project(self: std::pin::Pin<&mut Self>) -> std::pin::Pin<&mut tokio::net::windows::named_pipe::NamedPipeServer> {
        unsafe { self.map_unchecked_mut(|s| &mut s.server) }
    }
}

impl tokio::io::AsyncRead for PlatformServerStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        tokio::io::AsyncRead::poll_read(self.project(), cx, buf)
    }
}

#[cfg(unix)]
impl PlatformClientStream {
    async fn connect(local_path: &Path) -> std::io::Result<Self> {
        Ok(Self {
            stream: tokio::net::UnixStream::connect(local_path).await?,
        })
    }

    fn project(self: std::pin::Pin<&mut Self>) -> std::pin::Pin<&mut tokio::net::UnixStream> {
        unsafe { self.map_unchecked_mut(|s| &mut s.stream) }
    }
}

#[cfg(windows)]
impl PlatformClientStream {
    async fn connect(global_name: &std::ffi::OsStr) -> std::io::Result<Self> {
        let mut pipe_name = std::ffi::OsString::new();
        pipe_name.push(r"\\.\pipe\");
        pipe_name.push(global_name);

        loop {
            match tokio::net::windows::named_pipe::ClientOptions::new().read(true).open(&pipe_name) {
                Ok(client) => return Ok(Self { client }),
                Err(e) if e.raw_os_error() == Some(231) => (), // ERROR_PIPE_BUSY
                Err(e) => return Err(e),
            }

            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    fn project(self: std::pin::Pin<&mut Self>) -> std::pin::Pin<&mut tokio::net::windows::named_pipe::NamedPipeClient> {
        unsafe { self.map_unchecked_mut(|s| &mut s.client) }
    }
}

impl tokio::io::AsyncWrite for PlatformServerStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        tokio::io::AsyncWrite::poll_write(self.project(), cx, buf)
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        tokio::io::AsyncWrite::poll_flush(self.project(), cx)
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        tokio::io::AsyncWrite::poll_shutdown(self.project(), cx)
    }
}

impl tokio::io::AsyncRead for PlatformClientStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        tokio::io::AsyncRead::poll_read(self.project(), cx, buf)
    }
}

impl tokio::io::AsyncWrite for PlatformClientStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        tokio::io::AsyncWrite::poll_write(self.project(), cx, buf)
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        tokio::io::AsyncWrite::poll_flush(self.project(), cx)
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        tokio::io::AsyncWrite::poll_shutdown(self.project(), cx)
    }
}

fn run_cli(
    cli: Cli,
    frontend: &FrontendHandle,
    backend: &BackendHandle,
) -> Option<tokio::sync::oneshot::Receiver<bridge::message::CliStartOutcome>> {
    if let Some(command) = cli_instance_command(&cli) {
        let (outcome, receiver) = tokio::sync::oneshot::channel();
        backend.send(bridge::message::MessageToBackend::CliInstanceCommand { command, outcome });
        return Some(receiver);
    }

    frontend.send(MessageToFrontend::OpenOrFocusMainWindow { monitor: cli.monitor });

    let (name, modal_action) = cli_start_request(&cli)?;
    let (outcome, receiver) = tokio::sync::oneshot::channel();
    backend.send(bridge::message::MessageToBackend::CliStartInstanceByName {
        name,
        modal_action,
        update_policy: cli_update_policy(&cli),
        outcome,
    });
    Some(receiver)
}

fn cli_update_policy(cli: &Cli) -> bridge::message::CliUpdatePolicy {
    use bridge::message::CliUpdatePolicy;
    if cli.update {
        CliUpdatePolicy::Update
    } else if cli.no_update {
        CliUpdatePolicy::NoUpdate
    } else {
        CliUpdatePolicy::Ask
    }
}

/// Release Windows builds use the GUI subsystem and get no console. For command-line starts,
/// borrow the calling shell's console so `[WBOMBO-PROFILE]` lines are visible there. Handles
/// the caller already redirected are kept, and no new console window is ever created.
#[cfg(all(windows, not(debug_assertions)))]
fn attach_parent_console() {
    use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_OUTPUT_HANDLE};
    let redirected =
        unsafe { GetStdHandle(STD_OUTPUT_HANDLE) }.is_ok_and(|handle| !handle.is_invalid() && !handle.0.is_null());
    if !redirected {
        // Fails harmlessly when the parent has no console (Explorer, scheduled task).
        _ = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
    }
}

#[cfg(not(all(windows, not(debug_assertions))))]
fn attach_parent_console() {}

/// `[WBOMBO-PROFILE]` and `[WBOMBO-INSTANCE]` lines go to stdout and stderr so wrappers can capture either
/// stream. With `--json` they go to stdout only, so the JSON is never printed twice.
fn print_cli_lines(lines: &[String], stdout_only: bool) {
    use std::io::Write as _;
    for line in lines {
        _ = writeln!(std::io::stdout(), "{line}");
        if !stdout_only {
            _ = writeln!(std::io::stderr(), "{line}");
        }
    }
    _ = std::io::stdout().flush();
}

fn parse_revision(value: &str) -> Result<bridge::message::CliRevision, String> {
    use bridge::message::CliRevision;
    if value == "latest" {
        return Ok(CliRevision::Latest);
    }
    let valid = value.len() <= 128
        && value.starts_with("rev_")
        && value.len() > 4
        && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'));
    if valid {
        Ok(CliRevision::Id(value.to_owned()))
    } else {
        Err(format!("invalid revision {value:?}: use `latest` or a revision id like rev_..."))
    }
}

/// The instance-management command the arguments ask for, if any. The parser guarantees the companion options.
fn cli_instance_command(cli: &Cli) -> Option<bridge::message::CliInstanceCommand> {
    use bridge::message::CliInstanceCommand;
    if cli.list_instances {
        return Some(CliInstanceCommand::List { json: cli.json });
    }
    if let Some(name) = &cli.instance_info {
        return Some(CliInstanceCommand::Info {
            name: name.clone(),
            json: cli.json,
        });
    }
    if let Some(name) = &cli.create_instance {
        return Some(CliInstanceCommand::Create {
            name: name.clone(),
            profile_id: cli.from_profile.clone()?,
            revision: cli.revision.clone().unwrap_or_default(),
        });
    }
    if let Some(name) = &cli.update_instance {
        return Some(CliInstanceCommand::Update {
            name: name.clone(),
            revision: cli.revision.clone().unwrap_or_default(),
        });
    }
    if let Some(name) = &cli.delete_instance {
        return Some(CliInstanceCommand::Delete {
            name: name.clone(),
            confirm: cli.confirm.clone()?,
        });
    }
    None
}

fn parse_monitor_number(value: &str) -> Result<usize, String> {
    let monitor = value.parse::<usize>().map_err(|_| format!("invalid monitor number: {value}"))?;
    if monitor == 0 {
        return Err("monitor numbers start at 1".into());
    }
    Ok(monitor)
}

fn cli_start_request(cli: &Cli) -> Option<(String, ModalAction)> {
    cli.run_instance
        .as_ref()
        .map(|name| (name.clone(), ModalAction::default()))
        .or_else(|| cli.run_instance_normal.as_ref().map(|name| (name.clone(), ModalAction::normal_launch())))
}

#[cfg(test)]
mod cli_tests {
    use super::{Cli, cli_instance_command, cli_start_request, cli_update_policy};
    use bridge::message::{CliInstanceCommand, CliRevision, CliUpdatePolicy};
    use bridge::modal_action::AssetVerificationMode;
    use clap::Parser;

    #[test]
    fn existing_run_instance_flag_keeps_full_verification() {
        let cli = Cli::try_parse_from(["pandora", "--run-instance", "Pack"]).unwrap();
        let (name, action) = cli_start_request(&cli).unwrap();
        assert_eq!(name, "Pack");
        assert_eq!(action.asset_verification_mode(), AssetVerificationMode::FullVerification);
    }

    #[test]
    fn normal_flag_selects_gui_start_verification_mode() {
        let cli = Cli::try_parse_from(["pandora", "--run-instance-normal", "Pack"]).unwrap();
        let (name, action) = cli_start_request(&cli).unwrap();
        assert_eq!(name, "Pack");
        assert_eq!(action.asset_verification_mode(), AssetVerificationMode::Normal);
    }

    #[test]
    fn run_instance_modes_are_mutually_exclusive() {
        assert!(Cli::try_parse_from(["pandora", "--run-instance", "Pack", "--run-instance-normal", "Pack",]).is_err());
    }

    #[test]
    fn update_flags_select_policy() {
        let ask = Cli::try_parse_from(["pandora", "--run-instance-normal", "Pack"]).unwrap();
        assert_eq!(cli_update_policy(&ask), CliUpdatePolicy::Ask);
        let update = Cli::try_parse_from(["pandora", "--run-instance-normal", "Pack", "--update"]).unwrap();
        assert_eq!(cli_update_policy(&update), CliUpdatePolicy::Update);
        let skip = Cli::try_parse_from(["pandora", "--run-instance", "Pack", "--no-update"]).unwrap();
        assert_eq!(cli_update_policy(&skip), CliUpdatePolicy::NoUpdate);
    }

    #[test]
    fn update_flags_need_a_start_and_exclude_each_other() {
        assert!(Cli::try_parse_from(["pandora", "--update"]).is_err());
        assert!(Cli::try_parse_from(["pandora", "--run-instance", "Pack", "--update", "--no-update"]).is_err());
    }

    #[test]
    fn no_start_flag_only_opens_launcher() {
        let cli = Cli::try_parse_from(["pandora"]).unwrap();
        assert!(cli_start_request(&cli).is_none());
        assert!(cli_instance_command(&cli).is_none());
    }

    fn command(args: &[&str]) -> Option<CliInstanceCommand> {
        let mut full = vec!["pandora"];
        full.extend_from_slice(args);
        cli_instance_command(&Cli::try_parse_from(full).unwrap())
    }

    fn rejected(args: &[&str]) -> bool {
        let mut full = vec!["pandora"];
        full.extend_from_slice(args);
        Cli::try_parse_from(full).is_err()
    }

    #[test]
    fn list_and_info_parse_with_optional_json() {
        assert_eq!(command(&["--list-instances"]), Some(CliInstanceCommand::List { json: false }));
        assert_eq!(command(&["--list-instances", "--json"]), Some(CliInstanceCommand::List { json: true }));
        assert_eq!(
            command(&["--instance-info", "Pack", "--json"]),
            Some(CliInstanceCommand::Info {
                name: "Pack".into(),
                json: true
            })
        );
    }

    #[test]
    fn create_needs_a_profile_and_takes_an_optional_revision() {
        assert_eq!(
            command(&[
                "--create-instance",
                "agente-269-x",
                "--from-profile",
                "profile_wachiland-elite-beta"
            ]),
            Some(CliInstanceCommand::Create {
                name: "agente-269-x".into(),
                profile_id: "profile_wachiland-elite-beta".into(),
                revision: CliRevision::Selected,
            })
        );
        assert_eq!(
            command(&[
                "--create-instance",
                "n",
                "--from-profile",
                "profile_test-1-a",
                "--revision",
                "latest"
            ]),
            Some(CliInstanceCommand::Create {
                name: "n".into(),
                profile_id: "profile_test-1-a".into(),
                revision: CliRevision::Latest,
            })
        );
        assert_eq!(
            command(&[
                "--create-instance",
                "n",
                "--from-profile",
                "p",
                "--revision",
                "rev_0123abc"
            ]),
            Some(CliInstanceCommand::Create {
                name: "n".into(),
                profile_id: "p".into(),
                revision: CliRevision::Id("rev_0123abc".into()),
            })
        );
        assert!(rejected(&["--create-instance", "n"]));
        assert!(rejected(&["--from-profile", "p"]));
    }

    #[test]
    fn update_takes_an_optional_revision() {
        assert_eq!(
            command(&["--update-instance", "Pack"]),
            Some(CliInstanceCommand::Update {
                name: "Pack".into(),
                revision: CliRevision::Selected
            })
        );
        assert_eq!(
            command(&["--update-instance", "Pack", "--revision", "latest"]),
            Some(CliInstanceCommand::Update {
                name: "Pack".into(),
                revision: CliRevision::Latest
            })
        );
    }

    #[test]
    fn delete_needs_its_confirmation() {
        assert_eq!(
            command(&["--delete-instance", "Pack", "--confirm", "Pack"]),
            Some(CliInstanceCommand::Delete {
                name: "Pack".into(),
                confirm: "Pack".into()
            })
        );
        assert!(rejected(&["--delete-instance", "Pack"]));
        assert!(rejected(&["--confirm", "Pack"]));
    }

    #[test]
    fn instance_commands_exclude_each_other_and_starts() {
        assert!(rejected(&["--list-instances", "--instance-info", "Pack"]));
        assert!(rejected(&[
            "--create-instance",
            "n",
            "--from-profile",
            "p",
            "--delete-instance",
            "n",
            "--confirm",
            "n"
        ]));
        assert!(rejected(&["--list-instances", "--run-instance", "Pack"]));
        assert!(rejected(&["--update-instance", "Pack", "--run-instance-normal", "Pack"]));
    }

    #[test]
    fn companion_options_need_their_command() {
        assert!(rejected(&["--json"]));
        assert!(rejected(&["--revision", "latest"]));
        assert!(rejected(&["--delete-instance", "n", "--confirm", "n", "--revision", "latest"]));
        assert!(rejected(&["--update-instance", "n", "--json"]));
    }

    #[test]
    fn revisions_must_be_latest_or_a_revision_id() {
        for bad in ["", "rev_", "stable", "REV_1", "rev_a b", "rev_a/../b"] {
            assert!(rejected(&["--update-instance", "n", "--revision", bad]), "{bad:?}");
        }
    }
}

fn setup_launcher_logging(launcher_dir: &Path, to_stdout: bool) {
    let log_file = launcher_dir.join("launcher.log");
    if log_file.exists() {
        let old_log_file = launcher_dir.join("launcher.log.old");
        _ = std::fs::rename(&log_file, old_log_file);
    }

    if let Err(error) = init_logging(log::LevelFilter::Debug, &log_file, to_stdout) {
        eprintln!("Unable to enable logging: {error:?}");
    }

    log::debug!("DEBUG logging enabled");
    log::trace!("TRACE logging enabled");

    panic::install_logging_hook();
}

fn show_error(error: String) {
    log::error!("{}", error);
    _ = DialogBuilder::message()
        .set_level(native_dialog::MessageLevel::Error)
        .set_title("An error occurred")
        .set_text(error)
        .alert()
        .show();
}

fn show_error_eprintln(error: String) {
    eprintln!("{}", error);
    _ = DialogBuilder::message()
        .set_level(native_dialog::MessageLevel::Error)
        .set_title("An error occurred")
        .set_text(error)
        .alert()
        .show();
}

fn start_deadlock_detection(
    deadlock_message: &Arc<parking_lot::lock_api::RwLock<parking_lot::RawRwLock, Option<String>>>,
    frontend_handle: &bridge::handle::FrontendHandle,
) {
    std::thread::spawn({
        let deadlock_message = deadlock_message.clone();
        let frontend_handle = frontend_handle.clone();
        move || {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(10));
                let deadlocks = parking_lot::deadlock::check_deadlock();
                if deadlocks.is_empty() {
                    continue;
                }

                let mut message = String::new();
                _ = writeln!(&mut message, "{} deadlock(s) detected", deadlocks.len());
                for (i, threads) in deadlocks.iter().enumerate() {
                    _ = writeln!(&mut message, "==== Deadlock #{} ({} threads) ====", i, threads.len());
                    for (thread_index, t) in threads.iter().enumerate() {
                        _ = writeln!(&mut message, "== Thread #{} ({:?}) ==", thread_index, t.thread_id());
                        _ = writeln!(&mut message, "{:#?}", t.backtrace());
                    }
                }

                log::error!("{}", message);
                *deadlock_message.write() = Some(message);
                frontend_handle.send(MessageToFrontend::Refresh);
                return;
            }
        }
    });
}

fn init_logging(level: log::LevelFilter, log_file: &Path, to_stdout: bool) -> Result<(), fern::InitError> {
    let base_config = fern::Dispatch::new()
        .level_for("pandora_launcher", level)
        .level_for("auth", level)
        .level_for("backend", level)
        .level_for("frontend", level)
        .level_for("bridge", level)
        .level_for("command", level)
        .level_for("gpui_component::text", log::LevelFilter::Off)
        .level(log::LevelFilter::Warn);

    let colors_line = ColoredLevelConfig::new().info(fern::colors::Color::BrightWhite);

    let file_config = fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{time} {level} {target}] {message}",
                time = humantime::format_rfc3339_seconds(SystemTime::now()),
                level = record.level(),
                target = record.target(),
                message = message
            ))
        })
        .chain(fern::log_file(log_file)?);

    let stdout_config = fern::Dispatch::new()
        .format(move |out, message, record| {
            out.finish(format_args!(
                "{color_line}[{time} {level} {target}{color_line}] {message}\x1B[0m",
                color_line = format_args!("\x1B[{}m", colors_line.get_color(&record.level()).to_fg_str()),
                time = humantime::format_rfc3339_seconds(SystemTime::now()),
                level = record.level(),
                target = record.target(),
                message = message
            ))
        })
        .chain(std::io::stdout());

    // Instance management prints its results on stdout, so the log goes to the file only.
    let base_config = base_config.chain(file_config);
    if to_stdout {
        base_config.chain(stdout_config).apply()?;
    } else {
        base_config.apply()?;
    }

    Ok(())
}

fn get_portable_dir() -> Option<PathBuf> {
    let current_exe = std::env::current_exe().ok()?;
    let file_name = current_exe.file_name()?;
    let file_name = file_name.to_string_lossy();
    if file_name.to_lowercase().contains("portable") {
        Some(current_exe.parent()?.into())
    } else {
        None
    }
}
