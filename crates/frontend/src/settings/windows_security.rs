use std::{path::{Component, Path, PathBuf}, rc::Rc};

use gpui::{prelude::*, *};
use gpui_component::v_flex;

use super::{SettingGroup, SettingItem, SettingItemWidget};

const OWNERSHIP_BLOCKER: &str =
    "Automatic add/remove is unavailable: Microsoft Defender path exclusions are string values without a per-entry owner/id, so Pandora cannot prove a later identical exclusion still belongs to Pandora before removing it.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CandidateBlocker {
    NotAbsolute,
    NotExecutable,
    PortableBuild,
    GameOrModsTree,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecommendationModel {
    exact_executable: Option<PathBuf>,
    candidate_blocker: Option<CandidateBlocker>,
}

fn exact_launcher_candidate(path: &Path) -> Result<PathBuf, CandidateBlocker> {
    if !path.is_absolute() {
        return Err(CandidateBlocker::NotAbsolute);
    }

    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return Err(CandidateBlocker::NotExecutable);
    };
    let file_name_lower = file_name.to_ascii_lowercase();
    if !file_name_lower.ends_with(".exe") {
        return Err(CandidateBlocker::NotExecutable);
    }
    if file_name_lower.contains("portable") {
        return Err(CandidateBlocker::PortableBuild);
    }

    if path.components().any(|component| {
        let Component::Normal(name) = component else {
            return false;
        };
        let name = name.to_string_lossy();
        name.eq_ignore_ascii_case(".minecraft") || name.eq_ignore_ascii_case("mods")
    }) {
        return Err(CandidateBlocker::GameOrModsTree);
    }

    Ok(path.to_path_buf())
}

fn recommendation_model() -> RecommendationModel {
    let Some(path) = std::env::current_exe().ok() else {
        return RecommendationModel {
            exact_executable: None,
            candidate_blocker: Some(CandidateBlocker::NotExecutable),
        };
    };
    let path = path.canonicalize().unwrap_or(path);

    match exact_launcher_candidate(&path) {
        Ok(path) => RecommendationModel {
            exact_executable: Some(path),
            candidate_blocker: None,
        },
        Err(blocker) => RecommendationModel {
            exact_executable: None,
            candidate_blocker: Some(blocker),
        },
    }
}

pub(super) fn create_group() -> SettingGroup {
    let model = recommendation_model();

    SettingGroup {
        title: Some(|| "Windows security"),
        items: vec![SettingItem {
            title: || "Microsoft Defender exclusion",
            description: || "Optional manual recommendation only. Pandora does not change Defender automatically in this candidate.",
            widget: SettingItemWidget::Any(Rc::new(move |_, _| {
                let candidate = if let Some(path) = &model.exact_executable {
                    format!(
                        "Narrow candidate only: Microsoft Defender Antivirus path exclusion for this exact launcher file:\n{}",
                        path.display()
                    )
                } else {
                    format!(
                        "No exclusion is recommended from this launch location ({:?}).",
                        model.candidate_blocker
                    )
                };

                v_flex()
                    .gap_1()
                    .max_w(px(520.0))
                    .child(div().text_sm().child(SharedString::new(candidate)))
                    .child(div().text_sm().child(OWNERSHIP_BLOCKER))
                    .child(div().text_sm().child(
                        "Risk: even a single-file exclusion reduces antivirus inspection for that file. Never exclude .minecraft, mods, downloads, a parent directory, or a drive.",
                    ))
                    .child(
                        div()
                            .id("bootoptim-open-windows-security")
                            .text_sm()
                            .underline()
                            .child("Open Windows Security")
                            .on_click(|_, _, _| {
                                _ = open::that_detached("ms-settings:windowsdefender");
                            }),
                    )
                    .into_any_element()
            })),
            ..Default::default()
        }]
        .into(),
        searched_items: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_is_exact_executable_not_parent() {
        let path = Path::new(r"C:\Program Files\BootOptim\PandoraLauncher.exe");
        assert_eq!(exact_launcher_candidate(path).unwrap(), path);
    }

    #[test]
    fn portable_and_game_content_locations_are_rejected() {
        assert_eq!(
            exact_launcher_candidate(Path::new(r"C:\Tools\PandoraLauncher-portable.exe")),
            Err(CandidateBlocker::PortableBuild)
        );
        assert_eq!(
            exact_launcher_candidate(Path::new(r"C:\Games\Pack\.minecraft\PandoraLauncher.exe")),
            Err(CandidateBlocker::GameOrModsTree)
        );
        assert_eq!(
            exact_launcher_candidate(Path::new(r"C:\Games\Pack\mods\PandoraLauncher.exe")),
            Err(CandidateBlocker::GameOrModsTree)
        );
    }

    #[test]
    fn non_executable_or_relative_paths_are_rejected() {
        assert_eq!(
            exact_launcher_candidate(Path::new(r"PandoraLauncher.exe")),
            Err(CandidateBlocker::NotAbsolute)
        );
        assert_eq!(
            exact_launcher_candidate(Path::new(r"C:\Program Files\BootOptim\PandoraLauncher")),
            Err(CandidateBlocker::NotExecutable)
        );
    }

    #[test]
    fn automatic_action_remains_blocked_by_ownership_contract() {
        assert!(OWNERSHIP_BLOCKER.contains("without a per-entry owner/id"));
        assert!(OWNERSHIP_BLOCKER.contains("cannot prove"));
    }
}
