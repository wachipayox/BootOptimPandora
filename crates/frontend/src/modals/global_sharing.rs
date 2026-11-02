use crate::{component::design, theme};
use bridge::{
    handle::BackendHandle,
    instance::InstanceID,
    message::{InstanceGlobalSharingSummary, MessageToBackend},
};
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Disableable, StyledExt, WindowExt,
    button::{Button, ButtonVariants},
    dialog::Dialog,
    h_flex, v_flex,
};

struct GlobalSharingModal {
    id: InstanceID,
    backend: BackendHandle,
    model: Option<InstanceGlobalSharingSummary>,
    busy: bool,
    error: Option<String>,
}
impl GlobalSharingModal {
    fn load(&mut self, cx: &mut Context<Self>) {
        let (send, receive) = tokio::sync::oneshot::channel();
        self.backend.send(MessageToBackend::GetInstanceGlobalSharing {
            id: self.id,
            channel: send,
        });
        cx.spawn(async move |entity, cx| {
            let result = receive.await.unwrap_or_else(|_| Err("Launcher backend stopped".into()));
            _ = entity.update(cx, |state, cx| {
                match result {
                    Ok(model) => {
                        state.model = Some(model);
                        #[cfg(feature = "ui-capture")]
                        if std::env::var("WBOMBO_UI_CAPTURE_STATE").as_deref() == Ok("editing") {
                            let model = state.model.as_mut().unwrap();
                            model.configuration.enabled = true;
                            model.configuration.choices.insert("screenshots".into(), false);
                            model.configuration.choices.insert(".voxy".into(), true);
                        }
                        #[cfg(feature = "ui-capture")]
                        log::info!("instance-global-sharing-ready pid={}", std::process::id());
                    },
                    Err(error) => {
                        log::warn!("Unable to load instance sharing: {error}");
                        state.error = Some("No se ha podido cambiar la sincronización.".into());
                    },
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(model) = &self.model else {
            return;
        };
        let configuration = model.configuration.clone();
        let (send, receive) = tokio::sync::oneshot::channel();
        self.backend.send(MessageToBackend::SetInstanceGlobalSharing {
            id: self.id,
            configuration,
            channel: send,
        });
        self.busy = true;
        self.error = None;
        cx.notify();
        cx.spawn_in(window, async move |entity, cx| {
            let result = receive.await.unwrap_or_else(|_| Err("Launcher backend stopped".into()));
            _ = entity.update_in(cx, |state, window, cx| {
                state.busy = false;
                match result {
                    Ok(()) => window.close_dialog(cx),
                    Err(error) => {
                        log::warn!("Unable to save instance sharing: {error}");
                        state.error =
                            Some(if error == "Detén las instancias afectadas antes de cambiar lo que comparten." {
                                error
                            } else {
                                "No se ha podido cambiar la sincronización.".into()
                            });
                        cx.notify();
                    },
                }
            });
        })
        .detach();
    }
    fn render(&mut self, dialog: Dialog, window: &mut Window, cx: &mut Context<Self>) -> Dialog {
        let mut body = v_flex().gap_3();
        if let Some(model) = &self.model {
            let enabled = model.configuration.enabled;
            body = body.child(
                h_flex().w_full().items_center().justify_between().child("Sobreescribir ajustes").child(
                    design::YesNo::new("instance-global-master")
                        .checked(enabled)
                        .disabled(self.busy)
                        .on_click(cx.listener(|state, value, _, cx| {
                            if let Some(model) = &mut state.model {
                                model.configuration.enabled = *value;
                            }
                            cx.notify();
                        })),
                ),
            );
            let catalog = schema::shared_data::global_catalog(&model.global);
            for (heading, file) in [("Archivos", true), ("Carpetas", false)] {
                let entries: Vec<_> = catalog.iter().filter(|(_, is_file)| **is_file == file).collect();
                if entries.is_empty() {
                    continue;
                }
                body = body.child(div().text_sm().child(heading));
                for (path, is_file) in entries {
                    let global = if *is_file {
                        model.global.files.contains(path.as_str())
                    } else {
                        model.global.folders.contains(path.as_str())
                    };
                    let choice = enabled.then(|| model.configuration.choices.get(path).copied()).flatten();
                    let mut controls = h_flex().flex_shrink_0();
                    for (index, value, label) in [
                        (0usize, Some(false), "NO"),
                        (1, None, if global { "G(SÍ)" } else { "G(NO)" }),
                        (2, Some(true), "SÍ"),
                    ] {
                        let selected = choice == value;
                        let selected_global = selected && value.is_none();
                        let color: Hsla = if selected_global {
                            cx.theme().warning
                        } else if selected {
                            rgb(theme::ACCENT_TEXT).into()
                        } else {
                            rgb(theme::INK_MUTED).into()
                        };
                        let mut yellow = cx.theme().warning;
                        yellow.a = 0.14;
                        let background: Hsla = if selected_global {
                            yellow
                        } else if selected {
                            rgba(0x16445366).into()
                        } else {
                            rgba(0xffffff04).into()
                        };
                        let path = path.clone();
                        controls = controls.child(
                            Button::new((SharedString::from(format!("instance-global-{path}")), index))
                                .ghost()
                                .rounded(px(0.))
                                .label(label)
                                .text_color(color)
                                .bg(background)
                                .disabled(!enabled || self.busy)
                                .on_click(cx.listener(move |state, _, _, cx| {
                                    if let Some(model) = &mut state.model {
                                        if let Some(value) = value {
                                            model.configuration.choices.insert(path.clone(), value);
                                        } else {
                                            model.configuration.choices.remove(&path);
                                        }
                                    }
                                    cx.notify();
                                })),
                        );
                    }
                    body = body.child(
                        h_flex()
                            .w_full()
                            .gap_3()
                            .items_center()
                            .justify_between()
                            .child(div().min_w_0().flex_1().child(path.clone()))
                            .child(controls),
                    );
                }
            }
        } else if self.error.is_none() {
            body = body.child(gpui_component::spinner::Spinner::new());
        }
        if let Some(error) = &self.error {
            body = body.child(div().text_sm().child(error.clone()));
        }
        design::dialog(dialog)
            .title(design::dialog_title("Gestionar global"))
            .width(px((window.viewport_size().width.as_f32() - 48.).min(720.)))
            .child(
                div()
                    .id("instance-global-sharing-body")
                    .max_h(px((window.viewport_size().height.as_f32() - 180.).clamp(180., 520.)))
                    .overflow_y_scroll()
                    .child(body),
            )
            .footer(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("cancel-instance-global-sharing")
                            .label("Cancelar")
                            .disabled(self.busy)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("save-instance-global-sharing")
                            .primary()
                            .label("Guardar")
                            .disabled(self.busy || self.model.is_none())
                            .on_click(cx.listener(|state, _, window, cx| state.save(window, cx))),
                    ),
            )
    }
}
pub fn open(id: InstanceID, backend: BackendHandle, window: &mut Window, cx: &mut App) {
    let state = cx.new(|_| GlobalSharingModal {
        id,
        backend,
        model: None,
        busy: false,
        error: None,
    });
    state.update(cx, |state, cx| state.load(cx));
    window.open_dialog(cx, move |dialog, window, cx| {
        cx.update_entity(&state, |state, cx| state.render(dialog, window, cx))
    });
}
