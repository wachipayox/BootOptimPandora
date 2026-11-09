//! "Gestionar opciones globales" of one instance: copy the launcher-wide Minecraft settings into it,
//! or make its own settings the launcher-wide ones. New instances get them on their own; this is
//! for instances that already exist.
use crate::{component::design, icon::PandoraIcon, theme};
use bridge::{handle::BackendHandle, instance::InstanceID, message::MessageToBackend};
use gpui::{prelude::*, *};
use gpui_component::{
    Icon, WindowExt,
    button::{Button, ButtonVariants},
    dialog::Dialog,
    h_flex, v_flex,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    CopyToInstance,
    UseAsGlobal,
}

impl Action {
    fn confirmation(self) -> &'static str {
        match self {
            Action::CopyToInstance => {
                "Se sobrescribirán los ajustes de Minecraft de esta instancia (vídeo, sonido, controles…) con los globales, adaptados a su versión. Los ajustes que no se copian se quedan como están."
            },
            Action::UseAsGlobal => {
                "Los ajustes de Minecraft de esta instancia pasarán a ser los globales, y las instancias nuevas se crearán con ellos. Las instancias que ya existen no cambian."
            },
        }
    }
}

struct GlobalOptionsModal {
    id: InstanceID,
    backend: BackendHandle,
    confirming: Option<Action>,
}

impl GlobalOptionsModal {
    fn action_row(
        &self,
        action: Action,
        icon: PandoraIcon,
        title: &'static str,
        help: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        design::DesignButton::new(match action {
            Action::CopyToInstance => "global-options-copy",
            Action::UseAsGlobal => "global-options-use",
        })
        .w_full()
        .h(px(56.))
        .px(px(14.))
        .child(
            h_flex()
                .w_full()
                .gap(px(12.))
                .items_center()
                .child(Icon::new(icon).size(px(18.)).flex_shrink_0())
                .child(
                    v_flex()
                        .min_w_0()
                        .gap(px(2.))
                        .items_start()
                        .child(div().text_size(px(14.)).child(title))
                        .child(div().text_size(px(12.)).text_color(rgb(theme::INK_MUTED)).child(help)),
                ),
        )
        .on_click(cx.listener(move |state, _, _, cx| {
            state.confirming = Some(action);
            cx.notify();
        }))
    }

    fn render(&mut self, dialog: Dialog, window: &mut Window, cx: &mut Context<Self>) -> Dialog {
        let body = match self.confirming {
            None => v_flex()
                .gap(px(10.))
                .child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(19.))
                        .text_color(rgb(theme::INK_MUTED))
                        .child("Las instancias nuevas se crean con los ajustes de Minecraft globales; después cada una guarda los suyos. Se editan en Ajustes del launcher › General › Sincronización."),
                )
                .child(self.action_row(
                    Action::CopyToInstance,
                    PandoraIcon::Download,
                    "Copiar los ajustes globales a esta instancia",
                    "Sustituye sus ajustes de Minecraft por los globales.",
                    cx,
                ))
                .child(self.action_row(
                    Action::UseAsGlobal,
                    PandoraIcon::Copy,
                    "Usar los ajustes de esta instancia como globales",
                    "Las instancias nuevas se crearán con ellos.",
                    cx,
                ))
                .into_any_element(),
            Some(action) => div()
                .text_size(px(13.))
                .line_height(px(19.))
                .child(action.confirmation())
                .into_any_element(),
        };
        let footer =
            match self.confirming {
                None => h_flex().w_full().justify_end().child(
                    Button::new("close-global-options")
                        .label("Cerrar")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                ),
                Some(action) => h_flex()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .child(Button::new("back-global-options").label("Volver").on_click(cx.listener(
                        |state, _, _, cx| {
                            state.confirming = None;
                            cx.notify();
                        },
                    )))
                    .child(Button::new("confirm-global-options").primary().label("Confirmar").on_click(cx.listener(
                        move |state, _, window, cx| {
                            let id = state.id;
                            state.backend.send(match action {
                                Action::CopyToInstance => MessageToBackend::CopyGlobalOptionsToInstance { id },
                                Action::UseAsGlobal => MessageToBackend::UseInstanceOptionsAsGlobal { id },
                            });
                            window.close_dialog(cx);
                        },
                    ))),
            };
        design::dialog(dialog)
            .title(design::dialog_title("Opciones globales"))
            .width(px((window.viewport_size().width.as_f32() - 48.).min(560.)))
            .child(body)
            .footer(footer)
    }
}

pub fn open(id: InstanceID, backend: BackendHandle, window: &mut Window, cx: &mut App) {
    let state = cx.new(|_| GlobalOptionsModal {
        id,
        backend,
        confirming: None,
    });
    window.open_dialog(cx, move |dialog, window, cx| {
        cx.update_entity(&state, |state, cx| state.render(dialog, window, cx))
    });
}
