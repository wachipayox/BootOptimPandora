use std::{borrow::Cow, collections::BTreeMap, ffi::{OsStr, OsString}, io::{Error, ErrorKind, PipeReader, PipeWriter}, path::{Path, PathBuf}, sync::Arc};

#[cfg(target_os = "macos")]
use crate::unix::unix_helpers::RawStringVec;
use crate::{process::PandoraProcess, spawner::SpawnType};

const BOOTOPTIM_PANDORA_UPSTREAM: &str = "4eb6c7849561151695288443c106519774ee05ea";


#[cfg(windows)]
const BOOTOPTIM_CREATE_NO_WINDOW: u32 = 0x08000000;

#[cfg(windows)]
fn configure_bootoptim_preflight(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(BOOTOPTIM_CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn configure_bootoptim_preflight(_command: &mut std::process::Command) {}

#[derive(Debug)]
struct BootOptimTraining {
    metadata: PathBuf,
    completion: PathBuf,
}

#[derive(Debug)]
pub struct PandoraCommand {
    pub(crate) executable: PandoraArg,
    pub(crate) args: Vec<PandoraArg>,
    pub(crate) inherit_env: Option<fn(&OsStr) -> bool>,
    pub(crate) env: BTreeMap<PandoraArg, PandoraArg>,
    pub(crate) current_dir: Option<PathBuf>,
    pub(crate) bootoptim_appcds_enabled: bool,
    appcds_identity_normal_gui: bool,
    pub(crate) stdin: PandoraStdioWriteMode,
    pub(crate) stdout: PandoraStdioReadMode,
    pub(crate) stderr: PandoraStdioReadMode,
    #[cfg(windows)]
    pub(crate) force_feedback: bool,
    #[cfg(unix)]
    pub(crate) pass_fds: Vec<std::os::fd::OwnedFd>,
    #[cfg(target_os = "macos")]
    pub(crate) sandbox_profile: Option<std::ffi::CString>,
    #[cfg(target_os = "macos")]
    pub(crate) sandbox_params: Option<RawStringVec>,
}

impl PandoraCommand {
    pub fn new(executable: impl Into<PandoraArg>) -> Self {
        let executable = executable.into();
        assert!(!executable.0.is_empty());
        Self {
            executable,
            args: Vec::new(),
            inherit_env: None,
            env: BTreeMap::default(),
            current_dir: None,
            bootoptim_appcds_enabled: true,
            appcds_identity_normal_gui: false,
            stdin: Default::default(),
            stdout: Default::default(),
            stderr: Default::default(),
            #[cfg(windows)]
            force_feedback: false,
            #[cfg(unix)]
            pass_fds: Default::default(),
            #[cfg(target_os = "macos")]
            sandbox_profile: None,
            #[cfg(target_os = "macos")]
            sandbox_params: None,
        }
    }

    pub fn arg(&mut self, arg: impl Into<PandoraArg>) {
        self.args.push(arg.into());
    }

    pub fn env(&mut self, k: impl Into<PandoraArg>, v: impl Into<PandoraArg>) {
        self.env.insert(k.into(), v.into());
    }

    pub fn current_dir(&mut self, current_dir: &Path) {
        self.current_dir = Some(current_dir.to_path_buf());
    }

    pub fn bootoptim_appcds_enabled(&mut self, enabled: bool) {
        self.bootoptim_appcds_enabled = enabled;
    }

    pub fn bootoptim_appcds_identity_normal_gui(&mut self, value: bool) {
        self.appcds_identity_normal_gui = value;
    }

    pub fn stdin(&mut self, stdin: PandoraStdioWriteMode) {
        self.stdin = stdin;
    }

    pub fn stdout(&mut self, stdout: PandoraStdioReadMode) {
        self.stdout = stdout;
    }

    pub fn stderr(&mut self, stderr: PandoraStdioReadMode) {
        self.stderr = stderr;
    }

    #[cfg(windows)]
    pub fn force_feedback(&mut self, force_feedback: bool) {
        self.force_feedback = force_feedback;
    }

    pub async fn spawn(mut self) -> std::io::Result<PandoraChild> {
        crate::spawner::probe_minecraft_command_ready(&self);
        let training = self.maybe_bootoptim_prepare();
        let result = crate::spawner::spawn(self, SpawnType::Normal)
            .await
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "spawning thread has shutdown"))
            .flatten();

        match result {
            Ok(mut child) => {
                if let Some(training) = training {
                    child.process.set_bootoptim_completion_marker(training.completion);
                }
                Ok(child)
            }
            Err(error) => {
                if let Some(training) = training {
                    let _ = std::fs::remove_file(training.metadata);
                    let _ = std::fs::remove_file(training.completion);
                }
                Err(error)
            }
        }
    }

    pub async fn spawn_elevated(self) -> std::io::Result<PandoraProcess> {
        crate::spawner::spawn(self, SpawnType::Elevated)
            .await
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "spawning thread has shutdown"))
            .flatten()
            .map(|child| child.process)
    }

    pub async fn spawn_sandboxed(self, sandbox: PandoraSandbox) -> std::io::Result<PandoraChild> {
        crate::spawner::probe_minecraft_command_ready(&self);
        crate::spawner::spawn(self, SpawnType::Sandboxed(sandbox))
            .await
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "spawning thread has shutdown"))
            .flatten()
    }

    fn maybe_bootoptim_prepare(&mut self) -> Option<BootOptimTraining> {
        if !self.bootoptim_appcds_enabled {
            log::info!("BOOTOPTIM_INTERPOSER status=disabled-per-instance activation=stock");
            return None;
        }
        let helper = std::env::var_os("BOOTOPTIM_LAUNCH_INTERPOSER")?;
        let helper = PathBuf::from(helper);
        if !helper.is_file() {
            return None;
        }
        let helper = helper.canonicalize().ok()?;
        let instance_dir = self.current_dir.clone()?;
        if !self.args.iter().any(|arg| arg.0 == OsStr::new("com.moulberry.pandora.LaunchWrapper")) {
            return None;
        }

        // v0 intentionally prepares only Pandora's direct Java executable. If a
        // user configured a wrapper command, or if sandbox/elevated launching is
        // used, we keep the stock path rather than guessing those semantics.
        if !is_java_executable(&self.executable.0) {
            return None;
        }

        let launcher_exe = std::env::current_exe().ok()?;
        let mut preflight = std::process::Command::new(&helper);
        preflight
            .arg("--instance-dir")
            .arg(&instance_dir)
            .arg("--launcher-exe")
            .arg(&launcher_exe)
            .arg("--upstream-commit")
            .arg(BOOTOPTIM_PANDORA_UPSTREAM)
            .arg("--appcds-identity-authority")
            .arg(if self.appcds_identity_normal_gui { "normal-gui" } else { "unknown" })
            .arg("--")
            .arg(&self.executable.0);
        for arg in &self.args {
            preflight.arg(&arg.0);
        }
        preflight.current_dir(&instance_dir);
        preflight.stdin(std::process::Stdio::null());
        preflight.stdout(std::process::Stdio::piped());
        preflight.stderr(std::process::Stdio::piped());
        configure_bootoptim_preflight(&mut preflight);

        // Give the preflight helper the same effective environment that the Java
        // process will receive, so hidden JVM option variables are evaluated
        // against the actual launch tuple rather than Pandora's ambient process.
        preflight.env_clear();
        for (k, v) in std::env::vars_os() {
            let key: PandoraArg = k.clone().into();
            if self.env.contains_key(&key) {
                continue;
            }
            if let Some(inherit_env) = self.inherit_env && !(inherit_env)(k.as_os_str()) {
                continue;
            }
            preflight.env(&k, &v);
        }
        for (k, v) in &self.env {
            preflight.env(&k.0, &v.0);
        }
        // This is launcher control state, not a JVM identity input. Ensure the
        // helper sees it even if a caller uses a restrictive Java env filter.
        for key in [
            "BOOTOPTIM_APPCDS_MODE",
            "BOOTOPTIM_APPCDS_IDENTITY_CACHE",
            "BOOTOPTIM_APPCDS_IDENTITY_FORCE_STOCK",
            "BOOTOPTIM_APPCDS_IDENTITY_DIAGNOSTICS",
        ] {
            if let Some(value) = std::env::var_os(key) {
                preflight.env(key, value);
            }
        }

        let probe_preflight = crate::spawner::is_probe_minecraft_launch(self);
        if probe_preflight {
            crate::spawner::probe_event("appcds_preflight", "begin", None);
        }
        let output = match preflight.output() {
            Ok(output) => output,
            Err(error) => {
                if probe_preflight {
                    crate::spawner::probe_event("appcds_preflight", "end", Some("error"));
                }
                log::warn!("BOOTOPTIM_INTERPOSER status=helper-spawn-error activation=stock error={error}");
                return None;
            }
        };
        if probe_preflight {
            crate::spawner::probe_event(
                "appcds_preflight",
                "end",
                Some(if output.status.success() { "ok" } else { "error" }),
            );
        }
        if !output.status.success() {
            log::warn!(
                "BOOTOPTIM_INTERPOSER status=helper-error activation=stock exit_code={:?} stderr_bytes={}",
                output.status.code(),
                output.stderr.len()
            );
            return None;
        }
        if std::env::var_os("BOOTOPTIM_APPCDS_IDENTITY_DIAGNOSTICS").is_some_and(|value| value == "1") {
            if let Some(line) = bootoptim_identity_diagnostic_line(&output.stderr) {
                log::info!("{line}");
            }
        }
        let decision = String::from_utf8_lossy(&output.stdout);
        match decision.trim() {
            "READY" => {
                let ready = instance_dir.join(".bootoptim").join("appcds").join("ready.jsa");
                self.prepend_bootoptim_flags(vec![
                    OsString::from("-Xshare:auto"),
                    bootoptim_os_flag("-XX:SharedArchiveFile=", &ready),
                ]);
                log::info!("BOOTOPTIM_INTERPOSER status=ready activation=enabled");
                None
            }
            "TRAIN" => {
                let cache_dir = instance_dir.join(".bootoptim").join("appcds");
                let training = cache_dir.join("training.jsa");
                let completion = cache_dir.join("training.complete");
                let _ = std::fs::remove_file(&completion);
                self.prepend_bootoptim_flags(vec![
                    OsString::from("-Xshare:auto"),
                    bootoptim_os_flag("-XX:ArchiveClassesAtExit=", &training),
                ]);
                log::info!("BOOTOPTIM_INTERPOSER status=generating activation=training");
                Some(BootOptimTraining {
                    metadata: cache_dir.join("training.meta"),
                    completion,
                })
            }
            "STOCK" => None,
            _ => {
                log::warn!(
                    "BOOTOPTIM_INTERPOSER status=invalid-helper-decision activation=stock stdout_bytes={}",
                    output.stdout.len()
                );
                None
            }
        }
    }

    fn prepend_bootoptim_flags(&mut self, flags: Vec<OsString>) {
        let mut args = Vec::with_capacity(flags.len() + self.args.len());
        args.extend(flags.into_iter().map(PandoraArg::from));
        args.append(&mut self.args);
        self.args = args;
    }

    pub(crate) fn resolve_executable_path(&self) -> std::io::Result<PathBuf> {
        let path = Path::new(&self.executable.0);
        let path = if path.components().count() > 1 {
            let Ok(path) = path.canonicalize() else {
                return Err(Error::new(ErrorKind::NotFound, "executable file doesn't exist"));
            };
            path
        } else if let Some(path) = crate::path_cache::get_command_path(&self.executable.0) {
            path.to_path_buf()
        } else {
            return Err(Error::new(ErrorKind::NotFound, "unable to resolve executable"));
        };

        debug_assert!(path.is_absolute());

        #[cfg(windows)]
        {
            // Try to remove the \\?\ verbatim path prefix since it can break some applications
            let encoded_bytes = path.as_os_str().as_encoded_bytes();
            if let Some(rest) = encoded_bytes.strip_prefix(b"\\\\?\\") {
                return Ok(PathBuf::from(unsafe { OsStr::from_encoded_bytes_unchecked(&rest) }));
            }
        }

        Ok(path)
    }

    pub(crate) fn take_final_env(&mut self) -> BTreeMap<PandoraArg, PandoraArg> {
        if let Some(inherit_env) = self.inherit_env {
            for (k, v) in std::env::vars_os() {
                let k: PandoraArg = k.into();
                if self.env.contains_key(&k) {
                    continue;
                }
                if !(inherit_env)(&k.0) {
                    continue;
                }
                self.env.insert(k, v.into());
            }
        } else {
            for (k, v) in std::env::vars_os() {
                let k: PandoraArg = k.into();
                if self.env.contains_key(&k) {
                    continue;
                }
                self.env.insert(k, v.into());
            }
        }
        std::mem::take(&mut self.env)
    }
}

