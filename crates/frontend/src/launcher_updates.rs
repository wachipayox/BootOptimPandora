//! Shared update state, native indicator and the approved version dialog.
use crate::{
    component::design::{self, ButtonAction},
    icon::PandoraIcon,
    interface_config::InterfaceConfig,
    theme,
};
use bridge::{handle::BackendHandle, message::MessageToBackend, modal_action::ModalAction};
use gpui::{prelude::*, *};
use gpui_component::{Disableable, WindowExt, h_flex, scroll::ScrollableElement, spinner::Spinner, v_flex};
use schema::pandora_update::{LauncherUpdateError, UpdatePrompt};

pub(crate) struct LauncherUpdates {
    backend: BackendHandle,
    pending: Option<UpdatePrompt>,
    checking: bool,
    checked: bool,
    error: Option<LauncherUpdateError>,
    #[cfg(feature = "ui-capture")]
    capture_override: bool,
}
impl Global for LauncherUpdates {}

pub(crate) fn init(backend: BackendHandle, cx: &mut App) {
    cx.set_global(LauncherUpdates {
        backend,
        pending: None,
        checking: false,
        checked: false,
        error: None,
        #[cfg(feature = "ui-capture")]
        capture_override: false,
    });
}

pub(crate) fn checking(cx: &mut App) {
    let state = cx.global_mut::<LauncherUpdates>();
    #[cfg(feature = "ui-capture")]
    if state.capture_override {
        return;
    }
    state.pending = None;
    state.checking = true;
    state.checked = false;
    state.error = None;
    cx.refresh_windows();
}

pub(crate) fn checked(
    result: Result<Option<UpdatePrompt>, LauncherUpdateError>,
    manual: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let (backend, update) = {
        let state = cx.global_mut::<LauncherUpdates>();
        #[cfg(feature = "ui-capture")]
        if state.capture_override {
            return;
        }
        state.checking = false;
        state.checked = true;
        match result {
            Ok(update) => {
                state.pending = update;
                state.error = None;
            },
            Err(error) => {
                state.pending = None;
                state.error = Some(error);
            },
        }
        (state.backend.clone(), state.pending.clone())
    };
    if let Some(update) = update {
        if manual || InterfaceConfig::get(cx).skipped_launcher_version.as_deref() != Some(update.new_version.as_ref()) {
            open_prompt(update, backend, window, cx);
        }
    }
    cx.refresh_windows();
}

pub(crate) fn request(cx: &mut App) {
    let backend = cx.global::<LauncherUpdates>().backend.clone();
    checking(cx);
    backend.send(MessageToBackend::CheckLauncherUpdates);
}

pub(crate) fn indicator(cx: &App) -> AnyElement {
    let state = cx.global::<LauncherUpdates>();
    let Some(update) = state.pending.clone() else {
        return div().into_any_element();
    };
    let backend = state.backend.clone();
    design::DesignButton::new("launcher-update-indicator")
        .aria_label("Actualizar")
        .tooltip("Actualizar")
        .w(px(24.))
        .h(px(24.))
        .px_0()
        .child(
            div()
                .text_color(rgb(theme::ACCENT_TEXT))
                .child(gpui_component::Icon::new(PandoraIcon::Download).size(px(14.))),
        )
        .on_click(move |_, window, cx| open_prompt(update.clone(), backend.clone(), window, cx))
        .into_any_element()
}

pub(crate) fn available(cx: &App) -> bool {
    cx.global::<LauncherUpdates>().pending.is_some()
}

pub(crate) fn about_widget(cx: &App) -> AnyElement {
    let state = cx.global::<LauncherUpdates>();
    let mut row = h_flex().gap(px(12.)).child(
        design::action_button("check-launcher-update", "Buscar actualización", ButtonAction::Secondary)
            .disabled(state.checking)
            .on_click(|_, _, cx| request(cx)),
    );
    if state.checking {
        row = row.child(Spinner::new());
    } else if let Some(error) = state.error {
        row = row.child(div().text_size(px(12.)).text_color(rgb(theme::DANGER)).child(error.message()));
    } else if state.checked && state.pending.is_none() {
        row = row.child(div().text_size(px(12.)).text_color(rgb(theme::INK_MUTED)).child("Estás al día"));
    }
    row.into_any_element()
}

pub fn open_prompt(update: UpdatePrompt, backend: BackendHandle, window: &mut Window, cx: &mut App) {
    open_prompt_expanded(update, backend, false, window, cx);
}

