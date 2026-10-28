use crate::component::design;
use bridge::{
    handle::BackendHandle,
    instance::InstanceID,
    message::{GlobalProfileSaveGroupTarget, GlobalProfileSummary, MessageToBackend},
    modal_action::ModalAction,
};
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Disableable, Selectable, StyledExt, WindowExt, button::Button, dialog::Dialog, h_flex,
    input::InputState, tooltip::Tooltip, v_flex,
};
use std::collections::BTreeSet;

pub fn family_roots(profiles: &[GlobalProfileSummary]) -> Vec<GlobalProfileSummary> {
    let parents = profiles
        .iter()
        .map(|p| (p.profile_id.clone(), p.parent_profile_id.clone()))
        .collect::<bridge::profile_family::Parents>();
    let roots = parents
        .keys()
        .map(|id| bridge::profile_family::root_id(id, &parents))
        .collect::<BTreeSet<_>>();
    profiles.iter().filter(|p| roots.contains(&p.profile_id)).cloned().collect()
}

/// Marks a test profile (`profile_test-…`), which only the `developer` launcher channel lists.
fn test_label(_cx: &App) -> impl IntoElement {
    div()
        .flex_shrink_0()
        .px(px(6.))
        .py(px(2.))
        .rounded(px(0.))
        .text_size(px(10.))
        .font_semibold()
        .bg(rgba(0x5bcef51f))
        .text_color(rgb(crate::theme::ACCENT_TEXT))
        .child("PRUEBA")
}

pub(crate) fn profile_icon(profile: &GlobalProfileSummary, size: f32) -> AnyElement {
    match &profile.icon_path {
        Some(path) => img(path.clone()).size(px(size)).flex_shrink_0().rounded(px(0.)).into_any_element(),
        None => img(ImageSource::Resource(Resource::Embedded("images/default_mod.png".into())))
            .size(px(size))
            .flex_shrink_0()
            .rounded(px(0.))
            .into_any_element(),
    }
}

struct ProfileFamilyModal {
    profiles: Vec<GlobalProfileSummary>,
    root: String,
    expanded: BTreeSet<String>,
    backend: BackendHandle,
}

impl ProfileFamilyModal {
    fn visible_rows(
        &self,
        id: &str,
        depth: usize,
        seen: &mut BTreeSet<String>,
        rows: &mut Vec<(GlobalProfileSummary, usize)>,
    ) {
        if !seen.insert(id.to_owned()) {
            return;
        }
        let Some(profile) = self.profiles.iter().find(|p| p.profile_id == id) else {
            return;
        };
        rows.push((profile.clone(), depth));
        if self.expanded.contains(id) {
            let mut children = self
                .profiles
                .iter()
                .filter(|p| p.parent_profile_id.as_deref() == Some(id))
                .collect::<Vec<_>>();
            children.sort_by_key(|p| p.name.to_lowercase());
            for child in children {
                self.visible_rows(&child.profile_id, depth + 1, seen, rows);
            }
        }
    }

