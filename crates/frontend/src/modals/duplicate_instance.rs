use std::sync::Arc;

use crate::{
    component::design::{self, DesignButton, YesNo},
    entity::instance::InstanceEntries,
    get_unique_instance_name,
    modals::generic,
    theme,
};
use bridge::{handle::BackendHandle, instance::InstanceID, message::MessageToBackend, modal_action::ModalAction};
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Disableable, WindowExt,
    dialog::Dialog,
    h_flex,
    input::InputState,
    slider::{Slider, SliderEvent, SliderState},
    v_flex,
};

struct DuplicateInstanceModalState {
    instance_id: InstanceID,
    backend: BackendHandle,
    name: Entity<InputState>,
    instances: Entity<InstanceEntries>,
    default_name: SharedString,
    as_branch: bool,
    group: Option<Result<bool, String>>,
    create_group: bool,
    reuse_icon: bool,
    hue: Entity<SliderState>,
}
impl DuplicateInstanceModalState {
    fn render(&mut self, dialog: Dialog, _window: &mut Window, cx: &mut Context<Self>) -> Dialog {
        let name = self.name.read(cx).value();
        let name = if name.trim().is_empty() {
            self.default_name.to_string()
        } else {
            name.trim().to_owned()
        };
        let valid = crate::is_valid_instance_name(&name)
            && !self.instances.read(cx).entries.values().any(|i| i.read(cx).name.as_str() == name);
        let ready = !self.as_branch || matches!(self.group, Some(Ok(_)));
        let enter_backend = self.backend.clone();
        let enter_id = self.instance_id;
        let enter_name = name.clone();
        let enter_as_branch = self.as_branch;
        let enter_create_group = self.create_group;
        let enter_reuse_icon = self.reuse_icon;
        let enter_hue_degrees = match self.hue.read(cx).value() {
            gpui_component::slider::SliderValue::Single(value) => value.round() as i32 - 180,
            gpui_component::slider::SliderValue::Range(value, _) => value.round() as i32 - 180,
        };
        let dialog = dialog.on_ok(move |_, window, cx| {
            if !valid || !ready {
                return false;
            }
            let modal_action = ModalAction::default();
            window.close_dialog(cx);
            generic::show_modal(
                window,
                cx,
                if enter_as_branch {
                    "Creando instancia hija…".into()
                } else {
                    "Duplicando instancia…".into()
                },
                if enter_as_branch {
                    "No se ha podido crear la instancia hija.".into()
                } else {
                    "No se ha podido duplicar la instancia.".into()
                },
                modal_action.clone(),
            );
            if enter_as_branch {
                enter_backend.send(MessageToBackend::CreateLocalBranch {
                    id: enter_id,
                    name: enter_name.as_str().into(),
                    create_save_group: enter_create_group,
                    reuse_parent_icon: enter_reuse_icon,
                    icon_hue_degrees: enter_hue_degrees,
                    modal_action,
                });
            } else {
                enter_backend.send(MessageToBackend::DuplicateInstance {
                    id: enter_id,
                    name: enter_name.as_str().into(),
                    modal_action,
                });
            }
            // Close this dialog before opening the progress dialog; leave it to the explicit
            // close above instead of letting the default confirmation close the new dialog.
            false
        });
        let mut content = v_flex().gap_3().child(crate::labelled("Nombre", design::text_field(&self.name)));
        if self.as_branch {
            let sharing = matches!(self.group, Some(Ok(true)));
            content = content
                .child(
                    h_flex()
                        .w_full()
                        .justify_between()
                        .gap(px(12.))
                        .child(div().flex_1().child("Compartir los mundos de la madre"))
                        .child(
                            YesNo::new("branch-share-worlds")
                                .checked(sharing || self.create_group)
                                .disabled(sharing || self.group.is_none())
                                .on_click(cx.listener(|this, value, _, cx| {
                                    this.create_group = *value;
                                    cx.notify();
                                })),
                        ),
                )
                .when(matches!(self.group, Some(Ok(false))) && self.create_group, |d| {
                    d.child(
                        div()
                            .text_size(px(12.))
                            .text_color(rgb(theme::INK_MUTED))
                            .child("La madre no tiene grupo de mundos: se crea uno para las dos."),
                    )
                })
                .when(matches!(self.group, Some(Err(_))), |d| {
                    d.child(div().text_color(rgb(theme::DANGER)).child("No se ha podido crear la instancia hija."))
                });
            content = content.child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .gap(px(12.))
                    .child(div().flex_1().child("Usar el icono de la madre"))
                    .child(YesNo::new("branch-reuse-icon").checked(self.reuse_icon).on_click(cx.listener(
                        |this, value, _, cx| {
                            this.reuse_icon = *value;
                            cx.notify();
                        },
                    ))),
            );
            if self.reuse_icon {
                let hue_value = match self.hue.read(cx).value() {
                    gpui_component::slider::SliderValue::Single(v) => v,
                    gpui_component::slider::SliderValue::Range(v, _) => v,
                };
                let hue_degrees = hue_value.round() as i32 - 180;
                let parent_icon = self
                    .instances
                    .read(cx)
                    .entries
                    .get(&self.instance_id)
                    .and_then(|instance| instance.read(cx).icon.clone());
                let icon_preview = parent_icon.and_then(|bytes| {
                    let image = image::load_from_memory(&bytes).ok()?.huerotate(hue_degrees);
                    let mut encoded = std::io::Cursor::new(Vec::new());
                    image.write_to(&mut encoded, image::ImageFormat::Png).ok()?;
                    Some(Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Png, encoded.into_inner())))
                });
                let preview = match icon_preview {
                    Some(image) => gpui::img(image).size_8().into_any_element(),
                    None => div().size_8().bg(cx.theme().muted).into_any_element(),
                };
                let hue_row = h_flex()
                    .gap_2()
                    .items_center()
                    .child(preview)
                    .child(Slider::new(&self.hue).flex_1())
                    .child(
                        div()
                            .w(px(56.0))
                            .flex_shrink_0()
                            .whitespace_nowrap()
                            .child(format!("{}°", hue_value.round() as i32)),
                    )
                    .child(DesignButton::new("randomize-parent-icon-hue").label("Aleatorio").on_click(cx.listener(
                        |this, _, window, cx| {
                            let nanos = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .subsec_nanos();
                            let value = (nanos % 361) as f32;
                            this.hue.update(cx, |slider, cx| slider.set_value(value, window, cx));
                            cx.notify();
                        },
                    )));
                content = content
                    .child(crate::labelled("Tono del icono", hue_row))
                    .child(div().text_size(px(12.)).text_color(rgb(theme::INK_MUTED)).child("Neutro a 180°"));
            }
        }
        let parent_name = self
            .instances
            .read(cx)
            .entries
            .get(&self.instance_id)
            .map(|i| i.read(cx).name.clone())
            .unwrap_or_default();
        let content = content
            .when(!valid, |d| {
                d.child(div().text_color(rgb(theme::DANGER)).child("El nombre no es válido o ya existe."))
            })
            .when(!self.as_branch, |d| d.child("Copia la instancia entera, mundos incluidos."));
        design::dialog(dialog)
            .w(px(560.))
            .title(
                v_flex()
                    .gap(px(6.))
                    .when(self.as_branch, |d| d.child(design::eyebrow(parent_name)))
                    .child(design::dialog_title(if self.as_branch {
                        "Crear hija"
                    } else {
                        "Duplicar instancia"
                    })),
            )
            .overlay_closable(false)
            .child(content)
            .footer(
                h_flex()
                    .gap_2()
                    .w_full()
                    .justify_end()
                    .child(
                        DesignButton::new("cancel")
                            .label("Cancelar")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        design::action_button(
                            "create",
                            if self.as_branch { "Crear hija" } else { "Duplicar" },
                            design::ButtonAction::Primary,
                        )
                        .disabled(!valid || !ready)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            let modal_action = ModalAction::default();
                            window.close_dialog(cx);
                            generic::show_modal(
                                window,
                                cx,
                                if this.as_branch {
                                    "Creando instancia hija…".into()
                                } else {
                                    "Duplicando instancia…".into()
                                },
                                if this.as_branch {
                                    "No se ha podido crear la instancia hija.".into()
                                } else {
                                    "No se ha podido duplicar la instancia.".into()
                                },
                                modal_action.clone(),
                            );
                            if this.as_branch {
                                this.backend.send(MessageToBackend::CreateLocalBranch {
                                    id: this.instance_id,
                                    name: name.as_str().into(),
                                    create_save_group: this.create_group,
                                    reuse_parent_icon: this.reuse_icon,
                                    icon_hue_degrees: {
                                        let value = match this.hue.read(cx).value() {
                                            gpui_component::slider::SliderValue::Single(v) => v,
                                            gpui_component::slider::SliderValue::Range(v, _) => v,
                                        };
                                        value.round() as i32 - 180
                                    },
                                    modal_action,
                                });
                            } else {
                                this.backend.send(MessageToBackend::DuplicateInstance {
                                    id: this.instance_id,
                                    name: name.as_str().into(),
                                    modal_action,
                                });
                            }
                        })),
                    ),
            )
    }
}
pub fn open_duplicate_instance(
    id: InstanceID,
    name: SharedString,
    instances: Entity<InstanceEntries>,
    backend: BackendHandle,
    window: &mut Window,
    cx: &mut App,
) {
    open_instance_copy(id, name, instances, backend, false, window, cx);
}
pub fn open_derived_instance(
    id: InstanceID,
    name: SharedString,
    instances: Entity<InstanceEntries>,
    backend: BackendHandle,
    window: &mut Window,
    cx: &mut App,
) {
    open_instance_copy(id, name, instances, backend, true, window, cx);
}
fn open_instance_copy(
    id: InstanceID,
    name: SharedString,
    instances: Entity<InstanceEntries>,
    backend: BackendHandle,
    as_branch: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let names = instances
        .read(cx)
        .entries
        .values()
        .map(|i| i.read(cx).name.to_string())
        .collect::<Vec<_>>();
    let refs = names.iter().map(String::as_str).collect::<Vec<_>>();
    let default_name: SharedString = get_unique_instance_name(name.as_str(), &refs).into();
    let state = cx.new(|cx| DuplicateInstanceModalState {
        instance_id: id,
        backend: backend.clone(),
        name: cx.new(|cx| InputState::new(window, cx).placeholder(default_name.clone())),
        instances,
        default_name,
        as_branch,
        group: None,
        create_group: true,
        reuse_icon: true,
        hue: cx.new(|_| SliderState::new().min(0.0).max(360.0).default_value(180.0)),
    });
    // Repaint validation as the name is edited.
    let input = state.read(cx).name.clone();
    cx.subscribe(&input, {
        let state = state.clone();
        move |_, _: &gpui_component::input::InputEvent, cx| {
            state.update(cx, |_, cx| cx.notify());
        }
    })
    .detach();
    if as_branch {
        let hue = state.read(cx).hue.clone();
        cx.subscribe(&hue, {
            let state = state.clone();
            move |_, _: &SliderEvent, cx| {
                state.update(cx, |_, cx| cx.notify());
            }
        })
        .detach();
        let (send, receive) = tokio::sync::oneshot::channel();
        backend.send(MessageToBackend::GetSaveGroups { id, channel: send });
        let state = state.clone();
        cx.spawn(async move |cx| {
            let result = receive
                .await
                .unwrap_or_else(|_| Err("Launcher backend stopped".into()))
                .map(|groups| groups.iter().any(|g| g.selected));
            _ = cx.update_entity(&state, |state, cx| {
                state.group = Some(result);
                cx.notify();
                log::info!("instance-derived-ready pid={}", std::process::id());
            });
        })
        .detach();
    }
    if !as_branch {
        log::info!("instance-derived-ready pid={}", std::process::id());
    }
    window.open_dialog(cx, move |dialog, window, cx| {
        cx.update_entity(&state, |state, cx| state.render(dialog, window, cx))
    });
}