fn is_java_executable(value: &OsStr) -> bool {
    let path = Path::new(value);
    let Some(name) = path.file_name().map(|v| v.to_string_lossy().to_ascii_lowercase()) else {
        return false;
    };
    if name != "java" && name != "java.exe" && name != "javaw.exe" {
        return false;
    }
    let Some(bin) = path.parent() else {
        return false;
    };
    if !bin.file_name().map(|v| v.to_string_lossy().eq_ignore_ascii_case("bin")).unwrap_or(false) {
        return false;
    }
    let Some(root) = bin.parent() else {
        return false;
    };
    root.join("lib").is_dir()
}

fn bootoptim_os_flag(prefix: &str, path: &Path) -> OsString {
    let mut out = OsString::from(prefix);
    out.push(path.as_os_str());
    out
}

fn bootoptim_identity_diagnostic_line(stderr: &[u8]) -> Option<String> {
    String::from_utf8_lossy(stderr)
        .lines()
        .find(|line| line.starts_with("BOOTOPTIM_APPCDS_IDENTITY_DIAG "))
        .map(str::to_owned)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PandoraStdioReadMode {
    Null,
    #[default]
    Inherit,
    Pipe,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PandoraStdioWriteMode {
    #[default]
    Null,
    Inherit,
    Pipe,
}

#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct PandoraArg(pub(crate) Cow<'static, OsStr>);

impl From<&'static str> for PandoraArg {
    fn from(value: &'static str) -> Self {
        PandoraArg(Cow::Borrowed(OsStr::new(value)))
    }
}

impl From<&'static OsStr> for PandoraArg {
    fn from(value: &'static OsStr) -> Self {
        PandoraArg(Cow::Borrowed(value))
    }
}