    fn render(&mut self, dialog: Dialog, window: &mut Window, cx: &mut Context<Self>) -> Dialog {
        let mut rows = Vec::new();
        self.visible_rows(&self.root, 0, &mut BTreeSet::new(), &mut rows);
        let rows = rows
            .into_iter()
            .map(|(profile, depth)| {
                let id = profile.profile_id.clone();
                let has_children = self.profiles.iter().any(|p| p.parent_profile_id.as_deref() == Some(&id));
                let expanded = self.expanded.contains(&id);
                let tooltip_profile = profile.clone();
                let backend = self.backend.clone();
                let mut row = h_flex().w_full().items_center().gap_2().pl(px(depth as f32 * 24.0));
                if depth > 0 {
                    row = row.child(
                        div()
                            .w(px(16.0))
                            .h(px(20.0))
                            .flex_shrink_0()
                            .border_l_1()
                            .border_b_1()
                            .border_color(cx.theme().border),
                    );
                }
                let toggle = if has_children {
                    design::DesignButton::new(format!("expand-{id}"))
                        .label(if expanded { "▾" } else { "▸" })
                        .w(px(28.))
                        .h(px(32.))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !this.expanded.remove(&id) {
                                this.expanded.insert(id.clone());
                            }
                            cx.notify();
                        }))
                        .into_any_element()
                } else {
                    div().w(px(28.)).flex_shrink_0().into_any_element()
                };
                row.child(toggle).child(
                    h_flex()
                        .id(format!("choose-profile-{}", profile.profile_id))
                        .flex_1()
                        .min_w_0()
                        .gap_3()
                        .p_2()
                        .items_center()
                        .rounded(px(0.))
                        .border_1()
                        .border_color(rgba(0xffffff12))
                        .bg(rgba(0xffffff06))
                        .cursor_pointer()
                        .hover(|s| s.bg(cx.theme().secondary))
                        .child(profile_icon(&profile, 36.0))
                        .child(div().font_semibold().truncate().child(profile.name.clone()))
                        .when(bridge::profile_family::is_test_profile(&profile.profile_id), |row| {
                            row.child(test_label(cx))
                        })
                        .tooltip(move |window, cx| {
                            let p = tooltip_profile.clone();
                            Tooltip::element(move |_, _| {
                                v_flex()
                                    .w(px(380.0))
                                    .p_3()
                                    .gap_3()
                                    .child(
                                        h_flex()
                                            .gap_3()
                                            .items_center()
                                            .child(profile_icon(&p, 56.0))
                                            .child(div().font_semibold().child(p.name.clone())),
                                    )
                                    .child(div().text_base().child(if p.description.is_empty() {
                                        String::new()
                                    } else {
                                        p.description.clone()
                                    }))
                            })
                            .build(window, cx)
                        })
                        .on_click(move |_, window, cx| {
                            window.close_dialog(cx);
                            open_global_profile_details(profile.clone(), backend.clone(), window, cx);
                        }),
                )
            })
            .collect::<Vec<_>>();
        design::dialog(dialog)
            .title(design::dialog_title("Perfil global"))
            .width(px((window.viewport_size().width.as_f32() - 48.0).min(760.0)))
            .child(
                v_flex().gap_3().child("Perfiles disponibles en el servidor.").child(
                    v_flex()
                        .id("profile-family-tree")
                        .gap_1()
                        .max_h(px((window.viewport_size().height.as_f32() - 220.0).clamp(180.0, 480.0)))
                        .overflow_y_scroll()
                        .children(rows),
                ),
            )
            .footer(
                h_flex().w_full().justify_end().child(
                    Button::new("close-profile-family")
                        .label("Cancelar")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                ),
            )
    }
}

pub fn open_global_profile_family(
    root: String,
    profiles: Vec<GlobalProfileSummary>,
    backend: BackendHandle,
    window: &mut Window,
    cx: &mut App,
) {
    let state = cx.new(|_| ProfileFamilyModal {
        expanded: BTreeSet::from([root.clone()]),
        root,
        profiles,
        backend,
    });
    window.open_dialog(cx, move |dialog, window, cx| {
        cx.update_entity(&state, |state, cx| state.render(dialog, window, cx))
    });
}

pub fn open_global_profiles(backend: BackendHandle, window: &mut Window, cx: &mut App) {
    let (send, receive) = tokio::sync::oneshot::channel();
    backend.send(MessageToBackend::GetGlobalProfiles { channel: send });
    let handle = window.window_handle();
    cx.spawn(async move |cx| {
        let result = receive.await.unwrap_or_else(|_| Err("Launcher backend stopped".into()));
        _ = cx.update_window(handle, |_, window, cx| match result {
            Ok(profiles) => {
                let roots = family_roots(&profiles);
                window.open_dialog(cx, move |dialog, _, _| {
                    dialog
                        .title("Perfil global")
                        .width(px(680.0))
                        .child(v_flex().gap_2().children(roots.iter().map(|root| {
                            let profiles = profiles.clone();
                            let root_id = root.profile_id.clone();
                            let backend = backend.clone();
                            Button::new(root_id.clone()).label(root.name.clone()).on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                                open_global_profile_family(
                                    root_id.clone(),
                                    profiles.clone(),
                                    backend.clone(),
                                    window,
                                    cx,
                                );
                            })
                        })))
                });
            },
            Err(error) => window.open_dialog(cx, move |dialog, _, _| {
                log::warn!("Global profile loading failed: {error}");
                design::dialog(dialog)
                    .title(design::dialog_title("Perfil global"))
                    .child("No se han podido cargar los perfiles.")
            }),
        });
    })
    .detach();
}

