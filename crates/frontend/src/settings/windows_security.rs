use std::rc::Rc;

use command::{DefenderProcessAction, DefenderProcessLocalState};
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};

use super::{SettingGroup, SettingItem, SettingItemWidget, SettingPage};

pub(super) fn create_page() -> SettingPage {
    SettingPage {
        title: || "Windows security",
        groups: vec![SettingGroup {
            title: None,
            items: vec![SettingItem {
                title: || "Microsoft Defender",
                description: || "Optional process exclusion for the canonical Pandora executable.",
                widget: SettingItemWidget::Any(Rc::new(|window, cx| {
                    let (status, action, label) = match command::defender_process_local_state() {
                        DefenderProcessLocalState::NotManaged => (
                            "Not enabled. Defender scans files opened by Pandora normally.",
                            Some(DefenderProcessAction::Enable),
                            "Enable optimization",
                        ),
                        DefenderProcessLocalState::ManagedCurrent => (
                            "Enabled and owned by Pandora for this executable.",
                            Some(DefenderProcessAction::Remove),
                            "Remove optimization",
                        ),
                        DefenderProcessLocalState::ManagedPrevious => (
                            "A previous Pandora executable in this installation still has an owned exclusion.",
                            Some(DefenderProcessAction::Remove),
                            "Remove previous exclusion",
                        ),
                        DefenderProcessLocalState::InvalidOwnershipRecord => (
                            "Pandora cannot validate its local exclusion ownership record, so automatic removal is disabled.",
                            None,
                            "",
                        ),
                    };

                    let mut controls = h_flex().gap_2();
                    if let Some(action) = action {
                        controls = controls.child(
                            Button::new("pandora-defender-process-action")
                                .label(label)
                                .when(action == DefenderProcessAction::Enable, ButtonVariants::success)
                                .on_click(move |_, window, cx| {
                                    let result = command::request_defender_process_action(action);
                                    crate::windows_security_ui::push_result(window, cx, result);
                                    cx.refresh_windows();
                                }),
                        );
                    }

                    controls = controls.child(
                        div()
                            .id("pandora-defender-process-details")
                            .text_sm()
                            .underline()
                            .child("How process exclusions work")
                            .on_click(|_, _, _| {
                                _ = open::that_detached(
                                    "https://learn.microsoft.com/defender-endpoint/microsoft-defender-antivirus-exclusions-overview",
                                );
                            }),
                    );

                    v_flex()
                        .gap_1()
                        .max_w(px(560.0))
                        .child(div().text_sm().child(
                            "Defender will not scan files opened by this Pandora executable. Java, Minecraft and game folders are not added as exclusions.",
                        ))
                        .child(div().text_sm().text_color(cx.theme().muted_foreground).child(status))
                        .child(controls)
                        .into_any_element()
                })),
                ..Default::default()
            }]
            .into(),
            searched_items: None,
        }]
        .into(),
        searched_groups: None,
    }
}
