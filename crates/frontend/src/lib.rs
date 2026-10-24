#![deny(unused_must_use)]

use std::{
    borrow::Cow,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};

use bridge::{
    handle::{BackendHandle, FrontendReceiver},
    quit::QuitCoordinator,
};
use gpui::*;
use gpui_component::{
    Root, StyledExt, WindowExt,
    notification::{Notification, NotificationType},
};
use indexmap::IndexMap;
use parking_lot::RwLock;

use crate::{
    entity::{
        DataEntities, PanicMessages, account::AccountEntries, instance::InstanceEntries, metadata::FrontendMetadata,
    },
    interface_config::InterfaceConfig,
    processor::Processor,
    root::{LauncherRoot, LauncherRootGlobal},
};

pub mod background;
pub mod component;
pub mod data_asset_loader;
pub mod entity;
pub mod game_output;
pub mod icon;
pub mod interface_config;
pub mod modals;
pub mod pages;
pub mod png_render_cache;
pub mod processor;
pub mod root;
pub mod settings;
pub mod skin_renderer;
pub mod skin_thumbnail_cache;
pub mod theme;
mod theme_palette;
pub mod ui;
#[cfg(windows)]
pub mod windows_security_ui;

#[derive(rust_embed::RustEmbed)]
#[folder = "../../assets"]
#[include = "icons/**/*.svg"]
#[include = "images/**/*.png"]
#[include = "images/**/*.svg"]
#[include = "fonts/**/*.ttf"]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path.is_empty() {
            return Ok(None);
        }

        Self::get(path)
            .map(|f| Some(f.data))
            .ok_or_else(|| anyhow::anyhow!("could not find asset at path \"{path}\""))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(Self::iter().filter_map(|p| p.starts_with(path).then(|| p.into())).collect())
    }
}

actions!([Quit, CloseWindow, OpenSettings, Forwards, Backwards, Confirm]);

