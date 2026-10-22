//! Switches off Microsoft OpenJDK's time-based G1 heap sizing for Wachiland profiles on Java 25.
//!
//! Microsoft's OpenJDK 25 builds (the `java-runtime-epsilon` runtime of Mojang is Microsoft 25.0.1) have a
//! build-specific feature, `G1UseTimeBasedHeapSizing`, that shrinks the heap from the `WatcherThread` timer
//! without taking the G1 free-list lock. In a long session the JVM dies with
//! `guarantee(... FreeList_lock->owned_by_self()) failed: master free list MT safety protocol at a safepoint`
//! (`g1HeapRegionManager.cpp`), 13 to 23 minutes into a game. See microsoft/openjdk#677 and PaperMC/Paper#14123
//! (it also fails on 25.0.3).
//!
//! The flag is a diagnostic option that only Microsoft's builds know; other JVMs reject an unknown `-XX:` option
//! and would not start. So the flag is added only when this exact JVM lists it in `-XX:+PrintFlagsFinal`. The
//! answer is cached per JDK build (hash of the `release` file plus the size of `lib/modules`) in memory and in a
//! small file, so the probe costs one JVM start per JDK and not one per Start. A JVM that no longer lists the
//! flag, or a probe that fails, launches without it and logs a `FALLBACK`.
//!
//! The flags go with the profile JVM arguments, before the player's memory setting and personal arguments, so a
//! player who sets the flag themselves keeps their choice (and then this module adds nothing). They are part of
//! the arguments that key the AOT cache, which is correct: the VM shape changed.

use std::{
    collections::HashMap,
    io::Read,
    path::Path,
    process::Stdio,
    sync::{LazyLock, Mutex, mpsc},
    time::Duration,
};

use serde::{Deserialize, Serialize};

/// Mechanism id in the `[WBOMBO-COMPAT]` log lines.
pub(crate) const MECHANISM: &str = "g1-time-based-heap-sizing-off";
/// The VM flag, without `-XX:` and without a sign.
pub(crate) const FLAG: &str = "G1UseTimeBasedHeapSizing";
/// Minimum Java major that can carry the bug (the feature is in Microsoft's builds from 25).
pub(crate) const MIN_JAVA_MAJOR: u32 = 25;
/// File name of the probe cache inside the launcher folder.
const CACHE_FILE_NAME: &str = "jvm-flag-probe.json";
/// A JVM that takes longer than this to print its flags is not trusted.
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);

/// Whether the JVM knows the flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Support {
    Known,
    Unknown,
    ProbeFailed,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Decision {
    Use(Vec<String>),
    /// A precondition does not hold; nothing to report as a problem.
    Skip(&'static str),
    /// The mechanism could not apply on a Java 25 Wachiland launch.
    Fallback(&'static str),
}

pub(crate) struct Inputs<'a> {
    pub java25: bool,
    pub java_major: u32,
    /// Every VM argument already decided for this launch: profile options and the player's personal arguments.
    pub existing_args: &'a [String],
}

/// Pure decision. `support` is asked only when the other preconditions hold, because it can start a JVM.
pub(crate) fn decide(inputs: &Inputs<'_>, support: impl FnOnce() -> Support) -> Decision {
    if !inputs.java25 {
        return Decision::Skip("not-java25");
    }
    if inputs.java_major < MIN_JAVA_MAJOR {
        return Decision::Skip("java-major-below-25");
    }
    if inputs.existing_args.iter().any(|arg| names_flag(arg)) {
        return Decision::Skip("set-by-profile-or-player");
    }
    match support() {
        Support::Known => {},
        Support::Unknown => return Decision::Fallback("flag-not-in-this-jvm"),
        Support::ProbeFailed => return Decision::Fallback("jvm-probe-failed"),
    }
    let mut args = Vec::new();
    if !inputs.existing_args.iter().any(|arg| arg == "-XX:+UnlockDiagnosticVMOptions") {
        args.push("-XX:+UnlockDiagnosticVMOptions".to_owned());
    }
    args.push(format!("-XX:-{FLAG}"));
    Decision::Use(args)
}

/// JVM arguments for this launch. Logs the decision. May start the JVM once per JDK build to look for the flag.
pub(crate) fn jvm_args(
    java25: bool,
    java_major: u32,
    java_path: &Path,
    cache_dir: &Path,
    existing_args: &[String],
) -> Vec<String> {
    let inputs = Inputs {
        java25,
        java_major,
        existing_args,
    };
    let version = std::cell::Cell::new(None::<String>);
    let decision = decide(&inputs, || {
        let (support, runtime_version) = probe_cached(java_path, cache_dir);
        version.set(runtime_version);
        support
    });
    let mod_label = version
        .take()
        .map_or_else(|| format!("java@{java_major}"), |version| format!("java@{version}"));
    match decision {
        Decision::Skip(reason) => {
            log::info!("[WBOMBO-COMPAT] SKIPPED project=Pandora mechanism={MECHANISM} mod={mod_label} reason={reason}");
            Vec::new()
        },
        Decision::Fallback(reason) => {
            log::warn!(
                "[WBOMBO-COMPAT] FALLBACK project=Pandora mechanism={MECHANISM} mod={mod_label} reason={reason}"
            );
            Vec::new()
        },
        Decision::Use(args) => {
            log::info!("[WBOMBO-COMPAT] ACTIVE project=Pandora mechanism={MECHANISM} mod={mod_label} arg=-XX:-{FLAG}");
            args
        },
    }
}

/// True for `-XX:+G1UseTimeBasedHeapSizing`, `-XX:-G1UseTimeBasedHeapSizing` and `-XX:G1UseTimeBasedHeapSizing=...`.
fn names_flag(arg: &str) -> bool {
    arg.strip_prefix("-XX:").is_some_and(|rest| {
        let rest = rest.strip_prefix(['+', '-']).unwrap_or(rest);
        rest.strip_prefix(FLAG).is_some_and(|tail| tail.is_empty() || tail.starts_with('='))
    })
}

// ---- probe ----------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedProbe {
    version: String,
    known: bool,
}