fn open_prompt_expanded(
    update: UpdatePrompt,
    backend: BackendHandle,
    expanded: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let view = cx.new(|_| UpdateView {
        update,
        backend,
        expanded,
    });
    window.open_dialog(cx, move |dialog, _, _| {
        design::dialog(dialog).overlay_closable(false).keyboard(false).child(view.clone())
    });
}

/// Cosmetic input for the explicit native capture feature only. The backend
/// rejects the fixed source marker before it can fetch or install anything.
#[cfg(feature = "ui-capture")]
pub(crate) fn capture(page: &str, window: &mut Window, cx: &mut App) -> anyhow::Result<()> {
    #[derive(serde::Deserialize)]
    struct Fixture {
        releases: Vec<schema::launcher_release::LauncherRelease>,
    }
    let path = std::env::var_os("WBOMBO_UI_CAPTURE_LAUNCHER_RELEASES")
        .ok_or_else(|| anyhow::anyhow!("launcher capture fixture is missing"))?;
    let bytes = std::fs::read(path)?;
    anyhow::ensure!(bytes.len() <= 4 * 1024 * 1024, "launcher capture fixture is too large");
    let fixture: Fixture = serde_json::from_slice(&bytes)?;
    let latest = fixture
        .releases
        .first()
        .ok_or_else(|| anyhow::anyhow!("launcher capture fixture is empty"))?;
    let update = UpdatePrompt {
        old_version: option_env!("PANDORA_RELEASE_VERSION")
            .unwrap_or(env!("CARGO_PKG_VERSION"))
            .trim_start_matches('v')
            .into(),
        new_version: latest.version.clone().into(),
        artifact: latest.artifact.clone(),
        releases: fixture.releases,
        channel: schema::backend_config::LauncherChannel::Developer,
        source: "capture-only".into(),
    };
    let backend = {
        let state = cx.global_mut::<LauncherUpdates>();
        state.capture_override = true;
        state.pending = if matches!(page, "launcher-update" | "launcher-update-expanded" | "launcher-update-indicator")
        {
            Some(update.clone())
        } else {
            None
        };
        state.checking = page == "launcher-update-checking";
        state.checked = !state.checking;
        state.error = match page {
            "launcher-update-search-error" => Some(LauncherUpdateError::Search),
            "launcher-update-verification-error" => Some(LauncherUpdateError::Verification),
            _ => None,
        };
        state.backend.clone()
    };
    match page {
        "launcher-update" | "launcher-update-expanded" => {
            open_prompt_expanded(update, backend, page.ends_with("expanded"), window, cx)
        },
        "launcher-update-progress" => {
            let action = ModalAction::default();
            let tracker = action.push_tracker("Actualizando".into());
            tracker.set_total(100);
            tracker.set_count(62);
            open_progress(action, window, cx);
        },
        "launcher-update-failed" => {
            let action = ModalAction::default();
            action.set_finished_with_error("No se ha podido actualizar el launcher.".into());
            open_progress(action, window, cx);
        },
        _ => {},
    }
    cx.refresh_windows();
    log::info!("launcher-update-ready pid={}", std::process::id());
    Ok(())
}