impl From<OsString> for PandoraArg {
    fn from(value: OsString) -> Self {
        PandoraArg(Cow::Owned(value))
    }
}

impl From<String> for PandoraArg {
    fn from(value: String) -> Self {
        PandoraArg(Cow::Owned(value.into()))
    }
}

impl From<PathBuf> for PandoraArg {
    fn from(value: PathBuf) -> Self {
        PandoraArg(Cow::Owned(value.into_os_string()))
    }
}

pub struct PandoraSandbox {
    pub allow_read: Vec<Arc<Path>>,
    pub allow_write: Vec<Arc<Path>>,
    pub is_jvm: bool,

    pub grant_network_access: bool,

    #[cfg(target_os = "linux")]
    pub sandbox_dir: Arc<Path>,
    #[cfg(windows)]
    pub name: Arc<OsStr>,
    #[cfg(windows)]
    pub description: Arc<OsStr>,
    #[cfg(windows)]
    pub self_elevate_for_acl_arg: Option<PandoraArg>,
    #[cfg(windows)]
    pub grant_winsta_writeattributes: bool,
}

#[derive(Debug)]
pub struct PandoraChild {
    pub process: PandoraProcess,
    pub stdin: Option<PipeWriter>,
    pub stdout: Option<PipeReader>,
    pub stderr: Option<PipeReader>,
}