static MEMORY_CACHE: LazyLock<Mutex<HashMap<String, CachedProbe>>> = LazyLock::new(Default::default);

/// The support of this JVM, from the memory cache, then the file, then a probe. Only a completed probe is cached.
fn probe_cached(java_path: &Path, cache_dir: &Path) -> (Support, Option<String>) {
    let Some((version, key)) = super::aot_cache::jdk_identity(java_path) else {
        // Without an identity there is nothing to key the cache on: ask the JVM every time.
        return (probe_flag(java_path), None);
    };
    let support = |known: bool| if known { Support::Known } else { Support::Unknown };
    if let Some(hit) = MEMORY_CACHE.lock().ok().and_then(|cache| cache.get(&key).cloned()) {
        return (support(hit.known), Some(hit.version));
    }
    let file = cache_dir.join(CACHE_FILE_NAME);
    let mut on_disk: HashMap<String, CachedProbe> = std::fs::read(&file)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    if let Some(hit) = on_disk.get(&key).cloned() {
        if let Ok(mut cache) = MEMORY_CACHE.lock() {
            cache.insert(key, hit.clone());
        }
        return (support(hit.known), Some(hit.version));
    }
    let probed = probe_flag(java_path);
    let known = match probed {
        Support::Known => true,
        Support::Unknown => false,
        Support::ProbeFailed => return (Support::ProbeFailed, Some(version)),
    };
    let entry = CachedProbe {
        version: version.clone(),
        known,
    };
    if let Ok(mut cache) = MEMORY_CACHE.lock() {
        cache.insert(key.clone(), entry.clone());
    }
    on_disk.insert(key, entry);
    if let Ok(bytes) = serde_json::to_vec_pretty(&on_disk) {
        let _ = std::fs::create_dir_all(cache_dir);
        let temporary = cache_dir.join(format!("{CACHE_FILE_NAME}.{}.tmp", std::process::id()));
        if std::fs::write(&temporary, bytes).is_err() || std::fs::rename(&temporary, &file).is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
    }
    (probed, Some(version))
}

/// Starts the JVM with `-XX:+PrintFlagsFinal` and looks for the flag in its table.
fn probe_flag(java_path: &Path) -> Support {
    let mut command = std::process::Command::new(java_path);
    command
        .args(["-XX:+UnlockDiagnosticVMOptions", "-XX:+PrintFlagsFinal", "-version"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let Ok(mut child) = command.spawn() else {
        return Support::ProbeFailed;
    };
    let Some(mut stdout) = child.stdout.take() else {
        let _ = child.kill();
        return Support::ProbeFailed;
    };
    // The flag table is large; read it on its own thread so the child never blocks on a full pipe.
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.read_to_end(&mut bytes);
        let _ = sender.send(bytes);
    });
    let Ok(bytes) = receiver.recv_timeout(PROBE_TIMEOUT) else {
        let _ = child.kill();
        let _ = child.wait();
        return Support::ProbeFailed;
    };
    match child.wait() {
        Ok(status) if status.success() => {},
        _ => return Support::ProbeFailed,
    }
    classify_flags_output(&String::from_utf8_lossy(&bytes))
}

