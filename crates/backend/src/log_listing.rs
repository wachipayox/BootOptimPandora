//! On-demand log inventory, including Minecraft crash reports.
use bridge::message::{LogFileInfo, LogFiles};
use std::{path::Path, sync::Arc, time::UNIX_EPOCH};
fn entry(path: &Path, crash: bool, launcher: bool) -> Option<LogFileInfo> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    let modified = metadata.modified().unwrap_or(UNIX_EPOCH).max(metadata.created().unwrap_or(UNIX_EPOCH));
    Some(LogFileInfo {
        path: Arc::from(path),
        modified_unix_ms: modified.duration_since(UNIX_EPOCH).unwrap_or_default().as_millis().min(u64::MAX as u128)
            as u64,
        size: metadata.len(),
        crash,
        launcher,
    })
}
pub(crate) fn list_logs(root: &Path, launcher: Option<&Path>) -> LogFiles {
    let mut out = LogFiles::default();
    for (folder, crash) in [("logs", false), ("crash-reports", true)] {
        if let Ok(entries) = std::fs::read_dir(root.join(folder)) {
            for file in entries.flatten() {
                let name = file.file_name();
                let Some(name) = name.to_str() else { continue };
                if !(if crash {
                    name.starts_with("crash-") && name.ends_with(".txt")
                } else {
                    name.ends_with(".log") || name.ends_with(".log.gz")
                }) {
                    continue;
                }
                if let Some(info) = entry(&file.path(), crash, false) {
                    if !crash && name.ends_with(".log.gz") {
                        out.total_gzipped_size =
                            out.total_gzipped_size.saturating_add(info.size.min(usize::MAX as u64) as usize);
                    }
                    out.files.push(info);
                }
            }
        }
    }
    if let Some(info) = launcher.and_then(|path| entry(path, false, true)) {
        out.files.push(info);
    }
    out.files
        .sort_by(|a, b| b.modified_unix_ms.cmp(&a.modified_unix_ms).then_with(|| a.path.cmp(&b.path)));
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lists_crashes_and_logs_with_sizes_without_treating_other_files_as_logs() {
        let root = std::env::temp_dir().join(format!("pandora-log-list-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("logs")).unwrap();
        std::fs::create_dir(root.join("crash-reports")).unwrap();
        for (path, body) in [
            ("logs/latest.log", "log"),
            ("logs/older.log.gz", "archive"),
            ("logs/options.txt", "skip"),
            ("crash-reports/crash-example.txt", "crash"),
        ] {
            std::fs::write(root.join(path), body).unwrap();
        }
        let found = list_logs(&root, None);
        assert_eq!(found.files.len(), 3);
        assert_eq!(found.total_gzipped_size, 7);
        assert_eq!(found.files.iter().filter(|f| f.crash).count(), 1);
        assert!(found.files.iter().all(|f| !f.launcher));
        assert!(found.files.iter().any(|f| f.size == 5 && f.crash));
        for path in [
            "logs/latest.log",
            "logs/older.log.gz",
            "logs/options.txt",
            "crash-reports/crash-example.txt",
        ] {
            std::fs::remove_file(root.join(path)).unwrap();
        }
        std::fs::remove_dir(root.join("logs")).unwrap();
        std::fs::remove_dir(root.join("crash-reports")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