pub fn start(
    launcher_dir: PathBuf,
    panic_message: Arc<RwLock<Option<String>>>,
    deadlock_message: Arc<RwLock<Option<String>>>,
    backend_handle: BackendHandle,
    mut recv: FrontendReceiver,
    quit_coordinator: QuitCoordinator,
) {
    let http_client = Arc::new(reqwest_client::ReqwestClient::user_agent(&*schema::USER_AGENT).unwrap());

    if let Err(error) = theme::install_font_licenses(&launcher_dir) {
        log::warn!("Could not write bundled font licenses: {error}");
    }

    gpui_platform::application()
        .with_http_client(http_client)
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            let _ = cx.text_system().add_fonts(vec![
                Assets.load("fonts/inter/Inter-Regular.ttf").unwrap().unwrap(),
                Assets.load("fonts/notosans/NotoSans[wdth,wght].ttf").unwrap().unwrap(),
                Assets.load("fonts/chakrapetch/ChakraPetch-Regular.ttf").unwrap().unwrap(),
                Assets.load("fonts/chakrapetch/ChakraPetch-Medium.ttf").unwrap().unwrap(),
                Assets.load("fonts/chakrapetch/ChakraPetch-SemiBold.ttf").unwrap().unwrap(),
                Assets.load("fonts/chakrapetch/ChakraPetch-Bold.ttf").unwrap().unwrap(),
                Assets.load("fonts/roboto-mono/RobotoMono-Regular.ttf").unwrap().unwrap(),
                Assets.load("fonts/minecraft.ttf").unwrap().unwrap(),
            ]);

            gpui_component::init(cx);
            theme::init(cx);
            pages::news::init(cx);
            InterfaceConfig::init(cx, launcher_dir.join("interface.json").into());

            t::set_lang(&InterfaceConfig::get(cx).language);

            gpui_component::Theme::change(gpui_component::ThemeMode::Dark, None, cx);

            let theme_folder = launcher_dir.join("themes");

            _ = gpui_component::ThemeRegistry::watch_dir(theme_folder.clone(), cx, move |cx| {
                InterfaceConfig::apply_theme(cx, true);
            });
            InterfaceConfig::apply_theme(cx, false);

            cx.set_quit_mode(QuitMode::Explicit);

            cx.on_app_quit(|cx| {
                InterfaceConfig::force_save(cx);
                async {}
            })
            .detach();

            let main_window_hidden = Arc::new(AtomicBool::new(false));

            cx.on_window_closed({
                let main_window_hidden = main_window_hidden.clone();
                let quit_coordinator = quit_coordinator.clone();
                move |cx, _window| {
                    if main_window_hidden.load(std::sync::atomic::Ordering::SeqCst) {
                        return;
                    }

                    let windows = cx.windows();

                    let config = InterfaceConfig::get(cx);
                    if config.quit_on_main_closed {
                        for window in &windows {
                            let is_main = window
                                .read(cx, |window: Entity<Root>, cx| {
                                    window.read(cx).view().clone().downcast::<LauncherRoot>().is_ok()
                                })
                                .unwrap_or(false);
                            if is_main {
                                return;
                            }
                        }

                        for window in &windows {
                            _ = window.update(cx, |_, window, _| {
                                window.remove_window();
                            });
                        }
                    }

                    quit_coordinator.set_can_quit(windows.is_empty());
                }
            })
            .detach();

            cx.bind_keys([
                KeyBinding::new("secondary-q", Quit, None),
                KeyBinding::new("secondary-w", CloseWindow, None),
                KeyBinding::new("secondary-,", OpenSettings, None),
                KeyBinding::new("secondary-[", Backwards, None),
                KeyBinding::new("secondary-]", Forwards, None),
                KeyBinding::new("enter", Confirm, None),
            ]);

            let instances = cx.new(|_| InstanceEntries {
                entries: IndexMap::new(),
            });
            let metadata = cx.new(|_| FrontendMetadata::new(backend_handle.clone()));
            let accounts = cx.new(|_| AccountEntries::default());
            let data = DataEntities {
                instances,
                metadata,
                backend_handle,
                accounts,
                theme_folder: theme_folder.into(),
                panic_messages: Arc::new(PanicMessages {
                    panic_message,
                    deadlock_message,
                }),
            };

            cx.on_action({
                let data = data.clone();
                move |_: &Quit, cx| {
                    // Instances optimizing their launch: ask first, in the main window.
                    if modals::aot_optimization::any_optimizing(&data, cx) {
                        let main_window = cx.windows().into_iter().find(|window| {
                            window
                                .read(cx, |window: Entity<Root>, cx| {
                                    window.read(cx).view().clone().downcast::<LauncherRoot>().is_ok()
                                })
                                .unwrap_or(false)
                        });
                        if let Some(main_window) = main_window {
                            let data = data.clone();
                            _ = main_window.update(cx, |_, window, cx| {
                                window.activate_window();
                                modals::aot_optimization::confirm_close(&data, window, cx, |_, cx| {
                                    cx.defer(close_all_windows);
                                });
                            });
                            return;
                        }
                    }
                    close_all_windows(cx);
                }
            });

            let mut processor = Processor::new(data.clone(), main_window_hidden, quit_coordinator);

            // Backend startup has already queued the complete instance snapshot. Create
            // the window after that snapshot, so loading existing instances is not
            // mistaken for adding new instances and replacing the saved Home selection.
            let mut startup_messages = Vec::new();
            while let Some(message) = recv.try_recv() {
                if matches!(
                    &message,
                    bridge::message::MessageToFrontend::InstanceAdded { .. }
                        | bridge::message::MessageToFrontend::AccountsUpdated { .. }
                ) {
                    processor.process(message, cx);
                } else {
                    startup_messages.push(message);
                }
            }
            // Flush the snapshot's deferred entity events before installing window
            // subscriptions; otherwise those events would still look like new adds.
            cx.defer(move |cx| {
                // Replay window, launch and notification messages in their original
                // order, including CLI launch/hide behavior.
                for message in startup_messages {
                    processor.process(message, cx);
                }
                cx.spawn(async move |cx| {
                    while let Some(message) = recv.recv().await {
                        _ = cx.update(|cx| {
                            processor.process(message, cx);
                        });
                    }
                })
                .detach();
            });
        });
}

fn close_all_windows(cx: &mut App) {
    for window in cx.windows() {
        _ = window.update(cx, |_, window, _| {
            window.remove_window();
        });
    }
}

