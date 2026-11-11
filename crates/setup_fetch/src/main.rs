//! Downloader embedded in the Windows setup (`package/windows/installer.nsi`).
//!
//! `wachiland-setup-fetch --out <file> --info <ini> [--progress <hwnd>] [--status <hwnd>]`
//!
//! Fetches the latest `player` release of the launcher for `windows-x86_64` from Distribution, verifies
//! it exactly like the in-place updater (`launcher_artifact`: Ed25519 statement signed with the built-in
//! key or a key discovered over HTTPS that never contradicts it, identity, size and SHA-256), and only
//! then moves it to `<file>`. Nothing is left behind on failure. The setup's progress bar and status
//! line are updated through their window handles; log lines go to stdout in the ANSI code page, which
//! is what `nsExec::ExecToLog` decodes.
//!
//! The origin is injected at build time (`WBOMBO_DISTRIBUTION_DEFAULT_URL`, the launcher's own default)
//! and never appears in the repository. `WBOMBO_SETUP_TEST_ORIGIN` (with `WBOMBO_SETUP_TEST_CA`, a PEM
//! root) points it at a test server, and only on a loopback address.
//!
//! Exit codes, read by the setup: 0 installed file ready; 10 built without an origin; 20 server
//! unreachable or not answering (retry); 21 no release published yet (retry); 30 verification failed;
//! 40 local file error; 2 wrong arguments.
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    time::{Duration, Instant},
};

use launcher_artifact::{LauncherRelease, ReleaseIndex};
use reqwest::{Url, blocking::Client};
use sha2::{Digest, Sha256};

const PLATFORM: &str = "windows-x86_64";
const MAX_JSON_BYTES: u64 = 4 * 1024 * 1024;
const TEST_ORIGIN_ENV: &str = "WBOMBO_SETUP_TEST_ORIGIN";
const TEST_CA_ENV: &str = "WBOMBO_SETUP_TEST_CA";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Usage = 2,
    NoOrigin = 10,
    Unreachable = 20,
    NoRelease = 21,
    Unverified = 30,
    Local = 40,
}

#[derive(Debug)]
struct Failure {
    kind: Kind,
    detail: String,
}

fn fail(kind: Kind, detail: impl Into<String>) -> Failure {
    Failure {
        kind,
        detail: detail.into(),
    }
}

fn main() -> ExitCode {
    let args = match Args::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(failure) => return report(failure),
    };
    let ui = Ui::new(args.progress, args.status);
    match run(&args, &ui) {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => report(failure),
    }
}

fn report(failure: Failure) -> ExitCode {
    say(&format!("ERROR: {}", failure.detail));
    ExitCode::from(failure.kind as u8)
}

struct Args {
    out: PathBuf,
    info: PathBuf,
    progress: Option<isize>,
    status: Option<isize>,
}

impl Args {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self, Failure> {
        let (mut out, mut info, mut progress, mut status) = (None, None, None, None);
        while let Some(flag) = args.next() {
            let value = args.next().ok_or_else(|| fail(Kind::Usage, format!("falta el valor de {flag}")))?;
            let handle = || value.parse::<isize>().ok().filter(|handle| *handle != 0);
            match flag.as_str() {
                "--out" => out = Some(PathBuf::from(&value)),
                "--info" => info = Some(PathBuf::from(&value)),
                "--progress" => progress = handle(),
                "--status" => status = handle(),
                _ => return Err(fail(Kind::Usage, format!("argumento desconocido {flag}"))),
            }
        }
        Ok(Self {
            out: out.ok_or_else(|| fail(Kind::Usage, "falta --out"))?,
            info: info.ok_or_else(|| fail(Kind::Usage, "falta --info"))?,
            progress,
            status,
        })
    }
}

fn run(args: &Args, ui: &Ui) -> Result<(), Failure> {
    let (origin, test_root) = origin()?;
    let client = client(test_root)?;

    ui.status("Buscando la última versión de Wachiland Launcher…");
    say("Buscando la última versión publicada de Wachiland Launcher…");
    let release = latest_release(&client, &origin)?;
    say(&format!(
        "Última versión publicada: {} ({}).",
        release.version,
        megabytes(release.artifact.size)
    ));

    let key = signing_key(&client, &origin, &release.artifact.signature.key_id)?;
    launcher_artifact::verify_signature(&release, PLATFORM, None, &key)
        .map_err(|_| fail(Kind::Unverified, "la firma de la versión publicada no es válida"))?;
    say(&format!("Firma verificada (clave {}).", release.artifact.signature.key_id));

    download(&client, &origin, &release, &args.out, ui)?;
    say(&format!("Descarga verificada (SHA-256 {}).", release.artifact.sha256));

    let info = format!(
        "[release]\r\nversion={}\r\nsize_kb={}\r\n",
        release.version,
        release.artifact.size.div_ceil(1024)
    );
    fs::write(&args.info, info).map_err(|error| {
        let _ = fs::remove_file(&args.out);
        fail(Kind::Local, format!("no se ha podido escribir {}: {error}", args.info.display()))
    })?;
    Ok(())
}

