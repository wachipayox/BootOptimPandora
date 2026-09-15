#![cfg(test)]

use std::{
    collections::{BTreeSet, HashMap},
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    time::Instant,
};

use uuid::Uuid;

use crate::fs::{copy_content_recursive, fastcopy, write_safe};

const SCHEMA: &str = "bootoptim.prelaunch_mods_probe.v1";

#[derive(Debug, Clone, Copy, Default)]
struct PhaseSample {
    wall_ns: u128,
    cpu_ns: Option<u128>,
    bytes: u64,
    files: u64,
    dirs: u64,
}

#[derive(Debug)]
struct TempTree {
    root: PathBuf,
}

impl TempTree {
    fn new() -> io::Result<Self> {
        let root = std::env::temp_dir().join(format!("bootoptim-agent175-{}", Uuid::new_v4()));
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[derive(Debug)]
struct Fixture {
    _temp: TempTree,
    instance_root: PathBuf,
    mods_dir: PathBuf,
    dot_minecraft: PathBuf,
    content_library: PathBuf,
    managed: Vec<(PathBuf, PathBuf)>,
    known_top_level: BTreeSet<PathBuf>,
}

impl Fixture {
    fn create(managed_count: usize, managed_bytes: usize, extra_bytes: usize) -> io::Result<Self> {
        let temp = TempTree::new()?;
        let instance_root = temp.root.join("instance");
        let mods_dir = instance_root.join(".minecraft/mods");
        let dot_minecraft = instance_root.join(".minecraft");
        let content_library = temp.root.join("content-library");
        fs::create_dir_all(&mods_dir)?;
        fs::create_dir_all(&content_library)?;

        let mut managed = Vec::new();
        let mut known_top_level = BTreeSet::new();
        for index in 0..managed_count {
            let rel = PathBuf::from(format!("managed-{index:03}.jar"));
            let source = content_library.join(format!("managed-{index:03}.jar"));
            write_pattern(&source, managed_bytes, index as u8)?;
            write_pattern(&mods_dir.join(&rel), 32, 0xA5)?;
            known_top_level.insert(rel.clone());
            managed.push((source, rel));
        }

        write_pattern(&mods_dir.join("user-added.jar"), extra_bytes, 0x33)?;
        write_pattern(&mods_dir.join("notes.txt"), 4096, 0x44)?;
        write_pattern(&mods_dir.join("mcef-cache/cache.bin"), extra_bytes / 2, 0x55)?;
        write_pattern(&mods_dir.join(".connector/cache.bin"), extra_bytes / 2, 0x66)?;

        Ok(Self {
            _temp: temp,
            instance_root,
            mods_dir,
            dot_minecraft,
            content_library,
            managed,
            known_top_level,
        })
    }
}

fn write_pattern(path: &Path, len: usize, seed: u8) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::File::create(path)?;
    let block = vec![seed; 8192];
    let mut remaining = len;
    while remaining > 0 {
        let amount = remaining.min(block.len());
        file.write_all(&block[..amount])?;
        remaining -= amount;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn process_cpu_ns() -> Option<u128> {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    let result = unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, &mut ts) };
    (result == 0).then_some(ts.tv_sec.max(0) as u128 * 1_000_000_000 + ts.tv_nsec.max(0) as u128)
}

#[cfg(not(target_os = "linux"))]
fn process_cpu_ns() -> Option<u128> {
    None
}

fn measured<T>(op: impl FnOnce() -> io::Result<(T, u64, u64, u64)>) -> io::Result<(T, PhaseSample)> {
    let wall_start = Instant::now();
    let cpu_start = process_cpu_ns();
    let (value, bytes, files, dirs) = op()?;
    let cpu_end = process_cpu_ns();
    let cpu_ns = match (cpu_start, cpu_end) {
        (Some(start), Some(end)) => Some(end.saturating_sub(start)),
        _ => None,
    };
    Ok((value, PhaseSample {
        wall_ns: wall_start.elapsed().as_nanos(),
        cpu_ns,
        bytes,
        files,
        dirs,
    }))
}

fn scan_unknown_top_level(mods_dir: &Path, known: &BTreeSet<PathBuf>) -> io::Result<(Vec<PathBuf>, u64, u64)> {
    let mut unknown = Vec::new();
    let mut files = 0;
    let mut dirs = 0;
    for entry in fs::read_dir(mods_dir)? {
        let entry = entry?;
        let relative = PathBuf::from(entry.file_name());
        let ty = entry.file_type()?;
        if ty.is_dir() {
            dirs += 1;
        } else {
            files += 1;
        }
        if !known.contains(&relative) {
            unknown.push(relative);
        }
    }
    unknown.sort();
    Ok((unknown, files, dirs))
}

fn copy_extra(from: &Path, to: &Path) -> io::Result<(u64, u64, u64)> {
    if from.is_dir() {
        fs::create_dir_all(to)?;
        let mut final_total = 0;
        copy_content_recursive(from, to, false, &|copied, total| {
            let _ = copied;
            final_total = total;
        })?;
        let mut files = 0;
        let mut dirs = 1;
        for entry in walkdir::WalkDir::new(from).min_depth(1) {
            let entry = entry.map_err(io::Error::other)?;
            if entry.file_type().is_file() {
                files += 1;
            } else if entry.file_type().is_dir() {
                dirs += 1;
            }
        }
        Ok((final_total, files, dirs))
    } else {
        let bytes = fs::copy(from, to)?;
        Ok((bytes, 1, 0))
    }
}

fn restore_stock_layout(instance_root: &Path, mods_dir: &Path, sandbox: bool) -> io::Result<PhaseSample> {
    let original_mods = instance_root.join("original_mods");
    let (_, sample) = measured(|| {
        let mut bytes = 0;
        let mut files = 0;
        let mut dirs = 0;
        if !sandbox {
            let connector = mods_dir.join(".connector");
            if connector.exists() {
                let original_connector = original_mods.join(".connector");
                fs::create_dir_all(&original_connector)?;
                let mut final_total = 0;
                copy_content_recursive(&connector, &original_connector, false, &|_, total| {
                    final_total = total;
                })?;
                bytes += final_total;
                dirs += 1;
                files += walkdir::WalkDir::new(&connector)
                    .min_depth(1)
                    .into_iter()
                    .filter_map(Result::ok)
                    .filter(|entry| entry.file_type().is_file())
                    .count() as u64;
            }
        }
        let _ = fs::remove_dir_all(mods_dir);
        fs::rename(&original_mods, mods_dir)?;
        Ok(((), bytes, files, dirs))
    })?;
    Ok(sample)
}

fn run_stock_filesystem_cycle(fixture: &Fixture, sandbox: bool) -> io::Result<HashMap<&'static str, PhaseSample>> {
    let mut samples = HashMap::new();

