//! Optional zipfs read-only Java agent for Wachiland profiles on Windows PCs.
//!
//! The agent (BootOptim's `tools/zipfs-readonly-agent`) makes the JDK open plain `.jar` files under `mods/` and
//! `libraries/` read-only, which avoids a per-open volume lookup in SecureJarHandler. It is passed as
//! `-javaagent` only when the game runs on the Java 25 path that [`super::java_default`] selected, so the JVM is
//! known to accept it.
//!
//! The JAR is embedded in the launcher (`zipfs-agent/zipfs-readonly-agent.jar`, with its source commit and
//! SHA-256 in `zipfs-readonly-agent.jar.source`). At launch it is written to `agents/` when that file is missing
//! or differs, through a temporary file and a rename, so the version always matches the launcher.
//!
//! The agent writes its own ACTIVE and FALLBACK lines to the status file given by
//! `-Dwbombo.zipfs.statusFile`, because the launcher does not keep the game's stderr on a normal start. This
//! module logs its decision to `launcher.log`.

use std::{
    io,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

/// Mechanism id in the `[WBOMBO-COMPAT]` log lines.
pub(crate) const MECHANISM: &str = "zipfs-readonly-agent";
/// File name of the agent JAR inside the launcher's `agents/` folder.
pub(crate) const AGENT_JAR_NAME: &str = "zipfs-readonly-agent.jar";
/// File name of the agent status file inside the game's `logs/` folder.
pub(crate) const STATUS_FILE_NAME: &str = "wbombo-zipfs-agent.log";
/// Environment switch: `0`, `off`, `false` or `no` disables the agent for this launcher session.
pub(crate) const DISABLE_ENV: &str = "WBOMBO_ZIPFS_AGENT";
/// Minimum Java major for the agent (it is compiled for the Java 25 class-file API).
pub(crate) const MIN_JAVA_MAJOR: u32 = 25;

/// The embedded agent JAR. Its reference (source commit and SHA-256) is in `zipfs-readonly-agent.jar.source`.
const EMBEDDED_JAR: &[u8] = include_bytes!("zipfs-agent/zipfs-readonly-agent.jar");
const SOURCE_REFERENCE: &str = include_str!("zipfs-agent/zipfs-readonly-agent.jar.source");

/// Why the agent is or is not used. The reasons are stable strings for the log.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Decision {
    Use(Vec<String>),
    Skip(&'static str),
}

pub(crate) struct Inputs<'a> {
    pub windows: bool,
    pub java25: bool,
    pub java_major: u32,
    pub agent_jar: &'a Path,
    pub status_file: &'a Path,
    pub disable_value: Option<&'a str>,
}

/// Pure decision on the preconditions: no filesystem access and no logging.
pub(crate) fn decide(inputs: &Inputs<'_>) -> Decision {
    if !inputs.windows {
        return Decision::Skip("not-windows");
    }
    if !inputs.java25 {
        return Decision::Skip("not-java25");
    }
    if inputs.java_major < MIN_JAVA_MAJOR {
        return Decision::Skip("java-major-below-25");
    }
    if disabled(inputs.disable_value) {
        return Decision::Skip("disabled-by-env");
    }
    Decision::Use(vec![
        format!("-javaagent:{}", jvm_path(inputs.agent_jar)),
        format!("-Dwbombo.zipfs.statusFile={}", jvm_path(inputs.status_file)),
    ])
}

/// JVM arguments for this launch, after making sure the embedded JAR is in `agents_dir`. Logs the decision.
pub(crate) fn jvm_args(
    java25: bool,
    java_major: u32,
    agents_dir: &Path,
    status_file: &Path,
    disable_value: Option<&str>,
) -> Vec<String> {
    let agent_jar = agents_dir.join(AGENT_JAR_NAME);
    let inputs = Inputs {
        windows: cfg!(windows),
        java25,
        java_major,
        agent_jar: &agent_jar,
        status_file,
        disable_value,
    };
    match decide(&inputs) {
        Decision::Skip(reason) => {
            log::info!("[WBOMBO-COMPAT] SKIPPED project=Pandora mechanism={MECHANISM} reason={reason}");
            Vec::new()
        },
        Decision::Use(args) => match ensure_agent_jar(agents_dir) {
            Ok(path) => {
                if let Some(logs_dir) = status_file.parent() {
                    let _ = std::fs::create_dir_all(logs_dir);
                }
                log::info!(
                    "[WBOMBO-COMPAT] ACTIVE project=Pandora mechanism={MECHANISM} jar={} source={}",
                    jvm_path(&path),
                    source_commit()
                );
                args
            },
            Err(error) => {
                log::warn!(
                    "[WBOMBO-COMPAT] FALLBACK project=Pandora mechanism={MECHANISM} reason=jar-write-failed error={error}"
                );
                Vec::new()
            },
        },
    }
}

/// Writes the embedded JAR to `agents_dir` unless the file there already has the embedded SHA-256.
/// Writes through a temporary file in the same folder and a rename, so a reader never sees a partial JAR.
pub(crate) fn ensure_agent_jar(agents_dir: &Path) -> io::Result<PathBuf> {
    let target = agents_dir.join(AGENT_JAR_NAME);
    if file_sha256(&target).is_some_and(|hash| hash == expected_sha256()) {
        return Ok(target);
    }
    std::fs::create_dir_all(agents_dir)?;
    let temporary = agents_dir.join(format!("{AGENT_JAR_NAME}.{}.tmp", std::process::id()));
    std::fs::write(&temporary, EMBEDDED_JAR)?;
    if let Err(error) = std::fs::rename(&temporary, &target) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(target)
}

