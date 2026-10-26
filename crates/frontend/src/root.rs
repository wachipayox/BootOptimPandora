use std::{
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};

use parking_lot::Mutex;

use bridge::{
    handle::BackendHandle,
    install::ContentInstall,
    instance::{InstanceContentID, InstanceID},
    message::{MessageToBackend, QuickPlayLaunch},
    modal_action::ModalAction,
};
use gpui::{prelude::*, *};
use gpui_component::{Root, Theme, WindowExt, h_flex, scroll::ScrollableElement, v_flex};
use rustc_hash::FxHashSet;

use crate::{
    Backwards, CloseWindow, Forwards, OpenSettings,
    entity::DataEntities,
    game_output::{GameOutput, GameOutputRoot},
    interface_config::{InterfaceConfig, LiveGameOutputDisplay},
    modals,
    pages::instance::instance_page::InstanceSubpageType,
    ui::{LauncherUI, PageType},
};

pub struct LauncherRootGlobal {
    pub root: Entity<LauncherRoot>,
}

impl Global for LauncherRootGlobal {}

pub struct LauncherRoot {
    pub ui: Entity<LauncherUI>,
    data: DataEntities,
    focus_handle: FocusHandle,
    startup: Option<crate::startup::Controller>,
}

impl LauncherRoot {
    pub fn new(
        data: &DataEntities,
        window: &mut Window,
        cx: &mut Context<Self>,
        show_startup: bool,
        opened: std::time::Instant,
    ) -> Self {
        let startup = show_startup.then(|| crate::startup::Controller::new(opened, cx)).flatten();
        let launcher_ui = cx.new(|cx| LauncherUI::new(data, window, cx));

        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);

        Self {
            ui: launcher_ui,
            data: data.clone(),
            focus_handle,
            startup,
        }
    }
}

static RENDER_CUSTOM_TITLEBAR: AtomicBool = AtomicBool::new(true);

static HOME_UPDATES: std::sync::LazyLock<Mutex<std::collections::HashMap<InstanceID, ModalAction>>> =
    std::sync::LazyLock::new(|| Mutex::new(Default::default()));
pub fn home_update_progress(id: InstanceID) -> Option<(f32, usize, usize)> {
    let modal = HOME_UPDATES.lock().get(&id).cloned()?;
    Some(modal.write_trackers(|trackers| {
        trackers
            .iter()
            .find(|t| t.get_title().as_ref() == "Downloading modpack assets")
            .map(|t| {
                let (count, total) = t.get();
                (t.get_float().unwrap_or(0.), count, total)
            })
            .unwrap_or((0., 0, 0))
    }))
}
pub fn inherited_operation_active(id: InstanceID) -> bool {
    bridge::inherited_operations::is_active(id)
}

fn begin_inherited_operation(id: InstanceID, cx: &mut App) -> bool {
    let inserted = bridge::inherited_operations::try_begin(id);
    if inserted && matches!(InterfaceConfig::get(cx).main_page, PageType::InstancePage { .. }) {
        InterfaceConfig::get_mut(cx).instance_subpage = InstanceSubpageType::Quickplay;
    }
    inserted
}

fn finish_inherited_operation(id: InstanceID, window: &mut Window) {
    HOME_UPDATES.lock().remove(&id);
    bridge::inherited_operations::finish(id);
    window.refresh();
}

pub fn update_inherited_files(id: InstanceID, data: &DataEntities, window: &mut Window, cx: &mut App) {
    if !begin_inherited_operation(id, cx) {
        return;
    }
    window.refresh();
    let modal = ModalAction::default();
    HOME_UPDATES.lock().insert(id, modal.clone());
    let (send, receive) = tokio::sync::oneshot::channel();
    data.backend_handle.send(MessageToBackend::UpdateInheritedChain {
        id,
        channel: send,
        modal_action: modal.clone(),
    });
    let progress_window = modals::generic::show_modal(
        window,
        cx,
        "Updating inherited files…".into(),
        "Unable to update inherited files".into(),
        modal.clone(),
    );
    let handle = window.window_handle();
    cx.spawn(async move |cx| {
        let result = receive.await.unwrap_or_else(|_| Err("Launcher backend stopped".into()));
        match result {
            Ok(()) => {
                modal.set_finished();
                if let Some(progress_window) = progress_window {
                    _ = cx.update_window(progress_window, |_, window, _| window.remove_window());
                }
            },
            Err(error) => modal.set_finished_with_error(error.into()),
        }
        _ = cx.update_window(handle, |_, window, _| finish_inherited_operation(id, window));
    })
    .detach();
}

