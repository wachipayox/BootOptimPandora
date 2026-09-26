use command::DefenderProcessAction;
use gpui::{prelude::*, *};
use gpui_component::{
    WindowExt,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};

pub fn open(window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, _, _| {
        let buttons = h_flex()
            .w_full()
            .gap_2()
            .child(
                Button::new("enable-pandora-defender-process-exclusion")
                    .flex_1()
                    .label("Enable optimization")
                    .success()
                    .on_click(|_, window, cx| {
                        let result =
                            command::request_defender_process_action(DefenderProcessAction::Enable);
                        window.close_all_dialogs(cx);
                        crate::windows_security_ui::push_result(window, cx, result);
                        cx.refresh_windows();
                    }),
            )
            .child(
                Button::new("skip-pandora-defender-process-exclusion")
                    .flex_1()
                    .label("Not now")
                    .on_click(|_, window, cx| {
                        window.close_all_dialogs(cx);
                    }),
            );

        dialog
            .title("Optimize Defender scanning for Wachiland Launcher")
            .overlay_closable(true)
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        "Defender will not scan files opened by this Wachiland Launcher executable. Enabling requires administrator approval.",
                    )
                    .child(
                        div()
                            .id("pandora-defender-process-first-run-details")
                            .text_sm()
                            .underline()
                            .child("Learn what a process exclusion changes")
                            .on_click(|_, _, _| {
                                _ = open::that_detached(
                                    "https://learn.microsoft.com/defender-endpoint/microsoft-defender-antivirus-exclusions-overview",
                                );
                            }),
                    )
                    .child(buttons),
            )
    });
}
