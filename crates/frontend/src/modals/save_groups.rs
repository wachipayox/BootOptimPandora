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
    input::{Input, InputState},
    v_flex,
};
use uuid::Uuid;

struct SaveGroupsModal {
    id: InstanceID,
    backend: BackendHandle,
    name: Entity<InputState>,
    rename: Option<(Uuid, Entity<InputState>)>,
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
        self.status = Some("Updating save group…".into());
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
                Operation::Rename(group_id) => backend.send(MessageToBackend::RenameSaveGroup {
                    group_id,
                    name,
                    channel: send,
                }),
            }
            let result = receive.await.unwrap_or_else(|_| Err("Launcher backend stopped".into()));
            let message = match &result {
                Ok(()) => "Save group updated".to_string(),
                Err(error) => error.clone(),
            };
            let groups = if result.is_ok() {
                let (send, receive) = tokio::sync::oneshot::channel();
                backend.send(MessageToBackend::GetSaveGroups { id, channel: send });
                Some(receive.await.unwrap_or_else(|_| Err("Launcher backend stopped".into())))
            } else {
                None
            };
            let _ = entity.update(cx, |state, cx| {
                state.busy = false;
                state.status = Some(message);
                if result.is_ok() {
                    state.rename = None;
                }
                if let Some(groups) = groups {
                    state.groups = Some(groups);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn render(&mut self, modal: Dialog, window: &mut Window, cx: &mut Context<Self>) -> Dialog {
        let body = match &self.groups {
            None => v_flex().gap_3().child("Loading save groups…").into_any_element(),
            Some(Err(error)) => v_flex().gap_3().child(error.clone()).into_any_element(),
            Some(Ok(groups)) => {
                let rows = groups
                    .iter()
                    .cloned()
                    .map(|group| {
                        let selected = group.selected;
                        let id = group.id;
                        let member_label = if selected {
                            format!("{} instance(s) · current group", group.member_count)
                        } else {
                            format!("{} instance(s)", group.member_count)
                        };
                        h_flex()
                            .w_full()
                            .gap_3()
                            .items_center()
                            .justify_between()
                            .p_3()
                            .border_1()
                            .border_color(cx.theme().border)
                            .rounded_md()
                            .child(
                                v_flex()
                                    .min_w_0()
                                    .flex_1()
                                    .gap_1()
                                    .child(
                                        if let Some((editing_id, input)) = &self.rename
                                            && *editing_id == id
                                        {
                                            Input::new(input).disabled(self.busy).into_any_element()
                                        } else {
                                            div().font_semibold().child(group.name.clone()).into_any_element()
                                        },
                                    )
                                    .child(div().text_xs().text_color(cx.theme().muted_foreground).child(member_label)),
                            )
                            .child(if self.rename.as_ref().is_some_and(|(editing_id, _)| *editing_id == id) {
                                h_flex()
                                    .gap_2()
                                    .child(
                                        Button::new(format!("save-group-name-{id}"))
                                            .label("Save")
                                            .disabled(self.busy)
                                            .on_click(
                                                cx.listener(move |this, _, _, cx| this.run(Operation::Rename(id), cx)),
                                            ),
                                    )
                                    .child(
                                        Button::new(format!("cancel-group-name-{id}"))
                                            .label("Cancel")
                                            .disabled(self.busy)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.rename = None;
                                                cx.notify();
                                            })),
                                    )
                            } else {
                                h_flex().child(
                                    Button::new(format!("rename-save-group-{id}"))
                                        .label("Rename")
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
                            .child(if selected {
                                Button::new("leave-save-group")
                                    .danger()
                                    .label("Leave group")
                                    .disabled(self.busy)
                                    .on_click(cx.listener(|this, _, _, cx| this.run(Operation::Leave, cx)))
                            } else {
                                Button::new(format!("join-save-group-{id}"))
                                    .label("Join group")
                                    .disabled(self.busy)
                                    .on_click(cx.listener(move |this, _, _, cx| this.run(Operation::Join(id), cx)))
                            })
                    })
                    .collect::<Vec<_>>();
                v_flex().gap_3()
                    .child("Instances in a group use the same saves folder. Joining moves this instance’s worlds; name collisions are renamed without replacing existing worlds. Stop running group members before moving worlds.")
                    .children(rows)
                    .child(h_flex().gap_2().items_center()
                        .child(Input::new(&self.name).disabled(self.busy))
                        .child(Button::new("create-save-group").success().label("Create group")
                            .disabled(self.busy || self.name.read(cx).value().trim().is_empty())
                            .on_click(cx.listener(|this, _, _, cx| this.run(Operation::Create, cx)))))
                    .when_some(self.status.clone(), |this, status| this.child(div().text_sm().child(status)))
                    .into_any_element()
            },
        };
        let state = cx.entity();
        modal
            .title("Shared save groups")
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
                        .label("Close")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                ),
            )
    }
}

pub fn open_save_groups(id: InstanceID, backend: BackendHandle, window: &mut Window, cx: &mut App) {
    let state = cx.new(|cx| SaveGroupsModal {
        id,
        backend: backend.clone(),
        name: cx.new(|cx| InputState::new(window, cx).placeholder("Group name")),
        rename: None,
        groups: None,
        status: None,
        busy: false,
    });
    state.update(cx, |modal, cx| modal.load(cx));
    window.open_dialog(cx, move |dialog, window, cx| {
        cx.update_entity(&state, |state, cx| state.render(dialog, window, cx))
    });
}