/// `PrintFlagsFinal` rows look like `     bool G1UseTimeBasedHeapSizing = true   {diagnostic} {default}`. An empty
/// table means the JVM did not print flags, which is a failed probe and not "unknown flag".
fn classify_flags_output(text: &str) -> Support {
    let mut rows = 0usize;
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let (Some(_ty), Some(name), Some("=")) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        rows += 1;
        if name == FLAG {
            return Support::Known;
        }
    }
    if rows == 0 {
        Support::ProbeFailed
    } else {
        Support::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    fn inputs(existing: &[String]) -> Inputs<'_> {
        Inputs {
            java25: true,
            java_major: 25,
            existing_args: existing,
        }
    }

    #[test]
    fn adds_unlock_and_the_flag_when_the_jvm_knows_it() {
        let existing = args(&["-Xmx8g"]);
        assert_eq!(
            decide(&inputs(&existing), || Support::Known),
            Decision::Use(args(&["-XX:+UnlockDiagnosticVMOptions", "-XX:-G1UseTimeBasedHeapSizing"]))
        );
    }

    #[test]
    fn does_not_repeat_unlock_when_it_is_already_there() {
        let existing = args(&["-XX:+UnlockDiagnosticVMOptions"]);
        assert_eq!(
            decide(&inputs(&existing), || Support::Known),
            Decision::Use(args(&["-XX:-G1UseTimeBasedHeapSizing"]))
        );
    }

    #[test]
    fn never_probes_or_adds_anything_outside_java_25() {
        let mut i = inputs(&[]);
        i.java25 = false;
        assert_eq!(decide(&i, || panic!("must not probe")), Decision::Skip("not-java25"));
        let mut i = inputs(&[]);
        i.java_major = 21;
        assert_eq!(decide(&i, || panic!("must not probe")), Decision::Skip("java-major-below-25"));
    }

    #[test]
    fn a_player_or_profile_setting_wins_in_every_spelling() {
        for arg in [
            "-XX:+G1UseTimeBasedHeapSizing",
            "-XX:-G1UseTimeBasedHeapSizing",
            "-XX:G1UseTimeBasedHeapSizing=true",
        ] {
            let existing = args(&["-Xmx8g", arg]);
            assert_eq!(
                decide(&inputs(&existing), || panic!("must not probe")),
                Decision::Skip("set-by-profile-or-player"),
                "{arg}"
            );
        }
        // A different flag that merely starts with the same letters does not count.
        let existing = args(&["-XX:+G1UseTimeBasedHeapSizingExtra"]);
        assert!(matches!(decide(&inputs(&existing), || Support::Known), Decision::Use(_)));
    }

    #[test]
    fn a_jvm_without_the_flag_or_a_failed_probe_launches_without_it() {
        assert_eq!(decide(&inputs(&[]), || Support::Unknown), Decision::Fallback("flag-not-in-this-jvm"));
        assert_eq!(decide(&inputs(&[]), || Support::ProbeFailed), Decision::Fallback("jvm-probe-failed"));
    }

    #[test]
    fn flags_table_classification() {
        let microsoft = "[Global flags]\n     intx ActiveProcessorCount                      = -1                                        {product} {default}\n     bool G1UseTimeBasedHeapSizing                  = true                                      {diagnostic} {default}\n";
        assert_eq!(classify_flags_output(microsoft), Support::Known);
        let other = "[Global flags]\n     intx ActiveProcessorCount                      = -1                                        {product} {default}\n     bool UseG1GC                                   = true                                      {product} {ergonomic}\n";
        assert_eq!(classify_flags_output(other), Support::Unknown);
        assert_eq!(classify_flags_output(""), Support::ProbeFailed);
        assert_eq!(
            classify_flags_output("Error: Could not create the Java Virtual Machine."),
            Support::ProbeFailed
        );
    }
}
