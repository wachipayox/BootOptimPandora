use crate::{component::design, icon::PandoraIcon};
use bridge::{
    handle::BackendHandle,
    instance::InstanceID,
    message::{MessageToBackend, SaveGroupSummary},
};
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Disableable, StyledExt, WindowExt,
    button::{Button, ButtonVariants},
    dialog::Dialog,
    h_flex,
    input::InputState,
    v_flex,
};
use schema::backend_config::SyncTargets;
use uuid::Uuid;

struct SaveGroupsModal {
    id: InstanceID,
    backend: BackendHandle,
    name: Entity<InputState>,
    rename: Option<(Uuid, Entity<InputState>)>,
    editing: Option<(Uuid, String, SyncTargets)>,
    custom_path: Entity<InputState>,
    groups: Option<Result<Vec<SaveGroupSummary>, String>>,
    status: Option<String>,
    busy: bool,
}

#[derive(Clone, Copy)]
enum Operation {
    Create,
    Join(Uuid),
    Leave,
    Rename(Uuid),
    SaveTargets(Uuid),
}

impl SaveGroupsModal {
    fn load(&mut self, cx: &mut Context<Self>) {
        let (send, receive) = tokio::sync::oneshot::channel();
        self.backend.send(MessageToBackend::GetSaveGroups {
            id: self.id,
            channel: send,
        });
        cx.spawn(async move |entity, cx| {
            let result = receive.await.unwrap_or_else(|_| Err("Launcher backend stopped".into()));
            let _ = entity.update(cx, |state, cx| {
                state.groups = Some(result);
                cx.notify();
            });
        })
        .detach();
    }