struct UpdateView {
    update: UpdatePrompt,
    backend: BackendHandle,
    expanded: bool,
}
impl Render for UpdateView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let heading = h_flex().gap(px(12.)).items_baseline().child(design::dialog_title("Versión nueva")).child(
            h_flex()
                .gap(px(8.))
                .font_family(theme::DISPLAY)
                .text_size(px(13.))
                .child(self.update.old_version.to_string())
                .child("→")
                .child(
                    div()
                        .text_color(rgb(theme::ACCENT_TEXT))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(self.update.new_version.to_string()),
                ),
        );
        let mut content = v_flex()
            .w_full()
            .gap(px(14.))
            .text_size(px(13.))
            .line_height(px(20.))
            .child(v_flex().gap(px(6.)).child(design::eyebrow("Wachiland Launcher")).child(heading));
        let mut notes = v_flex()
            .gap(px(14.))
            .w_full()
            .p(px(14.))
            .bg(rgba(0xffffff05))
            .text_size(px(12.5))
            .line_height(px(19.))
            .text_color(rgb(theme::INK_MUTED));
        let mut has_notes = false;
        for release in &self.update.releases {
            if release.notes.trim().is_empty() {
                continue;
            }
            has_notes = true;
            let date = chrono::DateTime::parse_from_rfc3339(&release.published_at)
                .map(|date| date.with_timezone(&chrono::Local).format("%d/%m/%Y").to_string())
                .unwrap_or_default();
            let mut section = v_flex().gap(px(4.)).child(
                h_flex()
                    .justify_between()
                    .child(
                        div()
                            .font_family(theme::DISPLAY)
                            .text_color(rgb(theme::ACCENT_TEXT))
                            .child(release.version.clone()),
                    )
                    .child(div().text_size(px(11.)).child(date)),
            );
            // Notes are plain server text. No HTML, Markdown links or executable media.
            for line in release.notes.lines() {
                section = section.child(div().min_h(px(19.)).child(line.to_owned()));
            }
            notes = notes.child(section);
        }
        if has_notes {
            content = content.child(if self.expanded {
                div()
                    .id("launcher-update-notes-scroll")
                    .max_h(px(320.))
                    .overflow_y_scrollbar()
                    .child(notes)
                    .into_any_element()
            } else {
                div()
                    .relative()
                    .max_h(px(96.))
                    .overflow_hidden()
                    .child(notes)
                    .child(div().absolute().bottom_0().left_0().w_full().h(px(40.)).bg(linear_gradient(
                        180.,
                        linear_color_stop(rgba(0x0e0e1100), 0.),
                        linear_color_stop(rgb(0x0e0e11), 1.),
                    )))
                    .into_any_element()
            });
            content = content.child(
                design::DesignButton::new("launcher-update-expand")
                    .h(px(24.))
                    .px_0()
                    .self_start()
                    .label(if self.expanded { "Ver menos ↑" } else { "Ver todo ↓" })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.expanded = !this.expanded;
                        cx.notify();
                    })),
            );
        }
        let size = format!("{:.0} MB", self.update.artifact.size as f64 / 1_000_000.);
        content
            .child(
                h_flex()
                    .justify_between()
                    .text_size(px(11.))
                    .text_color(rgb(theme::INK_MUTED))
                    .child(size)
                    .child("Se reinicia el launcher"),
            )
            .child(
                h_flex()
                    .justify_end()
                    .gap(px(10.))
                    .pt(px(6.))
                    .child(
                        design::action_button("launcher-update-later", "Más tarde", ButtonAction::Secondary).on_click(
                            cx.listener(|this, _, window, cx| {
                                InterfaceConfig::get_mut(cx).skipped_launcher_version =
                                    Some(this.update.new_version.to_string());
                                window.close_dialog(cx);
                            }),
                        ),
                    )
                    .child(
                        design::action_button("launcher-update-install", "Actualizar", ButtonAction::Primary).on_click(
                            cx.listener(|this, _, window, cx| {
                                let action = ModalAction::default();
                                this.backend.send(MessageToBackend::InstallUpdate {
                                    update: this.update.clone(),
                                    modal_action: action.clone(),
                                });
                                window.close_dialog(cx);
                                open_progress(action, window, cx);
                            }),
                        ),
                    ),
            )
    }
}

struct UpdateProgress {
    action: ModalAction,
    _task: Task<()>,
}

fn open_progress(action: ModalAction, window: &mut Window, cx: &mut App) {
    let notify = action.get_notify();
    let task = window.spawn(cx, async move |cx| {
        loop {
            notify.notified().await;
            if cx.update(|window, _| window.refresh()).is_err() {
                break;
            }
        }
    });
    let view = cx.new(|_| UpdateProgress { action, _task: task });
    window.open_dialog(cx, move |dialog, _, _| {
        design::dialog(dialog)
            .title(design::dialog_title("Actualizando"))
            .overlay_closable(false)
            .keyboard(false)
            .child(view.clone())
    });
}
impl Render for UpdateProgress {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        if let Some(error) = self.action.get_error_message() {
            return v_flex()
                .gap(px(14.))
                .child(SharedString::new(error))
                .child(
                    h_flex().justify_end().child(
                        design::action_button("launcher-update-error-close", "Aceptar", ButtonAction::Secondary)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
                )
                .into_any_element();
        }
        let (entries, animating) = crate::modals::generic::render_progress_trackers(&self.action, 0.);
        if animating {
            window.request_animation_frame();
        }
        if entries.is_empty() {
            return div().child(Spinner::new()).into_any_element();
        }
        v_flex().gap(px(10.)).children(entries).into_any_element()
    }
}