    let (unknown, scan) = measured(|| {
        let (unknown, files, dirs) = scan_unknown_top_level(&fixture.mods_dir, &fixture.known_top_level)?;
        Ok((unknown, 0, files, dirs))
    })?;
    samples.insert("scan_mods_top_level", scan);

    let original_mods = fixture.instance_root.join("original_mods");
    let (_, rotate) = measured(|| {
        fs::rename(&fixture.mods_dir, &original_mods)?;
        fs::create_dir_all(&fixture.mods_dir)?;
        Ok(((), 0, 0, 1))
    })?;
    samples.insert("rotate_original_mods", rotate);

    let (_, immutable) = measured(|| {
        let mut bytes = 0;
        for (source, relative) in &fixture.managed {
            let target = fixture.mods_dir.join(relative);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fastcopy(source, &target, true, false)?;
            bytes += fs::metadata(source)?.len();
        }
        Ok(((), bytes, fixture.managed.len() as u64, 0))
    })?;
    samples.insert("materialize_immutable_mods", immutable);

    let (_, extras) = measured(|| {
        let mut bytes = 0;
        let mut files = 0;
        let mut dirs = 0;
        for relative in &unknown {
            let (b, f, d) = copy_extra(&original_mods.join(relative), &fixture.mods_dir.join(relative))?;
            bytes += b;
            files += f;
            dirs += d;
        }
        Ok(((), bytes, files, dirs))
    })?;
    samples.insert("copy_user_extra_entries", extras);

    let (_, modpack_extra) = measured(|| {
        let target = fixture.dot_minecraft.join("config/yosbr/options.txt");
        let payload = vec![0x77; 32 * 1024];
        write_safe(&target, &payload)?;
        Ok(((), payload.len() as u64, 1, 0))
    })?;
    samples.insert("write_modpack_extra_file", modpack_extra);

    write_pattern(&fixture.mods_dir.join(".connector/cache.bin"), 96 * 1024, 0x99)?;
    let restore = restore_stock_layout(&fixture.instance_root, &fixture.mods_dir, sandbox)?;
    samples.insert("restore_original_mods", restore);

    Ok(samples)
}

fn emit_sample(phase: &str, sample: PhaseSample) {
    let cpu = sample.cpu_ns.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string());
    eprintln!(
        "{{\"schema\":\"{SCHEMA}\",\"phase\":\"{phase}\",\"wall_ns\":{},\"cpu_ns\":{cpu},\"bytes\":{},\"files\":{},\"dirs\":{}}}",
        sample.wall_ns, sample.bytes, sample.files, sample.dirs
    );
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ReuseIdentity {
    managed_selection: u64,
    modpack_files: u64,
    disabled_children: u64,
    extra_entries: u64,
    config_inputs: u64,
    sync_targets: u64,
    sandbox: bool,
}

fn fail_open_reuse_allowed(
    prior: Option<&ReuseIdentity>,
    current: &ReuseIdentity,
    prepared_layout_present: bool,
    previous_exit_clean: bool,
    original_mods_absent: bool,
) -> bool {
    previous_exit_clean
        && prepared_layout_present
        && original_mods_absent
        && prior == Some(current)
}