pub(crate) fn should_render_custom_titlebar() -> bool {
    RENDER_CUSTOM_TITLEBAR.load(std::sync::atomic::Ordering::Relaxed)
}

pub(crate) fn set_should_render_custom_titlebar(value: bool) {
    RENDER_CUSTOM_TITLEBAR.store(value, std::sync::atomic::Ordering::Relaxed);
}

impl Render for LauncherRoot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(message) = &*self.data.panic_messages.deadlock_message.read() {
            let purple = Hsla {
                h: 0.8333333333,
                s: 1.,
                l: 0.25,
                a: 1.,
            };
            return v_flex()
                .size_full()
                .text_color(gpui::white())
                .bg(purple)
                .child(message.clone())
                .overflow_y_scrollbar()
                .into_any_element();
        }
        if let Some(message) = &*self.data.panic_messages.panic_message.read() {
            return v_flex()
                .size_full()
                .text_color(gpui::white())
                .bg(gpui::blue())
                .child(message.clone())
                .overflow_y_scrollbar()
                .into_any_element();
        }
        if self.data.backend_handle.is_closed() {
            return v_flex()
                .size_full()
                .text_color(gpui::white())
                .bg(gpui::red())
                .child(t::system::backend_shutdown())
                .into_any_element();
        }

        Theme::global_mut(cx).sheet.margin_top = Pixels::ZERO;

        let sheet_layer = Root::render_sheet_layer(window, cx);
        let dialog_layer = Root::render_dialog_layer(window, cx);
        let notification_layer = Root::render_notification_layer(window, cx);
        let ready = self.startup.is_some() && self.ui.update(cx, |ui, cx| ui.startup_ready(window, cx));
        let startup_layer = self.startup.as_mut().and_then(|startup| startup.layer(ready, window, cx));
        #[cfg(feature = "ui-capture")]
        let startup_layer = startup_layer.or_else(|| {
            let page = std::env::var("WBOMBO_UI_CAPTURE_PAGE").ok()?;
            let progress = page.strip_prefix("startup-transition:")?.parse::<f32>().ok()?.clamp(0., 1.);
            Some(
                div()
                    .absolute()
                    .inset_0()
                    .size_full()
                    .opacity(1. - progress)
                    .child(crate::startup::vector::frame(6.))
                    .into_any_element(),
            )
        });
        if startup_layer.is_none() {
            self.startup = None;
        }
        let startup_layer = startup_layer.map(|layer| {
            div()
                .id("startup-presentation")
                .absolute()
                .inset_0()
                .size_full()
                .occlude()
                .child(layer)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        if let Some(startup) = &mut this.startup {
                            startup.skip();
                        }
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
        });

        v_flex()
            .relative()
            .size_full()
            .child(
                if std::env::var("WBOMBO_UI_CAPTURE_PAGE").is_ok_and(|page| page.starts_with("components")) {
                    crate::component::design_review::render(window, cx)
                } else {
                    self.ui.clone().into_any_element()
                },
            )
            .children(sheet_layer)
            .children(dialog_layer)
            .children(notification_layer)
            .children(startup_layer)
            .track_focus(&self.focus_handle)
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if let Some(startup) = &mut this.startup {
                    if event.keystroke.key == "escape" {
                        startup.skip();
                        cx.notify();
                    }
                    cx.stop_propagation();
                }
            }))
            .on_action(|_: &CloseWindow, window, _| {
                window.remove_window();
            })
            .on_action({
                let data = self.data.clone();
                move |_: &OpenSettings, window, cx| {
                    crate::settings::open_settings_window(window, &data, cx);
                }
            })
            .on_action({
                let ui = self.ui.clone();
                move |_: &Backwards, window, cx| {
                    ui.update(cx, |ui, cx| {
                        ui.nav_backwards(window, cx);
                    });
                }
            })
            .on_action({
                let ui = self.ui.clone();
                move |_: &Forwards, window, cx| {
                    ui.update(cx, |ui, cx| {
                        ui.nav_forwards(window, cx);
                    });
                }
            })
            .on_mouse_down(MouseButton::Navigate(NavigationDirection::Back), {
                let ui = self.ui.clone();
                move |_, window, cx| {
                    ui.update(cx, |ui, cx| {
                        ui.nav_backwards(window, cx);
                    });
                }
            })
            .on_mouse_down(MouseButton::Navigate(NavigationDirection::Forward), {
                let ui = self.ui.clone();
                move |_, window, cx| {
                    ui.update(cx, |ui, cx| {
                        ui.nav_forwards(window, cx);
                    });
                }
            })
            .into_any_element()
    }
}

