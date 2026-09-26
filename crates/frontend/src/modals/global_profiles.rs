use bridge::{
    handle::BackendHandle,
    message::{GlobalProfileSummary, MessageToBackend},
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

struct GlobalProfilesModal {
    backend_handle: BackendHandle,
    name_input: Entity<InputState>,
    profiles: Option<Result<Vec<GlobalProfileSummary>, String>>,
    _load_task: Option<Task<()>>,
}

impl GlobalProfilesModal {
    fn render(&mut self, modal: Dialog, window: &mut Window, cx: &mut Context<Self>) -> Dialog {
        let body = match &self.profiles {
            None => v_flex()
                .gap_3()
                .child("Connecting to the private Distribution service…")
                .into_any_element(),
            Some(Err(error)) => v_flex()
                .gap_3()
                .child("Unable to load global profiles")
                .child(div().text_sm().child(error.clone()))
                .into_any_element(),
            Some(Ok(profiles)) if profiles.is_empty() => {
                v_flex().gap_3().child("No global profiles have been published yet.").into_any_element()
            },
            Some(Ok(profiles)) => {
                let name = self.name_input.read(cx).value().to_string();
                let rows = profiles
                    .iter()
                    .cloned()
                    .map(|profile| {
                        let profile_id = profile.profile_id.clone();
                        let (revision_id, sequence, digest) = match (
                            profile.stable_revision_id.as_ref(),
                            profile.stable_sequence,
                            profile.stable_manifest_sha256.as_ref(),
                        ) {
                            (Some(id), Some(sequence), Some(digest)) => (id.clone(), sequence, digest.clone()),
                            _ => (
                                profile.latest_revision_id.clone(),
                                profile.latest_sequence,
                                profile.latest_manifest_sha256.clone(),
                            ),
                        };
                        let display_name = if name.trim().is_empty() {
                            profile.name.clone()
                        } else {
                            name.clone()
                        };
                        let valid_name = crate::is_valid_instance_name(display_name.trim());
                        let title = profile.name.clone();
                        let id_label = profile_id.clone();
                        let backend = self.backend_handle.clone();
                        h_flex()
                            .w_full()
                            .gap_3()
                            .justify_between()
                            .items_center()
                            .p_3()
                            .border_1()
                            .border_color(cx.theme().border)
                            .rounded_md()
                            .child(
                                v_flex().gap_1().child(div().font_semibold().child(title)).child(
                                    div()
                                        .text_xs()
                                        .child(format!("{} · revision {} · {}", id_label, sequence, &revision_id)),
                                ),
                            )
                            .child(
                                Button::new(format!("create-global-{}", profile.profile_id))
                                    .success()
                                    .label("Create instance")
                                    .disabled(!valid_name)
                                    .on_click(cx.listener(move |_, _, window, cx| {
                                        backend.send(MessageToBackend::CreateGlobalProfileInstance {
                                            name: display_name.clone(),
                                            profile_id: profile.profile_id.clone(),
                                            revision_id: revision_id.clone(),
                                            sequence,
                                            manifest_sha256: digest.clone(),
                                        });
                                        window.close_dialog(cx);
                                    })),
                            )
                    })
                    .collect::<Vec<_>>();
                v_flex()
                    .gap_3()
                    .child("Instance name")
                    .child(Input::new(&self.name_input))
                    .children(rows)
                    .into_any_element()
            },
        };

        modal.title("Global profiles").child(body).footer(
            h_flex().w_full().justify_end().child(
                Button::new("close-global-profiles")
                    .label("Close")
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            ),
        )
    }
}

pub fn open_global_profiles(backend_handle: BackendHandle, window: &mut Window, cx: &mut App) {
    let state = cx.new(|cx| GlobalProfilesModal {
        backend_handle: backend_handle.clone(),
        name_input: cx.new(|cx| InputState::new(window, cx).placeholder("New instance name")),
        profiles: None,
        _load_task: None,
    });
    let (send, receive) = tokio::sync::oneshot::channel();
    backend_handle.send(MessageToBackend::GetGlobalProfiles { channel: send });
    let task_state = state.clone();
    let task = cx.spawn(async move |cx| {
        let result = receive
            .await
            .unwrap_or_else(|_| Err("The launcher backend stopped before the request completed".into()));
        _ = cx.update_entity(&task_state, |modal, cx| {
            modal.profiles = Some(result);
            cx.notify();
        });
    });
    state.update(cx, |modal, _| modal._load_task = Some(task));

    window.open_dialog(cx, move |modal, window, cx| {
        cx.update_entity(&state, |state, cx| state.render(modal, window, cx))
    });
}
