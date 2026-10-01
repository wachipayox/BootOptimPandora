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
                                v_flex()
                                    .min_w_0()
                                    .flex_1()
                                    .gap_1()
                                    .child(div().truncate().font_semibold().child(title))
                                    .child(div().text_sm().child(profile.description.clone()))
                                    .child(div().text_sm().child(format!("Minecraft {} · NeoForge {}", profile.minecraft, profile.neoforge)))
                                    .child(
                                        div()
                                            .truncate()
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
                    .child("Name for the new instance")
                    .child(Input::new(&self.name_input))
                    .child(div().text_xs().text_color(cx.theme().muted_foreground).child(
                        "Leave blank to use the global profile name. Choose a different name if you already have an instance with that name.",
                    ))
                    .children(rows)
                    .into_any_element()
            },
        };

        modal.title("Global profiles").width(px(720.0)).child(body).footer(
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
        name_input: cx.new(|cx| InputState::new(window, cx).placeholder("Optional instance name")),
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

pub fn open_global_profile_details(profile: GlobalProfileSummary, backend_handle: BackendHandle, window: &mut Window, cx: &mut App) {
    let input = cx.new(|cx| InputState::new(window, cx).placeholder(profile.name.clone()));
    window.open_dialog(cx, move |dialog, _, _| {
        let name_input = input.clone();
        let create_profile = profile.clone();
        let backend = backend_handle.clone();
        let enter_name_input = input.clone();
        let enter_profile = profile.clone();
        let enter_backend = backend_handle.clone();
        let dialog = dialog.on_ok(move |_, _, cx| {
            let name = enter_name_input.read(cx).value();
            let name = if name.trim().is_empty() { enter_profile.name.clone() } else { name.trim().to_owned() };
            if !crate::is_valid_instance_name(&name) { return false; }
            let (revision_id, sequence, manifest_sha256) = match (
                &enter_profile.stable_revision_id,
                enter_profile.stable_sequence,
                &enter_profile.stable_manifest_sha256,
            ) {
                (Some(id), Some(sequence), Some(digest)) => (id.clone(), sequence, digest.clone()),
                _ => (
                    enter_profile.latest_revision_id.clone(),
                    enter_profile.latest_sequence,
                    enter_profile.latest_manifest_sha256.clone(),
                ),
            };
            enter_backend.send(MessageToBackend::CreateGlobalProfileInstance {
                name,
                profile_id: enter_profile.profile_id.clone(),
                revision_id,
                sequence,
                manifest_sha256,
            });
            true
        });
        let icon = match &profile.icon_path {
            Some(path) => gpui::img(path.clone()).size_24().rounded_lg().into_any_element(),
            None => gpui::img(ImageSource::Resource(Resource::Embedded("images/default_mod.png".into()))).size_24().into_any_element(),
        };
        dialog.title(profile.name.clone()).width(px(760.0))
            .child(v_flex().gap_5()
                .child(h_flex().gap_4().items_center().child(icon).child(v_flex().gap_2()
                    .child(format!("Minecraft {}", profile.minecraft))
                    .child(format!("NeoForge {}", profile.neoforge))
                    .child(format!("Version {}", profile.stable_sequence.unwrap_or(profile.latest_sequence)))))
                .child(div().child(if profile.description.is_empty() { "No description available".to_owned() } else { profile.description.clone() }))
                .child(crate::labelled("Instance name", Input::new(&input))))
            .footer(h_flex().gap_3().w_full().justify_end()
                .child(Button::new("close").label("Close").on_click(|_, window, cx| window.close_dialog(cx)))
                .child(Button::new("create-global").success().label("Create instance").on_click(move |_, window, cx| {
                    let name = name_input.read(cx).value();
                    let name = if name.trim().is_empty() { create_profile.name.clone() } else { name.trim().to_owned() };
                    if !crate::is_valid_instance_name(&name) { return; }
                    let (revision_id, sequence, manifest_sha256) = match (&create_profile.stable_revision_id, create_profile.stable_sequence, &create_profile.stable_manifest_sha256) {
                        (Some(id), Some(sequence), Some(digest)) => (id.clone(), sequence, digest.clone()),
                        _ => (create_profile.latest_revision_id.clone(), create_profile.latest_sequence, create_profile.latest_manifest_sha256.clone()),
                    };
                    backend.send(MessageToBackend::CreateGlobalProfileInstance { name, profile_id: create_profile.profile_id.clone(), revision_id, sequence, manifest_sha256 });
                    window.close_dialog(cx);
                })))
    });
}
