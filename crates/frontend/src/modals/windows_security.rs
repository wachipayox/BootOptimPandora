use crate::component::design;
use command::DefenderProcessAction;
use gpui::{prelude::*, *};
use gpui_component::{WindowExt, h_flex, v_flex};

pub fn open(window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, _, _| {
        let buttons = h_flex()
            .w_full()
            .gap_2()
            .child(
                design::action_button("enable-pandora-defender-process-exclusion","Activar",design::ButtonAction::Primary)
                    .on_click(|_, window, cx| {
                        window.close_all_dialogs(cx);
                        crate::windows_security_ui::request_action(DefenderProcessAction::Enable, window, cx);
                    }),
            )
            .child(
                design::action_button("skip-pandora-defender-process-exclusion","Ahora no",design::ButtonAction::Secondary)
                    .on_click(|_, window, cx| {
                        window.close_all_dialogs(cx);
                    }),
            );

        design::dialog(dialog)
            .title(design::dialog_title("Acelerar el análisis de Defender"))
            .overlay_closable(true)
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        "Defender dejará de analizar los archivos que abre este ejecutable del launcher. Requiere permiso de administrador.",
                    )
                    .child(
                        div()
                            .id("pandora-defender-process-first-run-details")
                            .text_sm()
                            .underline()
                            .child("Qué cambia una exclusión de proceso")
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
