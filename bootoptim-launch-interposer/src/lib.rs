use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const HELPER_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const PANDORA_MAIN: &str = "com.moulberry.pandora.LaunchWrapper";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Plan,
    Train,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum CacheState {
    Absent,
    Generating,
    Ready,
    Stale,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileIdentity {
    pub path: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JavaIdentity {
    pub executable_path: String,
    pub executable_sha256: String,
    pub release_sha256: String,
    pub vendor: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchPlan {
    pub schema: u32,
    pub helper_version: String,
    pub helper_sha256: String,
    pub os: String,
    pub arch: String,
    pub java: JavaIdentity,
    // Order is semantically significant. Never sort this vector.
    pub classpath: Vec<FileIdentity>,
    // Ordered SHA-256 values of JVM arguments other than the classpath pair.
    // Raw values are deliberately not persisted because they can contain local data.
    pub jvm_arg_sha256: Vec<String>,
    pub main_class: String,
    // Mods are identity inputs, not a launch-order source. Sorting here only makes
    // the manifest canonical and is never fed back into Pandora/NeoForge.
    pub mods: Vec<FileIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadyMetadata {
    pub schema: u32,
    pub plan_sha256: String,
    pub archive_sha256: String,
    pub helper_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateMetadata {
    pub schema: u32,
    pub state: CacheState,
    pub plan_sha256: String,
}

pub struct CacheLock {
    file: File,
}

impl CacheLock {
    pub fn try_acquire(cache: &Path) -> io::Result<Option<Self>> {
        fs::create_dir_all(cache)?;
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(cache.join("cache.lock"))?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(Self { file })),
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl Drop for CacheLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

pub fn cache_dir(game_dir: &Path) -> PathBuf {
    game_dir.join(".bootoptim").join("appcds")
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 128 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn file_identity(path: &Path, display_path: String) -> io::Result<FileIdentity> {
    let meta = fs::metadata(path)?;
    Ok(FileIdentity {
        path: display_path,
        sha256: sha256_file(path)?,
        size: meta.len(),
    })
}

fn parse_release(path: &Path) -> io::Result<(String, String, String)> {
    let bytes = fs::read(path)?;
    let release_sha256 = sha256_bytes(&bytes);
    let text = String::from_utf8_lossy(&bytes);
    let mut vendor = String::new();
    let mut version = String::new();
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("IMPLEMENTOR=") {
            vendor = v.trim_matches('"').to_owned();
        } else if let Some(v) = line.strip_prefix("JAVA_VERSION=") {
            version = v.trim_matches('"').to_owned();
        }
    }
    Ok((release_sha256, vendor, version))
}

fn java_identity(java: &Path) -> io::Result<JavaIdentity> {
    let canonical = java.canonicalize()?;
    let java_home = canonical
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "java path has no JDK root"))?;
    let (release_sha256, vendor, version) = parse_release(&java_home.join("release"))?;
    Ok(JavaIdentity {
        executable_path: canonical.to_string_lossy().into_owned(),
        executable_sha256: sha256_file(&canonical)?,
        release_sha256,
        vendor,
        version,
    })
}

pub fn split_classpath(raw: &OsStr, separator: char) -> Vec<PathBuf> {
    raw.to_string_lossy()
        .split(separator)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect()
}

fn collect_mods(game_dir: &Path) -> io::Result<Vec<FileIdentity>> {
    let mods_dir = game_dir.join("mods");
    if !mods_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut paths = Vec::new();
    collect_jar_paths(&mods_dir, &mods_dir, &mut paths)?;
    paths.sort_by(|a, b| a.0.cmp(&b.0));
    paths
        .into_iter()
        .map(|(relative, absolute)| file_identity(&absolute, relative))
        .collect()
}

fn collect_jar_paths(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_jar_paths(root, &path, out)?;
        } else if path
            .extension()
            .and_then(OsStr::to_str)
            .is_some_and(|ext| ext.eq_ignore_ascii_case("jar"))
        {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.push((relative, path));
        }
    }
    Ok(())
}

pub fn build_plan(
    java: &Path,
    java_args: &[OsString],
    game_dir: &Path,
    helper_exe: &Path,
    classpath_separator: char,
) -> io::Result<LaunchPlan> {
    let main_index = java_args
        .iter()
        .position(|arg| arg == PANDORA_MAIN)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Pandora LaunchWrapper main class not found"))?;

    let mut cp_value: Option<&OsStr> = None;
    let mut jvm_hashes = Vec::new();
    let mut i = 0usize;
    while i < main_index {
        let arg = &java_args[i];
        let arg_text = arg.to_string_lossy();
        if matches!(arg_text.as_ref(), "-cp" | "-classpath" | "--class-path") {
            if cp_value.is_some() || i + 1 >= main_index {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "ambiguous classpath"));
            }
            cp_value = Some(java_args[i + 1].as_os_str());
            i += 2;
            continue;
        }
        jvm_hashes.push(sha256_bytes(arg_text.as_bytes()));
        i += 1;
    }

    let cp_value = cp_value.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "classpath not found"))?;
    let classpath_paths = split_classpath(cp_value, classpath_separator);
    let mut classpath = Vec::with_capacity(classpath_paths.len());
    for path in classpath_paths {
        // Preserve the exact classpath string/order as supplied by Pandora.
        let display = path.to_string_lossy().into_owned();
        classpath.push(file_identity(&path, display)?);
    }

    Ok(LaunchPlan {
        schema: 1,
        helper_version: HELPER_VERSION.to_owned(),
        helper_sha256: sha256_file(helper_exe)?,
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        java: java_identity(java)?,
        classpath,
        jvm_arg_sha256: jvm_hashes,
        main_class: PANDORA_MAIN.to_owned(),
        mods: collect_mods(game_dir)?,
    })
}