/// The built-in origin, or a loopback test origin from the environment.
fn origin() -> Result<(Url, Option<Vec<u8>>), Failure> {
    if let Ok(test) = std::env::var(TEST_ORIGIN_ENV) {
        let url = parse_origin(&test).ok_or_else(|| fail(Kind::NoOrigin, format!("{TEST_ORIGIN_ENV} no es válido")))?;
        if !matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")) {
            return Err(fail(Kind::NoOrigin, format!("{TEST_ORIGIN_ENV} solo admite una dirección local")));
        }
        let root = match std::env::var(TEST_CA_ENV) {
            Ok(path) => Some(
                fs::read(&path)
                    .map_err(|error| fail(Kind::NoOrigin, format!("no se ha podido leer {TEST_CA_ENV}: {error}")))?,
            ),
            Err(_) => None,
        };
        return Ok((url, root));
    }
    let built = option_env!("WBOMBO_DISTRIBUTION_DEFAULT_URL").unwrap_or_default();
    parse_origin(built)
        .map(|url| (url, None))
        .ok_or_else(|| fail(Kind::NoOrigin, "este instalador se compiló sin servidor de descargas"))
}

/// HTTPS only, no credentials, query or fragment (the launcher's `parse_base_url`).
fn parse_origin(value: &str) -> Option<Url> {
    let url = Url::parse(value.trim()).ok()?;
    (url.scheme() == "https"
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none())
    .then_some(url)
}

fn join(origin: &Url, path: &str) -> Result<Url, Failure> {
    let base = format!("{}/", origin.as_str().trim_end_matches('/'));
    Url::parse(&base)
        .and_then(|base| base.join(path.trim_start_matches('/')))
        .map_err(|_| fail(Kind::Unverified, "ruta de descarga no válida"))
}

fn client(test_root: Option<Vec<u8>>) -> Result<Client, Failure> {
    let mut builder = Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::none())
        .https_only(true)
        .user_agent(concat!("WachilandSetup/", env!("CARGO_PKG_VERSION")));
    if let Some(pem) = test_root {
        let certificate = reqwest::Certificate::from_pem(&pem)
            .map_err(|error| fail(Kind::NoOrigin, format!("certificado de prueba no válido: {error}")))?;
        builder = builder.add_root_certificate(certificate);
    }
    builder
        .build()
        .map_err(|error| fail(Kind::Local, format!("no se ha podido preparar la conexión: {}", describe(error))))
}

/// A request error without the URL (the origin stays out of the installer log) plus its causes.
fn describe(error: reqwest::Error) -> String {
    let error = error.without_url();
    let mut text = error.to_string();
    let mut source = std::error::Error::source(&error);
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

fn get(client: &Client, url: Url, timeout: Duration) -> Result<reqwest::blocking::Response, Failure> {
    let response = client.get(url).timeout(timeout).send().map_err(|error| {
        fail(
            Kind::Unreachable,
            format!("no se ha podido conectar con el servidor: {}", describe(error)),
        )
    })?;
    if response.status() != reqwest::StatusCode::OK {
        return Err(fail(
            Kind::Unreachable,
            format!("el servidor respondió HTTP {}", response.status().as_u16()),
        ));
    }
    Ok(response)
}

fn get_json<T: serde::de::DeserializeOwned>(client: &Client, url: Url) -> Result<T, Failure> {
    let response = get(client, url, Duration::from_secs(60))?;
    let mut body = Vec::new();
    response
        .take(MAX_JSON_BYTES + 1)
        .read_to_end(&mut body)
        .map_err(|error| fail(Kind::Unreachable, format!("se ha cortado la respuesta del servidor: {error}")))?;
    if body.len() as u64 > MAX_JSON_BYTES {
        return Err(fail(Kind::Unverified, "la respuesta del servidor es demasiado grande"));
    }
    serde_json::from_slice(&body).map_err(|_| fail(Kind::Unverified, "la respuesta del servidor no es válida"))
}

fn latest_release(client: &Client, origin: &Url) -> Result<LauncherRelease, Failure> {
    let mut url = join(origin, "/v1/launcher/pandora/releases")?;
    url.query_pairs_mut().append_pair("platform", PLATFORM).append_pair("channel", "player");
    let index: ReleaseIndex = get_json(client, url)?;
    let releases = launcher_artifact::validate_index(index, PLATFORM, true, None)
        .map_err(|_| fail(Kind::Unverified, "la lista de versiones del servidor no es válida"))?;
    releases
        .into_iter()
        .next()
        .ok_or_else(|| fail(Kind::NoRelease, "todavía no hay ninguna versión publicada para Windows"))
}

#[derive(serde::Deserialize)]
struct KeyCatalog {
    schema_version: u32,
    keys: BTreeMap<String, String>,
}

/// The built-in key, or one discovered over HTTPS that never gives the built-in id another key.
fn signing_key(client: &Client, origin: &Url, id: &str) -> Result<[u8; 32], Failure> {
    let pinned = launcher_artifact::pinned_keys();
    if let Some(key) = pinned.get(id) {
        return Ok(*key);
    }
    let catalog: KeyCatalog = get_json(client, join(origin, "/v1/signing-keys")?)?;
    let keys = launcher_artifact::validate_key_catalog(catalog.schema_version, catalog.keys, &pinned)
        .map_err(|_| fail(Kind::Unverified, "las claves de firma del servidor no son válidas"))?;
    keys.get(id)
        .copied()
        .ok_or_else(|| fail(Kind::Unverified, format!("la versión está firmada con una clave desconocida ({id})")))
}

/// Removes the partial file unless the download was verified.
struct Partial<'a> {
    path: &'a Path,
    keep: bool,
}