#[test]
fn hosted_representative_prelaunch_filesystem_attribution() -> io::Result<()> {
    let fixture = Fixture::create(64, 64 * 1024, 256 * 1024)?;
    let samples = run_stock_filesystem_cycle(&fixture, false)?;

    for phase in [
        "scan_mods_top_level",
        "rotate_original_mods",
        "materialize_immutable_mods",
        "copy_user_extra_entries",
        "write_modpack_extra_file",
        "restore_original_mods",
    ] {
        let sample = samples[phase];
        emit_sample(phase, sample);
        assert!(sample.wall_ns > 0);
    }

    assert!(samples["materialize_immutable_mods"].bytes >= 4 * 1024 * 1024);
    assert!(samples["copy_user_extra_entries"].bytes > 0);
    assert!(fixture.mods_dir.join("user-added.jar").is_file());
    assert!(fixture.mods_dir.join("mcef-cache/cache.bin").is_file());
    assert_eq!(fs::metadata(fixture.mods_dir.join(".connector/cache.bin"))?.len(), 96 * 1024);
    assert!(!fixture.instance_root.join("original_mods").exists());
    Ok(())
}

#[test]
fn repeated_layouts_preserve_user_visible_mods_and_connector_roundtrip() -> io::Result<()> {
    for _ in 0..2 {
        let fixture = Fixture::create(12, 8192, 32 * 1024)?;
        let _ = run_stock_filesystem_cycle(&fixture, false)?;
        assert_eq!(fs::metadata(fixture.mods_dir.join("user-added.jar"))?.len(), 32 * 1024);
        assert_eq!(fs::metadata(fixture.mods_dir.join(".connector/cache.bin"))?.len(), 96 * 1024);
    }
    Ok(())
}

#[test]
fn sandbox_restore_does_not_merge_runtime_connector_cache() -> io::Result<()> {
    let fixture = Fixture::create(4, 4096, 16 * 1024)?;
    let _ = run_stock_filesystem_cycle(&fixture, true)?;
    assert_eq!(fs::metadata(fixture.mods_dir.join(".connector/cache.bin"))?.len(), 8 * 1024);
    Ok(())
}

#[test]
fn interrupted_prelaunch_can_restore_original_layout_fail_open() -> io::Result<()> {
    let fixture = Fixture::create(8, 4096, 16 * 1024)?;
    let (unknown, _, _) = scan_unknown_top_level(&fixture.mods_dir, &fixture.known_top_level)?;
    let original_mods = fixture.instance_root.join("original_mods");
    fs::rename(&fixture.mods_dir, &original_mods)?;
    fs::create_dir_all(&fixture.mods_dir)?;
    for relative in unknown {
        let _ = copy_extra(&original_mods.join(&relative), &fixture.mods_dir.join(&relative))?;
    }
    write_pattern(&fixture.mods_dir.join(".connector/cache.bin"), 48 * 1024, 0x12)?;

    let _ = restore_stock_layout(&fixture.instance_root, &fixture.mods_dir, false)?;
    assert!(fixture.mods_dir.join("user-added.jar").is_file());
    assert_eq!(fs::metadata(fixture.mods_dir.join(".connector/cache.bin"))?.len(), 48 * 1024);
    assert!(!original_mods.exists());
    Ok(())
}

#[test]
fn proposed_reuse_identity_invalidates_every_semantic_input_and_uncertain_state() {
    let base = ReuseIdentity {
        managed_selection: 1,
        modpack_files: 2,
        disabled_children: 3,
        extra_entries: 4,
        config_inputs: 5,
        sync_targets: 6,
        sandbox: false,
    };
    assert!(fail_open_reuse_allowed(Some(&base), &base, true, true, true));

    let mut variants = Vec::new();
    let mut changed = base.clone(); changed.managed_selection += 1; variants.push(changed);
    let mut changed = base.clone(); changed.modpack_files += 1; variants.push(changed);
    let mut changed = base.clone(); changed.disabled_children += 1; variants.push(changed);
    let mut changed = base.clone(); changed.extra_entries += 1; variants.push(changed);
    let mut changed = base.clone(); changed.config_inputs += 1; variants.push(changed);
    let mut changed = base.clone(); changed.sync_targets += 1; variants.push(changed);
    let mut changed = base.clone(); changed.sandbox = true; variants.push(changed);

    for changed in variants {
        assert!(!fail_open_reuse_allowed(Some(&base), &changed, true, true, true));
    }
    assert!(!fail_open_reuse_allowed(None, &base, true, true, true));
    assert!(!fail_open_reuse_allowed(Some(&base), &base, false, true, true));
    assert!(!fail_open_reuse_allowed(Some(&base), &base, true, false, true));
    assert!(!fail_open_reuse_allowed(Some(&base), &base, true, true, false));
}