pub fn open_main_window(data: &DataEntities, display_id: Option<DisplayId>, cx: &mut App) -> AnyWindowHandle {
    let config = InterfaceConfig::get(cx);

    // A display override should use that display's default placement, not coordinates saved for another screen.
    let window_bounds = if display_id.is_some() {
        None
    } else {
        match config.main_window_bounds {
            interface_config::WindowBounds::Inherit => None,
            interface_config::WindowBounds::Windowed { x, y, w, h } => {
                Some(WindowBounds::Windowed(Bounds::new(Point::new(px(x), px(y)), Size::new(px(w), px(h)))))
            },
            interface_config::WindowBounds::Maximized { x, y, w, h } => {
                Some(WindowBounds::Maximized(Bounds::new(Point::new(px(x), px(y)), Size::new(px(w), px(h)))))
            },
            interface_config::WindowBounds::Fullscreen { x, y, w, h } => {
                Some(WindowBounds::Fullscreen(Bounds::new(Point::new(px(x), px(y)), Size::new(px(w), px(h)))))
            },
        }
    };

    let use_custom_titlebar = !config.use_os_titlebar;
    crate::root::set_should_render_custom_titlebar(use_custom_titlebar);
    let handle = cx
        .open_window(
            WindowOptions {
                app_id: Some("PandoraLauncher".into()),
                window_min_size: Some(size(px(480.0), px(270.0))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Wachiland Launcher".into()),
                    appears_transparent: use_custom_titlebar,
                    ..Default::default()
                }),
                app_owns_titlebar_drag: use_custom_titlebar,
                window_bounds,
                window_decorations: Some(if use_custom_titlebar {
                    WindowDecorations::Client
                } else {
                    WindowDecorations::Server
                }),
                display_id,
                ..Default::default()
            },
            |window, cx| {
                let capture_page = std::env::var("WBOMBO_UI_CAPTURE_PAGE").ok();
                if let Some((name, subpage)) = capture_page.as_deref().and_then(capture_instance_target) {
                    InterfaceConfig::get_mut(cx).instance_subpage = subpage;
                    InterfaceConfig::get_mut(cx).main_page = ui::PageType::InstancePage { name: name.into() };
                    if capture_page.as_deref().is_some_and(|p| p.starts_with("instance-live:")) {
                        if let Some(id) = crate::entity::instance::InstanceEntries::find_id_by_name(
                            &data.instances,
                            &SharedString::from(name.to_owned()),
                            cx,
                        ) {
                            let (send, receive) = tokio::sync::mpsc::unbounded_channel();
                            let output = cx.new(|cx| crate::game_output::GameOutput::new(receive, cx));
                            let view = cx.new(|cx| crate::game_output::GameOutputRoot::new(output, window, cx));
                            if let Some(instance) = data.instances.read(cx).entries.get(&id).cloned() {
                                instance.update(cx, |instance, cx| {
                                    instance.live_game_output = Some(view);
                                    cx.notify();
                                });
                            }
                            for index in 0..48 {
                                let _ = send.send(bridge::message::GameOutputMsg {
                                    time: 0,
                                    level: if index % 9 == 0 {
                                        bridge::game_output::GameOutputLogLevel::Warn
                                    } else {
                                        bridge::game_output::GameOutputLogLevel::Info
                                    },
                                    text: Arc::from([Arc::<str>::from(format!(
                                        "Registro sintético del agente 274 · {index}"
                                    ))]),
                                });
                            }
                            drop(send);
                            log::info!("instance-live-fixture-ready pid={}", std::process::id());
                        }
                    }
                }
                match capture_page.as_deref() {
                    Some("instances" | "home-menu" | "home-news") => {
                        InterfaceConfig::get_mut(cx).main_page = ui::PageType::Instances
                    },
                    Some(name) if name.starts_with("news-") => {
                        InterfaceConfig::get_mut(cx).main_page = ui::PageType::Instances;
                    },
                    Some("skins") => InterfaceConfig::get_mut(cx).main_page = ui::PageType::Skins,
                    Some("import") => InterfaceConfig::get_mut(cx).main_page = ui::PageType::Import,
                    Some("syncing") => InterfaceConfig::get_mut(cx).main_page = ui::PageType::Syncing,
                    Some(name) if name.starts_with("instance:") || name.starts_with("update:") => {
                        InterfaceConfig::get_mut(cx).main_page = ui::PageType::InstancePage {
                            name: name.split_once(':').unwrap().1.into(),
                        };
                    },
                    _ => {},
                }
                let launcher_root = cx.new(|cx| {
                    cx.observe_window_bounds(window, move |_, window, cx| {
                        let origin = window.bounds().origin;
                        let size = window.viewport_size();
                        let new_bounds = (
                            origin.x.to_f64() as f32,
                            origin.y.to_f64() as f32,
                            size.width.to_f64() as f32,
                            size.height.to_f64() as f32,
                        );

                        let old_window_bounds = InterfaceConfig::get(cx).main_window_bounds.clone();
                        let old_bounds = match old_window_bounds {
                            interface_config::WindowBounds::Inherit => new_bounds,
                            interface_config::WindowBounds::Windowed { x, y, w, h } => (x, y, w, h),
                            interface_config::WindowBounds::Maximized { x, y, w, h } => (x, y, w, h),
                            interface_config::WindowBounds::Fullscreen { x, y, w, h } => (x, y, w, h),
                        };

                        let new_window_bounds = if window.is_fullscreen() {
                            interface_config::WindowBounds::Fullscreen {
                                x: old_bounds.0,
                                y: old_bounds.1,
                                w: old_bounds.2,
                                h: old_bounds.3,
                            }
                        } else if window.is_maximized() {
                            interface_config::WindowBounds::Maximized {
                                x: old_bounds.0,
                                y: old_bounds.1,
                                w: old_bounds.2,
                                h: old_bounds.3,
                            }
                        } else {
                            interface_config::WindowBounds::Windowed {
                                x: new_bounds.0,
                                y: new_bounds.1,
                                w: new_bounds.2,
                                h: new_bounds.3,
                            }
                        };

                        if new_window_bounds != old_window_bounds {
                            InterfaceConfig::get_mut(cx).main_window_bounds = new_window_bounds;
                        }
                    })
                    .detach();

                    LauncherRoot::new(&data, window, cx)
                });

                // Closing the main window while instances optimize their launch asks first.
                window.on_window_should_close(cx, {
                    let data = data.clone();
                    move |window, cx| {
                        modals::aot_optimization::confirm_close(&data, window, cx, |window, _| window.remove_window())
                    }
                });

                DataEntities::init_globals(cx);
                cx.set_global(LauncherRootGlobal {
                    root: launcher_root.clone(),
                });
                if capture_page.as_deref() == Some("appearance") {
                    crate::settings::open_settings_window(window, &data, cx);
                }
                let root = cx.new(|cx| Root::new(launcher_root, window, cx));
                if let Some(name) = capture_page.as_deref().and_then(|page| page.strip_prefix("update:")) {
                    let name: SharedString = name.to_owned().into();
                    let capture_data = data.clone();
                    window.on_next_frame(move |window, cx| {
                        if let Some(id) = crate::entity::instance::InstanceEntries::find_id_by_name(
                            &capture_data.instances,
                            &name,
                            cx,
                        ) {
                            crate::root::capture_inherited_update(id, name, &capture_data, window, cx);
                        }
                    });
                }
                if let Some(page) = capture_page
                    .as_deref()
                    .filter(|page| page.starts_with("derived:") || page.starts_with("duplicate:"))
                {
                    let branch = page.starts_with("derived:");
                    let name: SharedString = page.split_once(':').unwrap().1.to_owned().into();
                    let capture_data = data.clone();
                    window.on_next_frame(move |window, cx| {
                        if let Some(id) = crate::entity::instance::InstanceEntries::find_id_by_name(
                            &capture_data.instances,
                            &name,
                            cx,
                        ) {
                            if branch {
                                crate::modals::duplicate_instance::open_derived_instance(
                                    id,
                                    name,
                                    capture_data.instances.clone(),
                                    capture_data.backend_handle.clone(),
                                    window,
                                    cx,
                                );
                            } else {
                                crate::modals::duplicate_instance::open_duplicate_instance(
                                    id,
                                    name,
                                    capture_data.instances.clone(),
                                    capture_data.backend_handle.clone(),
                                    window,
                                    cx,
                                );
                            }
                        }
                    });
                }
                if capture_page.as_deref() == Some("components-dialog") {
                    window.on_next_frame(crate::component::design_review::open_review_dialog);
                }
                if capture_page.as_deref() == Some("accounts") {
                    window.on_next_frame(|_, cx| crate::root::open_home_accounts(cx));
                }
                root
            },
        )
        .unwrap();

    cx.activate(true);

    handle.into()
}

