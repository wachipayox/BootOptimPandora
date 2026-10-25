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
        title: || "Seguridad de Windows",
        groups: vec![SettingGroup {
            title: None,
            items: vec![SettingItem {
                title: || "Microsoft Defender",
                description: || "Exclusión opcional de Microsoft Defender para el ejecutable del launcher: acelera el arranque. Pide permiso de administrador.",
                widget: SettingItemWidget::Any(Rc::new(|_window, cx| {
                    let (status, action, label) = match command::defender_process_local_state() {
                        DefenderProcessLocalState::NotManaged => (
                            "Desactivada",
                            Some(DefenderProcessAction::Enable),
                            "Activar optimización",
                        ),
                        DefenderProcessLocalState::ManagedCurrent => (
                            "Activada",
                            Some(DefenderProcessAction::Remove),
                            "Quitar optimización",
                        ),
                        DefenderProcessLocalState::ManagedPrevious => (
                            "Existe una exclusión de un ejecutable anterior de esta instalación.",
                            Some(DefenderProcessAction::Remove),
                            "Quitar exclusión anterior",
                        ),
                        DefenderProcessLocalState::InvalidOwnershipRecord => (
                            "No se ha podido comprobar la exclusión. No se puede quitar automáticamente.",
                            None,
                            "",
                        ),
                    };

                    let mut controls = h_flex().gap_2();
                    if let Some(action) = action {
                        controls = controls.child(
                            Button::new("pandora-defender-process-action")
                                .label(label)
                                .ghost().rounded(px(0.)).text_size(px(13.))
                                .on_click(move |_, window, cx| {
                                    crate::windows_security_ui::request_action(action, window, cx);
                                }),
                        );
                    }

                    controls = controls.child(
                        div()
                            .id("pandora-defender-process-details")
                            .text_sm()
                            .underline()
                            .child("Cómo funciona")
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
                            "Defender dejará de analizar los archivos que abre este ejecutable del launcher. Requiere permiso de administrador.",
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