#[cfg(all(test, windows))]
mod bootoptim_windows_preflight_tests {
    use super::*;

    #[test]
    fn create_no_window_preserves_redirected_stdout_and_stderr() {
        let mut command = std::process::Command::new("cmd.exe");
        command.args([
            "/D",
            "/S",
            "/C",
            "echo READY & echo helper-diagnostic 1>&2 & exit /b 7",
        ]);
        command.stdin(std::process::Stdio::null());
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::piped());
        configure_bootoptim_preflight(&mut command);

        let output = command.output().expect("hidden child must spawn");
        assert_eq!(output.status.code(), Some(7));
        assert!(String::from_utf8_lossy(&output.stdout).contains("READY"));
        assert!(String::from_utf8_lossy(&output.stderr).contains("helper-diagnostic"));
    }
}


#[cfg(test)]
mod appcds_identity_authority_tests {
    use super::*;

    #[test]
    fn command_defaults_to_unknown_and_requires_explicit_normal_gui_authority() {
        let mut command = PandoraCommand::new("java");
        assert!(!command.appcds_identity_normal_gui);
        command.bootoptim_appcds_identity_normal_gui(true);
        assert!(command.appcds_identity_normal_gui);
    }

    #[test]
    fn identity_diagnostic_forwarding_is_single_line_and_ignores_other_helper_stderr() {
        let stderr = b"BOOTOPTIM_INTERPOSER status=ready activation=enabled\nBOOTOPTIM_APPCDS_IDENTITY_DIAG requested=true authority=accepted authority_reason=normal-gui manifest=absent eligible=42 reused=0 strong=42 unverifiable=none publication=published\nBOOTOPTIM_APPCDS_IDENTITY_DIAG duplicate\n";
        assert_eq!(
            bootoptim_identity_diagnostic_line(stderr).as_deref(),
            Some("BOOTOPTIM_APPCDS_IDENTITY_DIAG requested=true authority=accepted authority_reason=normal-gui manifest=absent eligible=42 reused=0 strong=42 unverifiable=none publication=published")
        );
    }
}