pub fn start_new_account_login(backend_handle: &BackendHandle, window: &mut Window, cx: &mut App) {
    let modal_action = ModalAction::default();

    backend_handle.send(MessageToBackend::AddNewAccount {
        modal_action: modal_action.clone(),
    });

    let title = t::account::add::title();
    modals::generic::show_modal(window, cx, title.into(), t::account::add::error().into(), modal_action);
}

/// Ask only about known revision changes before entering the launch pipeline.
pub fn start_instance(
    id: InstanceID,
    name: SharedString,
    quick_play: Option<QuickPlayLaunch>,
    data: &DataEntities,
    window: &mut Window,
    cx: &mut App,
) {
    if !begin_inherited_operation(id, cx) {
        return;
    }
    window.refresh();
    let check_modal = ModalAction::default();
    let check_tracker = check_modal.push_tracker("Checking for updates".into());
    let check_window = modals::generic::show_modal(
        window,
        cx,
        "Checking for updates…".into(),
        "Unable to check for updates".into(),
        check_modal.clone(),
    );
    let (send, receive) = tokio::sync::oneshot::channel();
    data.backend_handle.send(MessageToBackend::CheckInheritedUpdates { id, channel: send });
    let handle = window.window_handle();
    let data = data.clone();
    cx.spawn(async move |cx| {
        let result = receive.await.unwrap_or_else(|_| Err("Launcher backend stopped".into()));
        check_tracker.set_finished(bridge::modal_action::ProgressTrackerFinishType::Normal);
        check_modal.set_finished();
        if let Some(check_window) = check_window {
            _ = cx.update_window(check_window, |_, window, _| window.remove_window());
        }
        _ = cx.update_window(handle, |_, window, cx| {
            match result {
                Ok(updates) if !updates.is_empty() => {
                    show_inherited_update(id, name, quick_play, data, updates, window, cx);
                },
                Ok(_) => {
                    finish_inherited_operation(id, window);
                    start_instance_unchecked(id, name, quick_play, &data, window, cx);
                },
                Err(error) => {
                    // Connectivity must not prevent playing an already installed instance.
                    log::warn!("Inherited update check skipped: {error}");
                    finish_inherited_operation(id, window);
                    start_instance_unchecked(id, name, quick_play, &data, window, cx);
                },
            }
        });
    })
    .detach();
}