/// SHA-256 of the embedded JAR, as written in the reference file.
pub(crate) fn expected_sha256() -> &'static str {
    SOURCE_REFERENCE
        .lines()
        .find_map(|line| line.strip_prefix("sha256="))
        .map(str::trim)
        .unwrap_or("")
}

/// Source commit recorded in the reference file.
pub(crate) fn source_commit() -> &'static str {
    SOURCE_REFERENCE
        .lines()
        .find_map(|line| line.strip_prefix("source_commit="))
        .map(str::trim)
        .unwrap_or("")
}

#[cfg(test)]
pub(crate) fn embedded_sha256() -> String {
    hex_digest(EMBEDDED_JAR)
}

fn file_sha256(path: &Path) -> Option<String> {
    std::fs::read(path).ok().map(|bytes| hex_digest(&bytes))
}

fn hex_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn disabled(value: Option<&str>) -> bool {
    matches!(
        value.map(|v| v.trim().to_ascii_lowercase()).as_deref(),
        Some("0" | "off" | "false" | "no")
    )
}

/// JVM options take forward slashes; the argument is passed as one element, so spaces are safe.
fn jvm_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Decision, Inputs, decide, embedded_sha256, expected_sha256, source_commit};

    const JAR: &str = "C:\\Users\\Pat\\AppData\\Roaming\\WachilandLauncher\\agents\\zipfs-readonly-agent.jar";
    const STATUS: &str =
        "C:\\Users\\Pat\\AppData\\Roaming\\WachilandLauncher\\instances\\X\\.minecraft\\logs\\wbombo-zipfs-agent.log";

    fn inputs<'a>(jar: &'a Path, status: &'a Path) -> Inputs<'a> {
        Inputs {
            windows: true,
            java25: true,
            java_major: 25,
            agent_jar: jar,
            status_file: status,
            disable_value: None,
        }
    }

    #[test]
    fn embedded_jar_matches_the_reference_sha256() {
        // The reference file names the JAR the launcher embeds; a rebuild must update both together.
        assert_eq!(embedded_sha256(), expected_sha256());
        assert_eq!(expected_sha256().len(), 64);
        assert_eq!(source_commit().len(), 40);
    }

    #[test]
    fn uses_agent_with_forward_slashes_when_everything_holds() {
        let (jar, status) = (Path::new(JAR), Path::new(STATUS));
        let Decision::Use(args) = decide(&inputs(jar, status)) else {
            panic!("expected Use");
        };
        assert_eq!(
            args,
            vec![
                "-javaagent:C:/Users/Pat/AppData/Roaming/WachilandLauncher/agents/zipfs-readonly-agent.jar".to_string(),
                "-Dwbombo.zipfs.statusFile=C:/Users/Pat/AppData/Roaming/WachilandLauncher/instances/X/.minecraft/logs/wbombo-zipfs-agent.log".to_string(),
            ]
        );
    }

    #[test]
    fn skips_outside_windows_and_outside_java25() {
        let (jar, status) = (Path::new(JAR), Path::new(STATUS));
        let mut i = inputs(jar, status);
        i.windows = false;
        assert_eq!(decide(&i), Decision::Skip("not-windows"));
        let mut i = inputs(jar, status);
        i.java25 = false;
        assert_eq!(decide(&i), Decision::Skip("not-java25"));
    }

    #[test]
    fn skips_when_the_jvm_is_below_java_25() {
        let (jar, status) = (Path::new(JAR), Path::new(STATUS));
        let mut i = inputs(jar, status);
        i.java_major = 21;
        assert_eq!(decide(&i), Decision::Skip("java-major-below-25"));
    }

    #[test]
    fn environment_switch_disables_it_in_every_spelling() {
        let (jar, status) = (Path::new(JAR), Path::new(STATUS));
        for value in ["0", "off", "OFF", " false ", "No"] {
            let mut i = inputs(jar, status);
            i.disable_value = Some(value);
            assert_eq!(decide(&i), Decision::Skip("disabled-by-env"), "value {value:?}");
        }
        for value in ["1", "on", ""] {
            let mut i = inputs(jar, status);
            i.disable_value = Some(value);
            assert!(matches!(decide(&i), Decision::Use(_)), "value {value:?}");
        }
    }

    #[test]
    fn writes_the_embedded_jar_once_and_rewrites_a_different_file() {
        let dir = std::env::temp_dir().join(format!("pandora-zipfs-agent-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = super::ensure_agent_jar(&dir).expect("first write");
        assert_eq!(super::file_sha256(&path).as_deref(), Some(expected_sha256()));
        // Same content: nothing else is written, and the file keeps its contents.
        let before = std::fs::metadata(&path).expect("metadata").modified().ok();
        super::ensure_agent_jar(&dir).expect("second call");
        assert_eq!(std::fs::metadata(&path).expect("metadata").modified().ok(), before);
        // A different file in place is replaced.
        std::fs::write(&path, b"stale").expect("stale write");
        super::ensure_agent_jar(&dir).expect("repair");
        assert_eq!(super::file_sha256(&path).as_deref(), Some(expected_sha256()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