#[cfg(test)]
mod bootoptim_appcds_instance_toggle_tests {
    use super::*;

    #[test]
    fn disabled_instance_never_enters_preflight_or_train_ready_decision() {
        let mut command = PandoraCommand::new(if cfg!(windows) { "javaw.exe" } else { "java" });
        command.bootoptim_appcds_enabled(false);
        command.arg("com.moulberry.pandora.LaunchWrapper");
        command.current_dir(Path::new("."));
        assert!(command.maybe_bootoptim_prepare().is_none());
        assert!(command.args.iter().all(|arg| {
            let text = arg.0.to_string_lossy();
            !text.starts_with("-XX:SharedArchiveFile=") && !text.starts_with("-XX:ArchiveClassesAtExit=")
        }));
    }

    #[test]
    fn appcds_preference_defaults_enabled_for_legacy_callers() {
        let command = PandoraCommand::new(if cfg!(windows) { "javaw.exe" } else { "java" });
        assert!(command.bootoptim_appcds_enabled);
    }

    #[test]
    fn appcds_preference_can_be_reenabled_without_mutating_command_args() {
        let mut command = PandoraCommand::new(if cfg!(windows) { "javaw.exe" } else { "java" });
        command.arg("-Xmx4G");
        let original = command.args.iter().map(|arg| arg.0.clone()).collect::<Vec<_>>();
        command.bootoptim_appcds_enabled(false);
        assert!(!command.bootoptim_appcds_enabled);
        command.bootoptim_appcds_enabled(true);
        assert!(command.bootoptim_appcds_enabled);
        assert_eq!(command.args.iter().map(|arg| arg.0.clone()).collect::<Vec<_>>(), original);
    }
}