fn update_then_start(
    id: InstanceID,
    name: SharedString,
    quick: Option<QuickPlayLaunch>,
    data: &DataEntities,
    window: &mut Window,
    cx: &mut App,
) {
    let (send, receive) = tokio::sync::oneshot::channel();
    let modal = ModalAction::default();
    HOME_UPDATES.lock().insert(id, modal.clone());
    data.backend_handle.send(MessageToBackend::UpdateInheritedChain {
        id,
        channel: send,
        modal_action: modal.clone(),
    });
    let progress_window = modals::generic::show_modal(
        window,
        cx,
        "Updating inherited files…".into(),
        "Unable to update inherited files".into(),
        modal.clone(),
    );
    let handle = window.window_handle();
    let data = data.clone();
    cx.spawn(
        async move |cx| match receive.await.unwrap_or_else(|_| Err("Launcher backend stopped".into())) {
            Ok(()) => {
                modal.set_finished();
                if let Some(progress_window) = progress_window {
                    _ = cx.update_window(progress_window, |_, window, _| window.remove_window());
                }
                _ = cx.update_window(handle, |_, window, cx| {
                    finish_inherited_operation(id, window);
                    start_instance_unchecked(id, name, quick, &data, window, cx);
                });
            },
            Err(error) => {
                modal.set_finished_with_error(error.into());
                _ = cx.update_window(handle, |_, window, _| finish_inherited_operation(id, window));
            },
        },
    )
    .detach();
}

pub fn start_instance_without_update(
    id: InstanceID,
    name: SharedString,
    data: &DataEntities,
    window: &mut Window,
    cx: &mut App,
) {
    start_instance_unchecked(id, name, None, data, window, cx);
}

fn start_instance_unchecked(
    id: InstanceID,
    name: SharedString,
    quick_play: Option<QuickPlayLaunch>,
    data: &DataEntities,
    window: &mut Window,
    cx: &mut App,
) {
    let modal_action = ModalAction::normal_launch();

    // Remove any stale live game outputs
    if let Some(instance_entry) = data.instances.read(cx).entries.get(&id).cloned() {
        instance_entry.update(cx, |entry, _| {
            entry.live_game_output = None;
        });
    };

    let (sender, receiver) = tokio::sync::oneshot::channel();

    let live_game_output_display = InterfaceConfig::get(cx).live_game_output_display;
    let live_game_output = if live_game_output_display == LiveGameOutputDisplay::Hidden {
        None
    } else {
        Some(sender)
    };

    data.backend_handle.send(MessageToBackend::StartInstance {
        id,
        quick_play,
        live_game_output,
        modal_action: modal_action.clone(),
    });

    let title: SharedString = t::instance::start::title(&name).into();
    modals::generic::show_modal(window, cx, title, t::instance::start::error().into(), modal_action);

    let window_handle = window.window_handle();
    let display_id = window.display(cx).map(|display| display.id());
    let output_window_bounds = WindowBounds::Windowed(Bounds::centered(display_id, size(px(720.0), px(480.0)), cx));
    let data = data.clone();
    cx.spawn(async move |cx| {
        let Ok(receiver) = receiver.await else {
            return;
        };

        match live_game_output_display {
            LiveGameOutputDisplay::Hidden => {},
            LiveGameOutputDisplay::SeparateWindow => {
                let options = WindowOptions {
                    app_id: Some("PandoraLauncher".into()),
                    window_min_size: Some(size(px(360.0), px(240.0))),
                    display_id,
                    window_bounds: Some(output_window_bounds),
                    titlebar: Some(TitlebarOptions {
                        title: Some(t::system::game_output().into()),
                        ..Default::default()
                    }),
                    window_decorations: Some(WindowDecorations::Server),
                    ..Default::default()
                };
                _ = cx.open_window(options, |window, cx| {
                    let game_output = cx.new(|cx| GameOutput::new(receiver, cx));
                    let game_output_root = cx.new(|cx| GameOutputRoot::new(game_output.clone(), window, cx));
                    window.activate_window();
                    cx.new(|cx| Root::new(game_output_root, window, cx))
                });
            },
            LiveGameOutputDisplay::TabOnInstancePage => {
                _ = cx.update_window(window_handle, |_, window, cx| {
                    let game_output = cx.new(|cx| GameOutput::new(receiver, cx));
                    let game_output_root = cx.new(|cx| GameOutputRoot::new(game_output.clone(), window, cx));

                    let Some(instance_entry) = data.instances.read(cx).entries.get(&id).cloned() else {
                        return;
                    };

                    instance_entry.update(cx, |entry, _| {
                        entry.live_game_output = Some(game_output_root);
                    });

                    let config = InterfaceConfig::get(cx);
                    if matches!(config.main_page, PageType::InstancePage { .. })
                        && config.instance_subpage != InstanceSubpageType::LiveGameOutput
                    {
                        InterfaceConfig::get_mut(cx).instance_subpage = InstanceSubpageType::LiveGameOutput;
                    }
                });
            },
        }
    })
    .detach();
}