    fn run(&mut self, operation: Operation, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.status = None;
        cx.notify();
        let id = self.id;
        let backend = self.backend.clone();
        let name = match operation {
            Operation::Rename(_) => self
                .rename
                .as_ref()
                .map(|(_, input)| input.read(cx).value().to_string())
                .unwrap_or_default(),
            _ => self.name.read(cx).value().to_string(),
        };
        let targets = self.editing.as_ref().map(|(_, _, targets)| targets.clone()).unwrap_or_default();
        cx.spawn(async move |entity, cx| {
            let (send, receive) = tokio::sync::oneshot::channel();
            match operation {
                Operation::Create => backend.send(MessageToBackend::CreateSaveGroup {
                    id,
                    name,
                    channel: send,
                }),
                Operation::Join(group_id) => backend.send(MessageToBackend::JoinSaveGroup {
                    id,
                    group_id,
                    channel: send,
                }),
                Operation::Leave => backend.send(MessageToBackend::LeaveSaveGroup { id, channel: send }),
                Operation::SaveTargets(group_id) => backend.send(MessageToBackend::SetSaveGroupTargets {
                    group_id,
                    targets,
                    channel: send,
                }),
                Operation::Rename(group_id) => backend.send(MessageToBackend::RenameSaveGroup {
                    group_id,
                    name,
                    channel: send,
                }),
            }
            let result = receive.await.unwrap_or_else(|_| Err("Launcher backend stopped".into()));
            let message = result.as_ref().err().map(|error| {
                log::warn!("Save group operation failed: {error}");
                if error == "Detén las instancias afectadas antes de cambiar lo que comparten." {
                    error.clone()
                } else {
                    "No se ha podido cambiar el grupo de mundos.".to_owned()
                }
            });
            let groups = {
                let (send, receive) = tokio::sync::oneshot::channel();
                backend.send(MessageToBackend::GetSaveGroups { id, channel: send });
                Some(receive.await.unwrap_or_else(|_| Err("Launcher backend stopped".into())))
            };
            let _ = entity.update(cx, |state, cx| {
                state.busy = false;
                state.status = message;
                if result.is_ok() {
                    state.rename = None;
                    state.editing = None;
                }
                if let Some(groups) = groups {
                    state.groups = Some(groups);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn render_editor(&mut self, group_id: Uuid, targets: SyncTargets, cx: &mut Context<Self>) -> AnyElement {
        let mut body = v_flex().gap_3();
        for (heading, file, defaults) in [
            ("Archivos", true, schema::shared_data::GROUP_FILES),
            ("Carpetas", false, schema::shared_data::GROUP_FOLDERS),
        ] {
            let selected = if file { &targets.files } else { &targets.folders };
            let mut paths: std::collections::BTreeSet<String> = defaults.iter().map(|p| (*p).to_owned()).collect();
            paths.extend(selected.iter().map(|p| p.to_string()));
            body = body.child(div().text_sm().child(heading));
            for path in paths {
                let enabled = selected.contains(path.as_str());
                let key = path.clone();
                body = body.child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .child(div().min_w_0().flex_1().overflow_x_hidden().child(if path == "saves" {
                            "Mundos (saves)".to_owned()
                        } else {
                            path.clone()
                        }))
                        .child(
                            design::YesNo::new(format!("group-target-{file}-{path}"))
                                .checked(enabled)
                                .disabled(self.busy)
                                .on_click(cx.listener(move |state, checked, _, cx| {
                                    if let Some((_, _, targets)) = &mut state.editing {
                                        let selected = if file { &mut targets.files } else { &mut targets.folders };
                                        if *checked {
                                            selected.insert(key.clone().into());
                                        } else {
                                            selected.remove(key.as_str());
                                        }
                                        cx.notify();
                                    }
                                })),
                        ),
                );
            }
        }
        let mut custom = h_flex()
            .w_full()
            .items_center()
            .gap_2()
            .child(design::text_field(&self.custom_path).disabled(self.busy).flex_1().min_w_0());
        for (index, label, file) in [(0usize, "Archivo propio…", true), (1, "Carpeta propia…", false)] {
            custom = custom.child(
                design::DesignButton::new(("group-custom-target", index))
                    .label(label)
                    .disabled(self.busy)
                    .on_click(cx.listener(move |state, _, window, cx| {
                        let path = state.custom_path.read(cx).value().trim().to_owned();
                        if path.is_empty() || bridge::safe_path::SafePath::new(&path).is_none() {
                            return;
                        }
                        if let Some((_, _, targets)) = &mut state.editing {
                            let (selected, other) = if file {
                                (&mut targets.files, &mut targets.folders)
                            } else {
                                (&mut targets.folders, &mut targets.files)
                            };
                            other.remove(path.as_str());
                            selected.insert(path.into());
                        }
                        state.custom_path.update(cx, |input, cx| input.set_value("", window, cx));
                        cx.notify();
                    })),
            );
        }
        body.child(custom)
            .when(self.busy, |body| body.child(gpui_component::spinner::Spinner::new()))
            .when_some(self.status.clone(), |body, status| body.child(div().text_sm().child(status)))
            .child(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .child(Button::new("cancel-group-targets").label("Cancelar").disabled(self.busy).on_click(
                        cx.listener(|state, _, _, cx| {
                            state.editing = None;
                            state.status = None;
                            cx.notify();
                        }),
                    ))
                    .child(
                        Button::new("save-group-targets").primary().label("Guardar").disabled(self.busy).on_click(
                            cx.listener(move |state, _, _, cx| state.run(Operation::SaveTargets(group_id), cx)),
                        ),
                    ),
            )
            .into_any_element()
    }

    fn render(&mut self, modal: Dialog, window: &mut Window, cx: &mut Context<Self>) -> Dialog {
        let body = if let Some((id, _, targets)) = self.editing.clone() {
            self.render_editor(id, targets, cx)
        } else {
            match &self.groups {
                None => v_flex().gap_3().child(gpui_component::spinner::Spinner::new()).into_any_element(),
                Some(Err(error)) => {
                    log::warn!("Save group loading failed: {error}");
                    v_flex().gap_3().child("No se han podido cargar los grupos de mundos.").into_any_element()
                },
                Some(Ok(groups)) => {
                    let rows = groups
                        .iter()
                        .cloned()
                        .map(|group| {
                            let selected = group.selected;
                            let id = group.id;
                            let member_label = format!(
                                "{} {}",
                                group.member_count,
                                if group.member_count == 1 {
                                    "instancia"
                                } else {
                                    "instancias"
                                }
                            );
                            h_flex()
                                .w_full()
                                .gap_3()
                                .items_center()
                                .justify_between()
                                .p_3()
                                .border_1()
                                .border_color(cx.theme().border)
                                .rounded(px(0.))
                                .child(
                                    v_flex()
                                        .min_w_0()
                                        .flex_1()
                                        .gap_1()
                                        .child(
                                            if let Some((editing_id, input)) = &self.rename
                                                && *editing_id == id
                                            {
                                                design::text_field(input).disabled(self.busy).into_any_element()
                                            } else {
                                                div().font_semibold().child(group.name.clone()).into_any_element()
                                            },
                                        )
                                        .child(
                                            div().text_xs().text_color(cx.theme().muted_foreground).child(member_label),
                                        ),
                                )
                                .child(if self.rename.as_ref().is_some_and(|(editing_id, _)| *editing_id == id) {
                                    h_flex()
                                        .gap_2()
                                        .child(
                                            Button::new(format!("save-group-name-{id}"))
                                                .label("Guardar")
                                                .disabled(self.busy)
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.run(Operation::Rename(id), cx)
                                                })),
                                        )
                                        .child(
                                            Button::new(format!("cancel-group-name-{id}"))
                                                .label("Cancelar")
                                                .disabled(self.busy)
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.rename = None;
                                                    cx.notify();
                                                })),
                                        )
                                } else {
                                    h_flex().child(
                                        Button::new(format!("rename-save-group-{id}"))
                                            .label("Renombrar")
                                            .disabled(self.busy)
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                let input = cx.new(|cx| {
                                                    InputState::new(window, cx).default_value(group.name.clone())
                                                });
                                                this.rename = Some((id, input));
                                                cx.notify();
                                            })),
                                    )
                                })
                                .child(
                                    design::DesignButton::new(format!("configure-save-group-{id}"))
                                        .aria_label("Ajustes")
                                        .disabled(self.busy)
                                        .child(gpui_component::Icon::new(PandoraIcon::Settings).size(px(14.)))
                                        .on_click(cx.listener({
                                            let name = group.name.clone();
                                            let targets = group.targets.clone();
                                            move |state, _, _, cx| {
                                                state.editing = Some((id, name.clone(), targets.clone()));
                                                state.status = None;
                                                cx.notify();
                                            }
                                        })),
                                )
                                .child(if selected {
                                    Button::new("leave-save-group")
                                        .danger()
                                        .label("Salir")
                                        .disabled(self.busy)
                                        .on_click(cx.listener(|this, _, _, cx| this.run(Operation::Leave, cx)))
                                } else {
                                    Button::new(format!("join-save-group-{id}"))
                                        .label("Unirse")
                                        .disabled(self.busy)
                                        .on_click(cx.listener(move |this, _, _, cx| this.run(Operation::Join(id), cx)))
                                })
                        })
                        .collect::<Vec<_>>();
                    v_flex().gap_3()
                    .child("Las instancias de un grupo comparten la carpeta saves. Al unirse, se trasladan los mundos de esta instancia; si hay nombres repetidos, se renombran sin sustituir mundos existentes. Detén las instancias del grupo antes de mover mundos.")
                    .children(rows)
                    .child(h_flex().gap_2().items_center()
                        .child(design::text_field(&self.name).disabled(self.busy))
                        .child(Button::new("create-save-group").ghost().rounded(px(0.)).label("Crear grupo")
                            .disabled(self.busy || self.name.read(cx).value().trim().is_empty())
                            .on_click(cx.listener(|this, _, _, cx| this.run(Operation::Create, cx)))))
                    .when(self.busy,|this|this.child(gpui_component::spinner::Spinner::new()))
                    .when_some(self.status.clone(), |this, status| this.child(div().text_sm().child(status)))
                    .into_any_element()
                },
            }
        };
        let state = cx.entity();
        design::dialog(modal)
            .title(design::dialog_title(
                self.editing
                    .as_ref()
                    .map(|(_, name, _)| name.clone())
                    .unwrap_or_else(|| "Grupos de mundos".to_owned()),
            ))
            .width(px((window.viewport_size().width.as_f32() - 48.0).min(720.0)))
            .on_ok(move |_, _, cx| {
                state.update(cx, |this, cx| {
                    if let Some((id, _)) = &this.rename {
                        this.run(Operation::Rename(*id), cx);
                    }
                });
                false
            })
            .child(
                div()
                    .id("save-groups-body")
                    .max_h(px((window.viewport_size().height.as_f32() - 180.0).clamp(180.0, 520.0)))
                    .overflow_y_scroll()
                    .child(body),
            )
            .footer(
                h_flex().w_full().justify_end().child(
                    Button::new("close-save-groups")
                        .label("Cerrar")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                ),
            )
    }
}

pub fn open_save_groups(id: InstanceID, backend: BackendHandle, window: &mut Window, cx: &mut App) {
    let state = cx.new(|cx| SaveGroupsModal {
        id,
        backend: backend.clone(),
        name: cx.new(|cx| InputState::new(window, cx).placeholder("Nuevo grupo")),
        rename: None,
        editing: None,
        custom_path: cx.new(|cx| InputState::new(window, cx)),
        groups: None,
        status: None,
        busy: false,
    });
    state.update(cx, |modal, cx| modal.load(cx));
    window.open_dialog(cx, move |dialog, window, cx| {
        cx.update_entity(&state, |state, cx| state.render(dialog, window, cx))
    });
}