pub fn canonical_plan_bytes(plan: &LaunchPlan) -> io::Result<Vec<u8>> {
    serde_json::to_vec(plan).map_err(io::Error::other)
}

pub fn plan_sha256(plan: &LaunchPlan) -> io::Result<String> {
    Ok(sha256_bytes(&canonical_plan_bytes(plan)?))
}

pub fn cds_compatible(java_args: &[OsString]) -> bool {
    let main = java_args.iter().position(|arg| arg == PANDORA_MAIN).unwrap_or(java_args.len());
    java_args[..main].iter().all(|arg| {
        let s = arg.to_string_lossy();
        !s.starts_with("-javaagent")
            && !s.starts_with("-agentlib")
            && !s.starts_with("-agentpath")
            && s != "-XX:+AllowArchivingWithJavaAgent"
            && !s.starts_with("-XX:ArchiveClassesAtExit")
            && !s.starts_with("-XX:SharedArchiveFile")
            && !s.starts_with("-Xshare:")
    })
}

pub fn ready_archive(cache: &Path) -> PathBuf {
    cache.join("appcds-ready.jsa")
}

pub fn ready_metadata(cache: &Path) -> PathBuf {
    cache.join("ready.json")
}

pub fn classify(cache: &Path, plan_hash: &str) -> io::Result<CacheState> {
    let archive = ready_archive(cache);
    let metadata_path = ready_metadata(cache);
    if !archive.exists() && !metadata_path.exists() {
        return Ok(CacheState::Absent);
    }
    if !archive.is_file() || !metadata_path.is_file() {
        return Ok(CacheState::Stale);
    }
    let metadata: ReadyMetadata = match serde_json::from_slice(&fs::read(&metadata_path)?) {
        Ok(value) => value,
        Err(_) => return Ok(CacheState::Stale),
    };
    if metadata.schema != 1
        || metadata.helper_version != HELPER_VERSION
        || metadata.plan_sha256 != plan_hash
        || metadata.archive_sha256 != sha256_file(&archive)?
    {
        return Ok(CacheState::Stale);
    }
    Ok(CacheState::Ready)
}

fn unique_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{}-{nanos}", std::process::id())
}

pub fn staging_archive(cache: &Path) -> PathBuf {
    cache.join(format!("staging-{}.jsa", unique_suffix()))
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no parent"))?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(".tmp-{}", unique_suffix()));
    {
        let mut file = OpenOptions::new().create_new(true).write(true).open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(&temp, path)?;
    Ok(())
}

pub fn persist_plan(cache: &Path, plan: &LaunchPlan) -> io::Result<Vec<u8>> {
    let bytes = canonical_plan_bytes(plan)?;
    atomic_write(&cache.join("launch-plan.json"), &bytes)?;
    Ok(bytes)
}

pub fn write_state(cache: &Path, state: CacheState, plan_hash: &str) -> io::Result<()> {
    let bytes = serde_json::to_vec(&StateMetadata {
        schema: 1,
        state,
        plan_sha256: plan_hash.to_owned(),
    })
    .map_err(io::Error::other)?;
    atomic_write(&cache.join("state.json"), &bytes)
}

pub fn promote(cache: &Path, staging: &Path, plan_hash: &str) -> io::Result<()> {
    let meta = fs::metadata(staging)?;
    if !meta.is_file() || meta.len() == 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "empty AppCDS staging archive"));
    }
    let archive_hash = sha256_file(staging)?;
    let ready = ready_archive(cache);
    if ready.exists() {
        fs::remove_file(&ready)?;
    }
    fs::rename(staging, &ready)?;
    let metadata = ReadyMetadata {
        schema: 1,
        plan_sha256: plan_hash.to_owned(),
        archive_sha256: archive_hash,
        helper_version: HELPER_VERSION.to_owned(),
    };
    atomic_write(
        &ready_metadata(cache),
        &serde_json::to_vec(&metadata).map_err(io::Error::other)?,
    )
}

