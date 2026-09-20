#![deny(unused_must_use)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::ffi::OsString;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::fmt::Write;
use std::time::{Duration, Instant, SystemTime};

use bridge::handle::{BackendHandle, FrontendHandle};
use bridge::message::{MessageToBackend, MessageToFrontend};
use bridge::quit::QuitCoordinator;
use clap::Parser;
use fern::colors::ColoredLevelConfig;
use native_dialog::DialogBuilder;
use parking_lot::RwLock;

#[derive(Parser, Debug)]
#[command()]
struct Cli {
    /// Instance to launch, instead of opening the launcher
    #[arg(long)]
    run_instance: Option<String>,
    /// Internal function to set traversable ACLs in an elevated context
    #[cfg(windows)]
    #[arg(long, hide = false, num_args = 2..)]
    internal_set_traverse_acls: Option<Vec<std::ffi::OsString>>,
}

pub mod panic;

fn main() {
    let cli = Cli::parse();

    #[cfg(windows)]
    if let Some(internal_set_traverse_acls) = cli.internal_set_traverse_acls {
        if let Err(err) = command::set_traverse_acls(internal_set_traverse_acls) {
            eprintln!("Unable to set traverse ACLs: {err}");
            std::process::exit(1);
        } else {
            std::process::exit(0);
        }
    }

    let data_dir = if let Some(portable_dir) = get_portable_dir() {
        portable_dir
    } else {
        let base_dirs = directories::BaseDirs::new().unwrap();
        base_dirs.data_dir().into()
    };

    let launcher_dir = data_dir.join("PandoraLauncher");
    _ = std::fs::create_dir_all(&launcher_dir);
    _ = std::env::set_current_dir(&launcher_dir);

    let socket = launcher_dir.join("launcher.sock");

    let lockfile_path = launcher_dir.join("launcher.lock");
    let lockfile = match OpenOptions::new().read(true).write(true).create(true).open(&lockfile_path) {
        Ok(lockfile) => lockfile,
        Err(err) => {
            show_error_eprintln(format!("Unable open launcher.lock file: {err}"));
            return;
        },
    };

    if lockfile.try_lock().is_ok() {
        setup_launcher_logging(&launcher_dir);

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

        let listener = match bind {
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

            async move {
                serve_listener(listener, listen_cancel, move |cli| {
                    run_cli(cli, &frontend_handle, &backend_handle);
                })
                .await;

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

        run_cli(cli, &frontend_handle, &backend_handle);

        backend::start(runtime, launcher_dir.clone(), frontend_handle, backend_handle.clone(), backend_recv, quit_handler.fork());
        frontend::start(launcher_dir.clone(), panic_message, deadlock_message, backend_handle, frontend_recv, quit_handler);
        log::info!("Quitting...");
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

            let bytes = match encode_cli_args(std::env::args_os()) {
                Ok(bytes) => bytes,
                Err(err) => {
                    show_error_eprintln(err.to_string());
                    return;
                },
            };

            use tokio::io::AsyncWriteExt;
            if let Err(err) = conn.write_all(&bytes).await {
                show_error_eprintln(format!("Error sending request to local socket: {err}"));
                return;
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
            listener: tokio::net::UnixListener::bind(local_path)?
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
        let pipe_name = windows_pipe_name(global_name);
        let pipe = create_windows_pipe_server(&pipe_name, true)?;

        Ok(Self { pipe_name, pipe })
    }

    async fn accept(&mut self) -> std::io::Result<PlatformServerStream> {
        self.pipe.connect().await?;
        let next_pipe = create_windows_pipe_server(&self.pipe_name, false)?;
        let old_pipe = std::mem::replace(&mut self.pipe, next_pipe);
        Ok(PlatformServerStream { server: old_pipe })
    }
}

#[cfg(windows)]
fn windows_pipe_name(global_name: &std::ffi::OsStr) -> std::ffi::OsString {
    let mut pipe_name = std::ffi::OsString::new();
    pipe_name.push(r"\\.\pipe\");
    pipe_name.push(global_name);
    pipe_name
}

#[cfg(windows)]
fn create_windows_pipe_server(
    pipe_name: &std::ffi::OsStr,
    first_pipe_instance: bool,
) -> std::io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
    let mut options = tokio::net::windows::named_pipe::ServerOptions::new();
    options.access_outbound(false);
    if first_pipe_instance {
        options.first_pipe_instance(true);
    }
    options.create(pipe_name)
}

impl PlatformServerStream {
    #[cfg(unix)]
    fn project(self: std::pin::Pin<&mut Self>) -> std::pin::Pin<&mut tokio::net::UnixStream> {
        unsafe { self.map_unchecked_mut(|s| { &mut s.stream }) }
    }

    #[cfg(windows)]
    fn project(self: std::pin::Pin<&mut Self>) -> std::pin::Pin<&mut tokio::net::windows::named_pipe::NamedPipeServer> {
        unsafe { self.map_unchecked_mut(|s| { &mut s.server }) }
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
        Ok(Self { stream: tokio::net::UnixStream::connect(local_path).await? })
    }

    fn project(self: std::pin::Pin<&mut Self>) -> std::pin::Pin<&mut tokio::net::UnixStream> {
        unsafe { self.map_unchecked_mut(|s| { &mut s.stream }) }
    }
}

#[cfg(windows)]
impl PlatformClientStream {
    async fn connect(global_name: &std::ffi::OsStr) -> std::io::Result<Self> {
        Self::connect_with_timeout(global_name, Duration::from_secs(2)).await
    }

    async fn connect_with_timeout(
        global_name: &std::ffi::OsStr,
        timeout: Duration,
    ) -> std::io::Result<Self> {
        const ERROR_PIPE_BUSY: i32 = 231;
        const RETRY_DELAY: Duration = Duration::from_millis(50);

        let pipe_name = windows_pipe_name(global_name);
        let started = Instant::now();
        let mut last_error = None;

        loop {
            match tokio::net::windows::named_pipe::ClientOptions::new()
                .read(false)
                .open(&pipe_name)
            {
                Ok(client) => return Ok(Self { client }),
                Err(err)
                    if err.raw_os_error() == Some(ERROR_PIPE_BUSY)
                        || err.kind() == std::io::ErrorKind::NotFound =>
                {
                    last_error = Some(err);
                },
                Err(err) => return Err(err),
            }

            let elapsed = started.elapsed();
            if elapsed >= timeout {
                break;
            }
            tokio::time::sleep(RETRY_DELAY.min(timeout - elapsed)).await;
        }

        let detail = last_error
            .map(|err| err.to_string())
            .unwrap_or_else(|| "no pipe instance became available".to_string());
        Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!(
                "Timed out after {} ms waiting for the running Pandora launcher named pipe (last error: {detail}). The launcher may be starting or shutting down; retry the command.",
                timeout.as_millis()
            ),
        ))
    }

    fn project(self: std::pin::Pin<&mut Self>) -> std::pin::Pin<&mut tokio::net::windows::named_pipe::NamedPipeClient> {
        unsafe { self.map_unchecked_mut(|s| { &mut s.client }) }
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

    fn poll_flush(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<std::io::Result<()>> {
        tokio::io::AsyncWrite::poll_flush(self.project(), cx)
    }

    fn poll_shutdown(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<std::io::Result<()>> {
        tokio::io::AsyncWrite::poll_shutdown(self.project(), cx)
    }
}

fn encode_cli_args(args: impl IntoIterator<Item = OsString>) -> Result<Vec<u8>, &'static str> {
    let args = args.into_iter().collect::<Vec<_>>();
    if args.len() >= u8::MAX as usize {
        return Err("Too many arguments");
    }

    let mut bytes = Vec::new();
    bytes.push(args.len() as u8);
    for arg in args {
        bytes.extend(arg.as_encoded_bytes());
        bytes.push(0);
    }
    Ok(bytes)
}

async fn read_cli_from_connection(conn: PlatformServerStream) -> Result<Cli, String> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt};

    let mut conn = tokio::io::BufReader::new(conn);
    let mut argc = [0; 1];
    conn.read_exact(&mut argc)
        .await
        .map_err(|err| format!("Error reading argument count from listener: {err}"))?;

    let mut args = Vec::with_capacity(argc[0] as usize);
    for _ in 0..argc[0] {
        let mut buf = Vec::new();
        conn.read_until(b'\0', &mut buf)
            .await
            .map_err(|err| format!("Error reading data from listener: {err}"))?;

        if buf.last().copied() != Some(0) {
            return Err("Error reading data from listener: expected last byte to be NUL byte".to_string());
        }

        buf.truncate(buf.len() - 1);
        args.push(unsafe { OsString::from_encoded_bytes_unchecked(buf) });
    }

    Cli::try_parse_from(&args).map_err(|err| format!("Error while parsing received arguments: {err}"))
}

async fn serve_listener<F>(
    mut listener: PlatformListener,
    listen_cancel: tokio_util::sync::CancellationToken,
    handler: F,
) where
    F: Fn(Cli) + Send + Sync + 'static,
{
    let handler = Arc::new(handler);
    let mut clients = tokio::task::JoinSet::new();

    loop {
        tokio::select! {
            _ = listen_cancel.cancelled() => {
                break;
            },
            conn = listener.accept() => {
                let conn = match conn {
                    Ok(conn) => conn,
                    Err(err) => {
                        log::error!("An error occurred trying to accept an incoming connection: {err}");
                        continue;
                    },
                };
                let handler = handler.clone();
                clients.spawn(async move {
                    match tokio::time::timeout(Duration::from_secs(2), read_cli_from_connection(conn)).await {
                        Ok(Ok(cli)) => handler(cli),
                        Ok(Err(err)) => log::error!("{err}"),
                        Err(_) => log::error!("Timed out reading a request from the local IPC connection"),
                    }
                });
            },
            Some(result) = clients.join_next(), if !clients.is_empty() => {
                if let Err(err) = result {
                    log::error!("Local IPC connection task failed: {err}");
                }
            },
        }
    }

    clients.abort_all();
    while clients.join_next().await.is_some() {}
}

fn run_cli(cli: Cli, frontend: &FrontendHandle, backend: &BackendHandle) {
    frontend.send(MessageToFrontend::OpenOrFocusMainWindow);

    if let Some(run_instance) = cli.run_instance {
        backend.send(bridge::message::MessageToBackend::StartInstanceByName {
            name: run_instance,
            quick_play: None,
        });
    }
}

fn setup_launcher_logging(launcher_dir: &Path) {
    let log_file = launcher_dir.join("launcher.log");
    if log_file.exists() {
        let old_log_file = launcher_dir.join("launcher.log.old");
        _ = std::fs::rename(&log_file, old_log_file);
    }

    if let Err(error) = init_logging(log::LevelFilter::Debug, &log_file) {
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

fn start_deadlock_detection(deadlock_message: &Arc<parking_lot::lock_api::RwLock<parking_lot::RawRwLock, Option<String>>>, frontend_handle: &bridge::handle::FrontendHandle) {
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

fn init_logging(level: log::LevelFilter, log_file: &Path) -> Result<(), fern::InitError> {
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
                color_line = format_args!(
                    "\x1B[{}m",
                    colors_line.get_color(&record.level()).to_fg_str()
                ),
                time = humantime::format_rfc3339_seconds(SystemTime::now()),
                level = record.level(),
                target = record.target(),
                message = message
            ))
        })
        .chain(std::io::stdout());

    base_config
        .chain(file_config)
        .chain(stdout_config)
        .apply()?;

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


#[cfg(all(test, windows))]
mod windows_ipc_tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use tokio::io::AsyncWriteExt;

    static NEXT_PIPE: AtomicU64 = AtomicU64::new(1);

    fn unique_pipe_name(label: &str) -> OsString {
        format!(
            "pandora-launcher-agent196-{label}-{}-{}",
            std::process::id(),
            NEXT_PIPE.fetch_add(1, Ordering::Relaxed)
        )
        .into()
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn partial_client_does_not_block_following_cli_and_unicode_is_delivered_once() {
        runtime().block_on(async {
            let name = unique_pipe_name("dispatch");
            let listener = PlatformListener::bind(&name).unwrap();
            let cancel = tokio_util::sync::CancellationToken::new();
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

            let server = tokio::spawn({
                let cancel = cancel.clone();
                async move {
                    serve_listener(listener, cancel, move |cli| {
                        tx.send(cli.run_instance).unwrap();
                    })
                    .await;
                }
            });

            let mut partial =
                PlatformClientStream::connect_with_timeout(&name, Duration::from_secs(1)).await.unwrap();
            partial.write_all(&[1]).await.unwrap();

            let expected = "BøøtOptimLaptop_日本語_😀";
            let mut second =
                PlatformClientStream::connect_with_timeout(&name, Duration::from_secs(1)).await.unwrap();
            second
                .write_all(
                    &encode_cli_args([
                        OsString::from("pandora-test.exe"),
                        OsString::from("--run-instance"),
                        OsString::from(expected),
                    ])
                    .unwrap(),
                )
                .await
                .unwrap();
            drop(second);

            let received = tokio::time::timeout(Duration::from_secs(1), rx.recv())
                .await
                .expect("second IPC request remained blocked")
                .expect("listener stopped before dispatch");
            assert_eq!(received.as_deref(), Some(expected));

            let mut focus_only =
                PlatformClientStream::connect_with_timeout(&name, Duration::from_secs(1)).await.unwrap();
            focus_only
                .write_all(&encode_cli_args([OsString::from("pandora-test.exe")]).unwrap())
                .await
                .unwrap();
            drop(focus_only);

            let received = tokio::time::timeout(Duration::from_secs(1), rx.recv())
                .await
                .expect("focus-only IPC request remained blocked")
                .expect("listener stopped before focus-only dispatch");
            assert_eq!(received, None);

            drop(partial);
            assert!(
                tokio::time::timeout(Duration::from_millis(150), rx.recv())
                    .await
                    .is_err(),
                "partial connection unexpectedly dispatched a CLI request"
            );

            cancel.cancel();
            tokio::time::timeout(Duration::from_secs(1), server)
                .await
                .expect("listener did not stop")
                .unwrap();
        });
    }

    #[test]
    fn busy_pipe_connect_is_bounded_and_actionable() {
        runtime().block_on(async {
            let name = unique_pipe_name("timeout");
            let pipe_name = windows_pipe_name(&name);
            let server = create_windows_pipe_server(&pipe_name, true).unwrap();
            let _client = tokio::net::windows::named_pipe::ClientOptions::new()
                .read(false)
                .open(&pipe_name)
                .unwrap();
            server.connect().await.unwrap();

            let started = Instant::now();
            let err = match PlatformClientStream::connect_with_timeout(&name, Duration::from_millis(180)).await {
                Ok(_) => panic!("a busy single pipe instance should time out"),
                Err(err) => err,
            };
            assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
            assert!(started.elapsed() < Duration::from_secs(1));
            let message = err.to_string();
            assert!(message.contains("Timed out"));
            assert!(message.contains("retry the command"));
        });
    }
}