pub(crate) fn is_valid_instance_name(name: &str) -> bool {
    is_single_component_path(name)
        && sanitize_filename::is_sanitized_with_options(
            name,
            sanitize_filename::OptionsForCheck {
                windows: true,
                ..Default::default()
            },
        )
}

pub(crate) fn is_single_component_path(path: &str) -> bool {
    let path = std::path::Path::new(path);
    let mut components = path.components().peekable();

    if let Some(first) = components.peek()
        && !matches!(first, std::path::Component::Normal(_))
    {
        return false;
    }

    components.count() == 1
}

pub(crate) fn get_unique_instance_name(original_name: &str, existing_names: &[&str]) -> String {
    if !existing_names.iter().any(|n| *n == original_name) {
        return original_name.to_string();
    }
    for i in 1..100 {
        let numbered = format!("{original_name} ({i})");
        if !existing_names.iter().any(|n| *n == &numbered) {
            return numbered;
        }
    }
    String::new()
}

#[inline]
pub(crate) fn labelled(label: impl Into<SharedString>, element: impl IntoElement) -> Div {
    gpui_component::v_flex()
        .gap_0p5()
        .child(div().text_sm().font_medium().child(label.into()))
        .child(element)
}

pub(crate) fn open_folder(path: &Path, window: &mut Window, cx: &mut App) {
    let mut is_dir = path.is_dir();
    if !is_dir && !path.exists() {
        if let Err(err) = std::fs::create_dir_all(path) {
            let notification: Notification =
                (NotificationType::Error, SharedString::new(t::file_system::open_folder::error(err))).into();
            window.push_notification(notification.autohide(false), cx);
            return;
        }
        is_dir = path.is_dir();
    }
    if is_dir {
        let canonical_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        #[cfg(windows)]
        let canonical_path = {
            let value = canonical_path.as_os_str().to_string_lossy();
            if let Some(path) = value.strip_prefix(r"\\?\UNC\") {
                PathBuf::from(format!(r"\\{path}"))
            } else if let Some(path) = value.strip_prefix(r"\\?\") {
                PathBuf::from(path)
            } else {
                canonical_path
            }
        };
        #[cfg(windows)]
        let open_result = std::process::Command::new("explorer.exe").arg(&canonical_path).spawn().map(|_| ());
        #[cfg(not(windows))]
        let open_result = open::that_detached(canonical_path);

        if let Err(err) = open_result {
            let notification: Notification =
                (NotificationType::Error, SharedString::new(t::file_system::open_folder::error(err))).into();
            window.push_notification(notification.autohide(false), cx);
        }
    } else {
        let notification: Notification =
            (NotificationType::Error, t::file_system::open_folder::not_a_directory()).into();
        window.push_notification(notification.autohide(false), cx);
    }
}