pub(crate) struct ProfileDetailsModal {
    profile: GlobalProfileSummary,
    backend: BackendHandle,
    name: Entity<InputState>,
    targets: Option<Result<Vec<GlobalProfileSaveGroupTarget>, String>>,
    selected_group: Option<InstanceID>,
    submitted: bool,
}

impl ProfileDetailsModal {
    fn load_groups(&mut self, cx: &mut Context<Self>) {
        self.targets = None;
        let (send, receive) = tokio::sync::oneshot::channel();
        self.backend.send(MessageToBackend::GetGlobalProfileSaveGroupTargets {
            profile_id: self.profile.profile_id.clone(),
            channel: send,
        });
        cx.spawn(async move |entity, cx| {
            let result = receive.await.unwrap_or_else(|_| Err("Launcher backend stopped".into()));
            _ = entity.update(cx, |this, cx| {
                this.targets = Some(result);
                cx.notify();
            });
        })
        .detach();
    }
    fn can_submit(&self, cx: &App) -> bool {
        !self.submitted
            && crate::is_valid_instance_name(self.instance_name(cx).trim())
            && self.targets.as_ref().is_some_and(|result| {
                result.as_ref().is_ok_and(|targets| targets.len() <= 1 || self.selected_group.is_some())
            })
    }
    fn instance_name(&self, cx: &App) -> String {
        let name = self.name.read(cx).value();
        if name.trim().is_empty() {
            self.profile.name.clone()
        } else {
            name.trim().to_owned()
        }
    }
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_submit(cx) {
            return;
        }
        let name = self.instance_name(cx);
        self.submitted = true;
        let targets = self.targets.as_ref().and_then(|r| r.as_ref().ok()).unwrap();
        let save_group_target = if targets.len() == 1 {
            Some(targets[0].clone())
        } else {
            targets.iter().find(|target| Some(target.anchor_id) == self.selected_group).cloned()
        };
        let p = &self.profile;
        let (revision_id, sequence, manifest_sha256) =
            match (&p.stable_revision_id, p.stable_sequence, &p.stable_manifest_sha256) {
                (Some(id), Some(seq), Some(digest)) => (id.clone(), seq, digest.clone()),
                _ => (p.latest_revision_id.clone(), p.latest_sequence, p.latest_manifest_sha256.clone()),
            };
        let modal_action = ModalAction::default();
        window.close_dialog(cx);
        crate::modals::generic::show_modal(
            window,
            cx,
            "Crear instancia".into(),
            "No se ha podido importar.".into(),
            modal_action.clone(),
        );
        self.backend.send(MessageToBackend::CreateGlobalProfileInstance {
            name,
            profile_id: p.profile_id.clone(),
            revision_id,
            sequence,
            manifest_sha256,
            modal_action,
            save_group_target,
        });
    }
    fn body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = &self.profile;
        let mut body = v_flex()
            .id("profile-details-body")
            .max_h(px((window.viewport_size().height.as_f32() - 220.0).clamp(180.0, 520.0)))
            .overflow_y_scroll()
            .gap_4()
            .child(
                h_flex().gap_4().items_center().child(profile_icon(p, 80.0)).child(
                    v_flex()
                        .gap_2()
                        .child(format!("Minecraft {} · {} {}", p.minecraft, p.loader.pretty_name(), p.loader_version))
                        .child(format!("rev. {}", p.stable_sequence.unwrap_or(p.latest_sequence))),
                ),
            )
            .child(div().child(if p.description.is_empty() {
                String::new()
            } else {
                p.description.clone()
            }))
            .child(crate::labelled("Nombre de la instancia", design::text_field(&self.name)));
        body = body.child(match &self.targets {
            None => gpui_component::spinner::Spinner::new().into_any_element(),
            Some(Err(_error)) => v_flex()
                .gap_2()
                .child(div().text_color(cx.theme().danger).child("No se han podido cargar los perfiles."))
                .child(
                    Button::new("retry-profile-groups")
                        .label("Reintentar")
                        .on_click(cx.listener(|this, _, _, cx| this.load_groups(cx))),
                )
                .into_any_element(),
            Some(Ok(targets)) if targets.len() > 1 => v_flex()
                .gap_2()
                .child("Las instancias relacionadas usan grupos de mundos distintos. Elige qué mundos compartir:")
                .children(targets.iter().map(|target| {
                    let id = target.anchor_id;
                    Button::new(format!("select-family-group-{id:?}"))
                        .label(target.name.clone())
                        .selected(self.selected_group == Some(id))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.selected_group = Some(id);
                            cx.notify();
                        }))
                }))
                .into_any_element(),
            Some(Ok(targets)) => div()
                .text_sm()
                .child(
                    targets
                        .first()
                        .map(|target| format!("Mundos compartidos · {}", target.name))
                        .unwrap_or_else(|| "Mundos: esta instancia empezará con su propia carpeta saves, o se une a un grupo compartido.".into()),
                )
                .into_any_element(),
        });
        body.into_any_element()
    }
    fn render(&mut self, dialog: Dialog, window: &mut Window, cx: &mut Context<Self>) -> Dialog {
        let body = self.body(window, cx);
        let p = &self.profile;
        let state = cx.entity();
        let title = h_flex()
            .gap(px(8.))
            .items_center()
            .child(design::dialog_title(p.name.clone()))
            .when(bridge::profile_family::is_test_profile(&p.profile_id), |title| {
                title.child(test_label(cx))
            });
        design::dialog(dialog)
            .title(title)
            .width(px((window.viewport_size().width.as_f32() - 48.0).min(760.0)))
            .on_ok(move |_, window, cx| {
                state.update(cx, |this, cx| this.submit(window, cx));
                false
            })
            .child(body)
            .footer(
                h_flex()
                    .gap_3()
                    .w_full()
                    .justify_end()
                    .child(
                        Button::new("close-profile-details")
                            .label("Cancelar")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        design::action_button(
                            "create-global-instance",
                            "Crear instancia",
                            design::ButtonAction::Primary,
                        )
                        .disabled(!self.can_submit(cx))
                        .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                    ),
            )
    }
}