pub fn repair_game_files(id: InstanceID, backend_handle: &BackendHandle, window: &mut Window, cx: &mut App) {
    let modal_action = ModalAction::default();
    backend_handle.send(MessageToBackend::RepairGameFiles {
        id,
        modal_action: modal_action.clone(),
    });
    modals::generic::show_modal(
        window,
        cx,
        "Repair game files".into(),
        "Unable to repair game files".into(),
        modal_action,
    );
}

pub fn start_install(
    content_install: ContentInstall,
    backend_handle: &BackendHandle,
    window: &mut Window,
    cx: &mut App,
) {
    let modal_action = ModalAction::default();

    backend_handle.send(MessageToBackend::InstallContent {
        content: content_install.clone(),
        modal_action: modal_action.clone(),
    });

    modals::generic::show_notification(window, cx, t::instance::content::install::error().into(), modal_action);
}

pub fn change_mod_version(
    content_install: ContentInstall,
    hash: u64,
    updating: &Arc<Mutex<FxHashSet<u64>>>,
    backend_handle: &BackendHandle,
    window: &mut Window,
    cx: &mut App,
) {
    let modal_action = ModalAction::default();

    updating.lock().insert(hash);
    modal_action.add_finish_effect({
        let updating = updating.clone();
        move || {
            updating.lock().remove(&hash);
        }
    });

    backend_handle.send(MessageToBackend::InstallContent {
        content: content_install.clone(),
        modal_action: modal_action.clone(),
    });

    modals::generic::show_notification(window, cx, t::instance::content::install::error().into(), modal_action);
}

pub fn start_update_check(instance: InstanceID, backend_handle: &BackendHandle, window: &mut Window, cx: &mut App) {
    let modal_action = ModalAction::default();

    backend_handle.send(MessageToBackend::UpdateCheck {
        instance,
        modal_action: modal_action.clone(),
    });

    let title: SharedString = t::instance::content::update::check::title().into();
    modals::generic::show_modal(window, cx, title, t::instance::content::update::check::error().into(), modal_action);
}

pub fn update_single_mod(
    instance: InstanceID,
    mod_id: InstanceContentID,
    hash: u64,
    updating: &Arc<Mutex<FxHashSet<u64>>>,
    backend_handle: &BackendHandle,
    window: &mut Window,
    cx: &mut App,
) {
    let modal_action = ModalAction::default();

    updating.lock().insert(hash);
    modal_action.add_finish_effect({
        let updating = updating.clone();
        move || {
            updating.lock().remove(&hash);
        }
    });

    backend_handle.send(MessageToBackend::UpdateContent {
        instance,
        content_id: mod_id,
        modal_action: modal_action.clone(),
    });

    modals::generic::show_notification(
        window,
        cx,
        t::instance::content::update::download::error().into(),
        modal_action,
    );
}

pub fn upload_log_file(path: Arc<Path>, backend_handle: &BackendHandle, window: &mut Window, cx: &mut App) {
    let modal_action = ModalAction::default();

    backend_handle.send(MessageToBackend::UploadLogFile {
        path,
        modal_action: modal_action.clone(),
    });

    let title: SharedString = t::instance::logs::upload::title().into();
    modals::generic::show_modal(window, cx, title, t::instance::logs::upload::error().into(), modal_action);
}