pub fn safe_status(state: CacheState) -> String {
    format!("BootOptim AppCDS state={state:?}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fixture_plan(root: &Path, java_path: &str, cp_hash: &str) -> LaunchPlan {
        LaunchPlan {
            schema: 1,
            helper_version: HELPER_VERSION.into(),
            helper_sha256: "11".repeat(32),
            os: "windows".into(),
            arch: "x86_64".into(),
            java: JavaIdentity {
                executable_path: java_path.into(),
                executable_sha256: "22".repeat(32),
                release_sha256: "33".repeat(32),
                vendor: "Vendor".into(),
                version: "25.0.4".into(),
            },
            classpath: vec![FileIdentity {
                path: root.join("lib with space.jar").to_string_lossy().into_owned(),
                sha256: cp_hash.into(),
                size: 3,
            }],
            jvm_arg_sha256: vec![sha256_bytes(b"-Xmx6G")],
            main_class: PANDORA_MAIN.into(),
            mods: vec![],
        }
    }

    #[test]
    fn windows_classpath_preserves_unicode_spaces_and_order() {
        let raw = OsString::from(r"C:\Juego con espacio\á.jar;D:\mods\β.jar;E:\z.jar");
        let split = split_classpath(&raw, ';');
        assert_eq!(split.len(), 3);
        assert_eq!(split[0], PathBuf::from(r"C:\Juego con espacio\á.jar"));
        assert_eq!(split[1], PathBuf::from(r"D:\mods\β.jar"));
        assert_eq!(split[2], PathBuf::from(r"E:\z.jar"));
    }

    #[test]
    fn identical_tuple_has_byte_identical_plan() {
        let tmp = TempDir::new().unwrap();
        let a = fixture_plan(tmp.path(), r"C:\Java 25\bin\javaw.exe", &"44".repeat(32));
        let b = a.clone();
        assert_eq!(canonical_plan_bytes(&a).unwrap(), canonical_plan_bytes(&b).unwrap());
        assert_eq!(plan_sha256(&a).unwrap(), plan_sha256(&b).unwrap());
    }

    #[test]
    fn jar_or_java_path_change_makes_tuple_stale() {
        let tmp = TempDir::new().unwrap();
        let cache = tmp.path();
        fs::write(ready_archive(cache), b"ready archive").unwrap();
        let original = fixture_plan(cache, r"C:\Java\bin\java.exe", &"44".repeat(32));
        let original_hash = plan_sha256(&original).unwrap();
        let metadata = ReadyMetadata {
            schema: 1,
            plan_sha256: original_hash.clone(),
            archive_sha256: sha256_file(&ready_archive(cache)).unwrap(),
            helper_version: HELPER_VERSION.into(),
        };
        fs::write(ready_metadata(cache), serde_json::to_vec(&metadata).unwrap()).unwrap();
        assert_eq!(classify(cache, &original_hash).unwrap(), CacheState::Ready);

        let changed_jar = fixture_plan(cache, r"C:\Java\bin\java.exe", &"55".repeat(32));
        assert_eq!(classify(cache, &plan_sha256(&changed_jar).unwrap()).unwrap(), CacheState::Stale);
        let changed_java = fixture_plan(cache, r"D:\OtherJava\bin\java.exe", &"44".repeat(32));
        assert_eq!(classify(cache, &plan_sha256(&changed_java).unwrap()).unwrap(), CacheState::Stale);
    }

    #[test]
    fn incomplete_staging_is_never_ready() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("staging-crashed.jsa"), b"partial").unwrap();
        assert_ne!(classify(tmp.path(), "deadbeef").unwrap(), CacheState::Ready);
    }

    #[test]
    fn lock_is_cross_handle_exclusive() {
        let tmp = TempDir::new().unwrap();
        let first = CacheLock::try_acquire(tmp.path()).unwrap().unwrap();
        assert!(CacheLock::try_acquire(tmp.path()).unwrap().is_none());
        drop(first);
        assert!(CacheLock::try_acquire(tmp.path()).unwrap().is_some());
    }

    #[test]
    fn no_sensitive_game_argument_is_serialized_or_logged() {
        let tmp = TempDir::new().unwrap();
        let plan = fixture_plan(tmp.path(), r"C:\Java\bin\java.exe", &"44".repeat(32));
        let bytes = canonical_plan_bytes(&plan).unwrap();
        let secret = "accessToken-super-secret-value";
        assert!(!String::from_utf8_lossy(&bytes).contains(secret));
        assert!(!safe_status(CacheState::Ready).contains(secret));
    }

    #[test]
    fn incompatible_existing_cds_or_agent_flags_fail_closed() {
        let base = vec![OsString::from("-Xmx6G"), OsString::from(PANDORA_MAIN)];
        assert!(cds_compatible(&base));
        for flag in [
            "-javaagent:x.jar",
            "-agentlib:jdwp",
            "-XX:+AllowArchivingWithJavaAgent",
            "-XX:SharedArchiveFile=x.jsa",
            "-XX:ArchiveClassesAtExit=x.jsa",
            "-Xshare:on",
        ] {
            let args = vec![OsString::from(flag), OsString::from(PANDORA_MAIN)];
            assert!(!cds_compatible(&args), "{flag}");
        }
    }
}
