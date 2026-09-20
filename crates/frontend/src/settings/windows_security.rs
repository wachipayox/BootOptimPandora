use std::rc::Rc;

use gpui::{prelude::*, *};
use gpui_component::v_flex;

use super::{SettingGroup, SettingItem, SettingItemWidget};

const UTILITY_BLOCKER: &str = "No Defender exclusion is recommended: excluding only Pandora.exe would stop Defender scanning that executable itself, but it would not reduce scans of the modpack JARs/files that motivated this investigation.";
const SCOPE_BLOCKER: &str = "A Defender process exclusion is also rejected because Microsoft documents it as excluding files opened by that process. Excluding .minecraft, mods, downloads, a parent directory, user files, or a drive is outside BootOptim's security contract.";
const OWNERSHIP_BLOCKER: &str = "Automatic add/remove is additionally blocked because Defender exclusion entries do not provide a per-entry Pandora owner/id. Pandora therefore cannot prove that a later identical exclusion is still its own before removing it.";

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CandidateScope {
    LauncherFile,
    LauncherProcess,
    GameOrUserTree,
    LauncherOwnedCache,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScopeDisposition {
    NotUsefulForObservedJarScanning,
    RejectedTooBroad,
    RejectedForbidden,
    UnavailableWithoutFixedIdentity,
}

#[cfg(test)]
fn scope_disposition(scope: CandidateScope) -> ScopeDisposition {
    match scope {
        CandidateScope::LauncherFile => ScopeDisposition::NotUsefulForObservedJarScanning,
        CandidateScope::LauncherProcess => ScopeDisposition::RejectedTooBroad,
        CandidateScope::GameOrUserTree => ScopeDisposition::RejectedForbidden,
        CandidateScope::LauncherOwnedCache => ScopeDisposition::UnavailableWithoutFixedIdentity,
    }
}

pub(super) fn create_group() -> SettingGroup {
    SettingGroup {
        title: Some(|| "Windows security"),
        items: vec![SettingItem {
            title: || "Microsoft Defender performance",
            description: || {
                "Diagnostic recommendation only. This candidate does not create or remove Defender exclusions."
            },
            widget: SettingItemWidget::Any(Rc::new(|_, _| {
                v_flex()
                    .gap_1()
                    .max_w(px(520.0))
                    .child(div().text_sm().child(UTILITY_BLOCKER))
                    .child(div().text_sm().child(SCOPE_BLOCKER))
                    .child(div().text_sm().child(OWNERSHIP_BLOCKER))
                    .child(div().text_sm().child(
                        "Safer next step: use Microsoft Defender's performance analyzer to identify the actual scan hot spots before considering any future narrowly owned cache integration.",
                    ))
                    .child(
                        div()
                            .id("bootoptim-open-defender-performance-docs")
                            .text_sm()
                            .underline()
                            .child("Open Defender performance analyzer documentation")
                            .on_click(|_, _, _| {
                                _ = open::that_detached(
                                    "https://learn.microsoft.com/defender-endpoint/performance-analyzer-reference",
                                );
                            }),
                    )
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
    fn launcher_file_exclusion_is_not_presented_as_jar_scan_remedy() {
        assert_eq!(
            scope_disposition(CandidateScope::LauncherFile),
            ScopeDisposition::NotUsefulForObservedJarScanning
        );
        assert!(UTILITY_BLOCKER.contains("would not reduce scans of the modpack JARs"));
    }

    #[test]
    fn process_exclusion_is_rejected_as_too_broad() {
        assert_eq!(
            scope_disposition(CandidateScope::LauncherProcess),
            ScopeDisposition::RejectedTooBroad
        );
        assert!(SCOPE_BLOCKER.contains("files opened by that process"));
    }

    #[test]
    fn game_and_user_content_exclusions_are_forbidden() {
        assert_eq!(
            scope_disposition(CandidateScope::GameOrUserTree),
            ScopeDisposition::RejectedForbidden
        );
        for forbidden in [".minecraft", "mods", "downloads", "user files", "drive"] {
            assert!(SCOPE_BLOCKER.contains(forbidden));
        }
    }

    #[test]
    fn launcher_cache_requires_fixed_identity_before_reconsideration() {
        assert_eq!(
            scope_disposition(CandidateScope::LauncherOwnedCache),
            ScopeDisposition::UnavailableWithoutFixedIdentity
        );
    }

    #[test]
    fn automatic_mutation_remains_blocked_by_ownership_contract() {
        assert!(OWNERSHIP_BLOCKER.contains("per-entry Pandora owner/id"));
        assert!(OWNERSHIP_BLOCKER.contains("cannot prove"));
    }
}