pub fn open_home_accounts(cx: &mut App) {
    cx.update_global::<LauncherRootGlobal, ()>(|global, cx| {
        global.root.update(cx, |root, cx| {
            root.ui.update(cx, |ui, cx| {
                ui.home_account_menu = true;
                cx.notify();
            });
        });
    });
}
pub fn switch_page(page: PageType, breadcrumbs: &[PageType], window: &mut Window, cx: &mut App) {
    cx.update_global::<LauncherRootGlobal, ()>(|global, cx| {
        global.root.update(cx, |launcher_root, cx| {
            launcher_root.ui.update(cx, |ui, cx| {
                ui.switch_page(page, breadcrumbs, window, cx);
            });
        });
    });
}

fn show_inherited_update(
    id: InstanceID,
    name: SharedString,
    quick_play: Option<QuickPlayLaunch>,
    data: DataEntities,
    updates: bridge::message::InheritedUpdateCheck,
    window: &mut Window,
    cx: &mut App,
) {
    let changelog = cx.new(|cx| {
        crate::pages::instance::update_changelog::UpdateChangelog::new(
            data.backend_handle.clone(),
            updates.global.clone(),
            name.clone(),
            window,
            cx,
        )
    });
    window.open_dialog(cx, move |dialog, _, _| {
        let skip_data = data.clone();
        let skip_name = name.clone();
        let skip_quick = quick_play.clone();
        let update_data = data.clone();
        let update_name = name.clone();
        let update_quick = quick_play.clone();
        let enter_data = data.clone();
        let enter_name = name.clone();
        let enter_quick = quick_play.clone();
        crate::component::design::dialog(dialog)
            .w(px(560.))
            .overlay_closable(false)
            .keyboard(false)
            .close_button(false)
            .on_ok(move |_, window, cx| {
                update_then_start(id, enter_name.clone(), enter_quick.clone(), &enter_data, window, cx);
                true
            })
            .child(changelog.clone())
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(rgb(crate::theme::INK_MUTED))
                    .child("Tus cambios locales se conservan"),
            )
            .footer(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .child(
                        crate::component::design::DesignButton::new("skip-update")
                            .label("Jugar sin actualizar")
                            .on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                                finish_inherited_operation(id, window);
                                start_instance_unchecked(
                                    id,
                                    skip_name.clone(),
                                    skip_quick.clone(),
                                    &skip_data,
                                    window,
                                    cx,
                                );
                            }),
                    )
                    .child(
                        crate::component::design::action_button(
                            "update-start",
                            "Actualizar y jugar",
                            crate::component::design::ButtonAction::Primary,
                        )
                        .on_click(move |_, window, cx| {
                            window.close_dialog(cx);
                            update_then_start(id, update_name.clone(), update_quick.clone(), &update_data, window, cx);
                        }),
                    ),
            )
    });
}

/// Capture only checks metadata and opens the real update prompt; it never auto-starts.
pub fn capture_inherited_update(
    id: InstanceID,
    name: SharedString,
    data: &DataEntities,
    window: &mut Window,
    cx: &mut App,
) {
    let (send, receive) = tokio::sync::oneshot::channel();
    data.backend_handle.send(MessageToBackend::CheckInheritedUpdates { id, channel: send });
    let data = data.clone();
    let handle = window.window_handle();
    cx.spawn(async move |cx| {
        let result = receive.await;
        _ = cx.update_window(handle, |_, window, cx| match result {
            Ok(Ok(updates)) if !updates.is_empty() => {
                show_inherited_update(id, name, None, data, updates, window, cx);
                log::info!("[WBOMBO-UI-CAPTURE] update-dialog-ready pid={}", std::process::id());
            },
            _ => log::warn!("UI capture has no pending inherited update"),
        });
    })
    .detach();
}

pub(crate) fn set_browser_manual_view(value: bool, cx: &mut App) {
    cx.update_global::<LauncherRootGlobal, ()>(|global, cx| {
        global.root.update(cx, |root, cx| {
            root.ui.update(cx, |ui, cx| {
                ui.browser_manual_view = value;
                cx.notify();
            })
        })
    });
}