pub fn format_downloads(downloads: u64) -> SharedString {
    if downloads >= 1_000_000_000 {
        t::instance::content::downloads::b((downloads / 10_000_000) as f64 / 100.0)
    } else if downloads >= 1_000_000 {
        t::instance::content::downloads::m((downloads / 10_000) as f64 / 100.0)
    } else if downloads >= 10_000 {
        t::instance::content::downloads::k((downloads / 10) as f64 / 100.0)
    } else {
        t::instance::content::downloads::n(downloads)
    }
    .into()
}

/// Additional capture routes only select presentation on an explicitly owned CLI fixture.
fn capture_instance_target(page: &str) -> Option<(&str, pages::instance::instance_page::InstanceSubpageType)> {
    use pages::instance::instance_page::InstanceSubpageType as Tab;
    let (route, name) = page.split_once(':')?;
    if !name.starts_with("agente-274-") {
        return None;
    }
    let tab = match route {
        "instance-mods" => Tab::Mods,
        "instance-resources" => Tab::ResourcePacks,
        "instance-shaders" => Tab::Shaders,
        "instance-logs" | "instance-crashes" => Tab::Logs,
        "instance-overwrites" => Tab::Overwrites,
        "instance-changelog" => Tab::Changelog,
        "instance-settings" | "derived" | "duplicate" => Tab::Settings,
        "instance-live" => Tab::LiveGameOutput,
        _ => return None,
    };
    Some((name, tab))
}