impl Drop for Partial<'_> {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_file(self.path);
        }
    }
}

fn download(client: &Client, origin: &Url, release: &LauncherRelease, out: &Path, ui: &Ui) -> Result<(), Failure> {
    let artifact = &release.artifact;
    let mut response = get(client, join(origin, &artifact.url)?, Duration::from_secs(30 * 60))?;
    if response.content_length().is_some_and(|length| length != artifact.size) {
        return Err(fail(Kind::Unverified, "el tamaño de la descarga no coincide con el firmado"));
    }
    let partial_path = PathBuf::from(format!("{}.part", out.display()));
    let mut partial = Partial {
        path: &partial_path,
        keep: false,
    };
    let mut file = fs::File::create(&partial_path)
        .map_err(|error| fail(Kind::Local, format!("no se ha podido crear {}: {error}", partial_path.display())))?;
    let title = format!("Descargando Wachiland Launcher {}", release.version);
    let mut hasher = Sha256::new();
    let mut received = 0u64;
    let mut buffer = vec![0u8; 256 * 1024];
    let mut last_update = Instant::now() - Duration::from_secs(1);
    loop {
        let read = response
            .read(&mut buffer)
            .map_err(|error| fail(Kind::Unreachable, format!("se ha cortado la descarga: {error}")))?;
        if read == 0 {
            break;
        }
        received += read as u64;
        if received > artifact.size {
            return Err(fail(Kind::Unverified, "la descarga es mayor que el tamaño firmado"));
        }
        hasher.update(&buffer[..read]);
        file.write_all(&buffer[..read])
            .map_err(|error| fail(Kind::Local, format!("no se ha podido escribir la descarga: {error}")))?;
        if last_update.elapsed() >= Duration::from_millis(100) {
            last_update = Instant::now();
            ui.progress(received, artifact.size);
            ui.status(&format!("{title}: {} de {}", megabytes(received), megabytes(artifact.size)));
        }
    }
    ui.progress(received, artifact.size);
    ui.status(&format!("{title}: comprobando la descarga…"));
    file.sync_all()
        .map_err(|error| fail(Kind::Local, format!("no se ha podido guardar la descarga: {error}")))?;
    drop(file);
    launcher_artifact::verify_download_digest(artifact, received, &hex::encode(hasher.finalize()))
        .map_err(|_| fail(Kind::Unverified, "la descarga no coincide con la versión firmada (tamaño o SHA-256)"))?;
    fs::rename(&partial_path, out)
        .map_err(|error| fail(Kind::Local, format!("no se ha podido preparar {}: {error}", out.display())))?;
    partial.keep = true;
    Ok(())
}

/// `12,3 MB` with a decimal comma, as the setup is in Spanish.
fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0).replace('.', ",")
}

/// The setup's progress bar and status line, owned by the installer process.
struct Ui {
    progress: Option<isize>,
    status: Option<isize>,
    range: u32,
}

impl Ui {
    fn new(progress: Option<isize>, status: Option<isize>) -> Self {
        let range = progress.map_or(0, platform::progress_range);
        Self {
            progress,
            status,
            range,
        }
    }

    fn progress(&self, done: u64, total: u64) {
        if let Some(window) = self.progress
            && self.range > 0
            && total > 0
        {
            let position = (done.min(total) as u128 * self.range as u128 / total as u128) as usize;
            platform::set_progress(window, position);
        }
    }

