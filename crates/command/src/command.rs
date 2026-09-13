use std::{borrow::Cow, collections::BTreeMap, ffi::{OsStr, OsString}, io::{Error, ErrorKind, PipeReader, PipeWriter}, path::{Path, PathBuf}, sync::Arc};

#[cfg(target_os = "macos")]
use crate::unix::unix_helpers::RawStringVec;
use crate::{process::PandoraProcess, spawner::SpawnType};

const BOOTOPTIM_PANDORA_UPSTREAM: &str = "4eb6c7849561151695288443c106519774ee05ea";

#[derive(Debug)]
pub struct PandoraCommand {
    pub(crate) executable: PandoraArg,
    pub(crate) args: Vec<PandoraArg>,
    pub(crate) inherit_env: Option<fn(&OsStr) -> bool>,
    pub(crate) env: BTreeMap<PandoraArg, PandoraArg>,
    pub(crate) current_dir: Option<PathBuf>,
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
        let training_marker = self.maybe_bootoptim_prepare();
        let result = crate::spawner::spawn(self, SpawnType::Normal)
            .await
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "spawning thread has shutdown"))
            .flatten();
        if result.is_err() {
            if let Some(path) = training_marker {
                let _ = std::fs::remove_file(path);
            }
        }
        result
    }

    pub async fn spawn_elevated(self) -> std::io::Result<PandoraProcess> {
        crate::spawner::spawn(self, SpawnType::Elevated)
            .await
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "spawning thread has shutdown"))
            .flatten()
            .map(|child| child.process)
    }

    pub async fn spawn_sandboxed(self, sandbox: PandoraSandbox) -> std::io::Result<PandoraChild> {
        crate::spawner::spawn(self, SpawnType::Sandboxed(sandbox))
            .await
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "spawning thread has shutdown"))
            .flatten()
    }

    fn maybe_bootoptim_prepare(&mut self) -> Option<PathBuf> {
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
            .arg("--")
            .arg(&self.executable.0);
        for arg in &self.args {
            preflight.arg(&arg.0);
        }
        preflight.current_dir(&instance_dir);
        preflight.stdin(std::process::Stdio::null());
        preflight.stdout(std::process::Stdio::piped());
        preflight.stderr(std::process::Stdio::null());

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
        if let Some(mode) = std::env::var_os("BOOTOPTIM_APPCDS_MODE") {
            preflight.env("BOOTOPTIM_APPCDS_MODE", mode);
        }

        let output = preflight.output().ok()?;
        if !output.status.success() {
            return None;
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
                self.prepend_bootoptim_flags(vec![
                    OsString::from("-Xshare:auto"),
                    bootoptim_os_flag("-XX:ArchiveClassesAtExit=", &training),
                ]);
                log::info!("BOOTOPTIM_INTERPOSER status=generating activation=training");
                Some(cache_dir.join("training.meta"))
            }
            _ => None,
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