impl Render for ProfileDetailsModal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = self.body(window, cx);
        v_flex().w_full().gap(px(16.)).child(body).child(
            h_flex()
                .w_full()
                .justify_end()
                .gap(px(8.))
                .child(
                    design::action_button("cancel-global-inline", "Cancelar", design::ButtonAction::Secondary)
                        .on_click(|_, w, c| crate::root::switch_page(crate::ui::PageType::Instances, &[], w, c)),
                )
                .child(
                    design::action_button("create-global-inline", "Crear instancia", design::ButtonAction::Primary)
                        .disabled(!self.can_submit(cx))
                        .on_click(cx.listener(|this, _, w, c| this.submit(w, c))),
                ),
        )
    }
}
pub(crate) fn create_profile_form(
    profile: GlobalProfileSummary,
    backend: BackendHandle,
    window: &mut Window,
    cx: &mut App,
) -> Entity<ProfileDetailsModal> {
    let state = cx.new(|cx| ProfileDetailsModal {
        name: cx.new(|cx| InputState::new(window, cx).placeholder(profile.name.clone())),
        profile,
        backend,
        targets: None,
        selected_group: None,
        submitted: false,
    });
    state.update(cx, |this, cx| this.load_groups(cx));
    state
}
pub fn open_global_profile_details(
    profile: GlobalProfileSummary,
    backend: BackendHandle,
    window: &mut Window,
    cx: &mut App,
) {
    let state = create_profile_form(profile, backend, window, cx);
    window.open_dialog(cx, move |dialog, window, cx| {
        cx.update_entity(&state, |state, cx| state.render(dialog, window, cx))
    });
}