    fn status(&self, text: &str) {
        if let Some(window) = self.status {
            platform::set_text(window, text);
        }
    }
}

/// Writes one log line in the ANSI code page.
fn say(text: &str) {
    let mut stdout = std::io::stdout().lock();
    let _ = stdout.write_all(&platform::to_ansi(text));
    let _ = stdout.write_all(b"\r\n");
    let _ = stdout.flush();
}

#[cfg(windows)]
mod platform {
    const WM_SETTEXT: u32 = 0x000C;
    const PBM_SETPOS: u32 = 0x0400 + 2;
    const PBM_GETRANGE: u32 = 0x0400 + 7;
    const CP_ACP: u32 = 0;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn IsWindow(window: isize) -> i32;
        fn SendMessageW(window: isize, message: u32, wparam: usize, lparam: isize) -> isize;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn WideCharToMultiByte(
            code_page: u32,
            flags: u32,
            wide: *const u16,
            wide_len: i32,
            out: *mut u8,
            out_len: i32,
            default_char: *const u8,
            used_default: *mut i32,
        ) -> i32;
    }

    fn valid(window: isize) -> bool {
        // SAFETY: IsWindow accepts any value and only reports whether it names a window.
        unsafe { IsWindow(window) != 0 }
    }

    pub fn progress_range(window: isize) -> u32 {
        if !valid(window) {
            return 0;
        }
        // SAFETY: PBM_GETRANGE with a null PBRANGE only returns the upper limit.
        unsafe { SendMessageW(window, PBM_GETRANGE, 0, 0) }.max(0) as u32
    }

    pub fn set_progress(window: isize, position: usize) {
        if valid(window) {
            // SAFETY: PBM_SETPOS takes the position by value.
            unsafe { SendMessageW(window, PBM_SETPOS, position, 0) };
        }
    }

    pub fn set_text(window: isize, text: &str) {
        if valid(window) {
            let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
            // SAFETY: WM_SETTEXT reads a null-terminated UTF-16 string that outlives the synchronous call;
            // the system marshals it to the window's process.
            unsafe { SendMessageW(window, WM_SETTEXT, 0, wide.as_ptr() as isize) };
        }
    }

    pub fn to_ansi(text: &str) -> Vec<u8> {
        let wide: Vec<u16> = text.encode_utf16().collect();
        if wide.is_empty() {
            return Vec::new();
        }
        // SAFETY: the first call only measures; the second writes at most `size` bytes into `out`.
        unsafe {
            let size = WideCharToMultiByte(
                CP_ACP,
                0,
                wide.as_ptr(),
                wide.len() as i32,
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
                std::ptr::null_mut(),
            );
            if size <= 0 {
                return text.as_bytes().to_vec();
            }
            let mut out = vec![0u8; size as usize];
            WideCharToMultiByte(
                CP_ACP,
                0,
                wide.as_ptr(),
                wide.len() as i32,
                out.as_mut_ptr(),
                size,
                std::ptr::null(),
                std::ptr::null_mut(),
            );
            out
        }
    }
}

#[cfg(not(windows))]
mod platform {
    pub fn progress_range(_window: isize) -> u32 {
        0
    }
    pub fn set_progress(_window: isize, _position: usize) {}
    pub fn set_text(_window: isize, _text: &str) {}
    pub fn to_ansi(text: &str) -> Vec<u8> {
        text.as_bytes().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_must_be_plain_https() {
        assert!(parse_origin("https://example.invalid:9443").is_some());
        assert!(parse_origin("http://example.invalid").is_none());
        assert!(parse_origin("https://user@example.invalid").is_none());
        assert!(parse_origin("https://example.invalid/?a=1").is_none());
        assert!(parse_origin("").is_none());
    }

    #[test]
    fn paths_stay_on_the_origin() {
        let origin = parse_origin("https://example.invalid:9443").unwrap();
        assert_eq!(
            join(&origin, "/v1/objects/sha256/ab").unwrap().as_str(),
            "https://example.invalid:9443/v1/objects/sha256/ab"
        );
    }

    #[test]
    fn sizes_use_a_decimal_comma() {
        assert_eq!(megabytes(74_874_368), "74,9 MB");
    }

    #[test]
    fn arguments_need_out_and_info() {
        let args = |list: &[&str]| Args::parse(list.iter().map(|arg| arg.to_string()));
        assert!(args(&["--out", "a"]).is_err());
        let parsed = args(&["--out", "a", "--info", "b", "--progress", "0", "--status", "42"]).ok().unwrap();
        assert_eq!(parsed.progress, None);
        assert_eq!(parsed.status, Some(42));
        assert!(args(&["--bogus", "x"]).is_err());
    }
}
