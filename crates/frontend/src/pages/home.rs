//! Selected instance Home; native launch flow and mutable cosmetic data.
use crate::{
    component::design::{self, DesignButton, PlayButton},
    entity::{DataEntities, instance::InstanceEntry},
    interface_config::InterfaceConfig,
    pages::{instance::instance_page::InstanceSubpageType, page::Page},
    root, theme,
    ui::PageType,
};
use bridge::{
    home::HomePresentation,
    instance::{InstanceServerSummary, InstanceStatus},
    message::{MessageToBackend, QuickPlayLaunch},
    serial::AtomicOptionSerial,
};
use gpui::{prelude::*, *};
use gpui_component::{Disableable, WindowExt, h_flex, input::InputState, scroll::ScrollableElement, v_flex};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
// The Home view is recreated while navigating; retain decoded visuals across
// views so an unresolved selection never flashes the Vanilla fallback.
#[derive(Default)]
struct HomeVisualCache {
    artwork: crate::component::retained_image::RetainedImage,
    logo: crate::component::retained_image::RetainedImage,
}
impl Global for HomeVisualCache {}
#[derive(Default)]
struct ServerMarquee {
    text: String,
    started: Option<Instant>,
}
impl ServerMarquee {
    fn elapsed(&mut self, text: &str, now: Instant) -> Duration {
        if self.started.is_none() || self.text != text {
            self.text = text.to_owned();
            self.started = Some(now);
        }
        now.duration_since(self.started.unwrap())
    }
}
struct ServerPlayerTooltip(SharedString);
impl Render for ServerPlayerTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .bg(rgb(0x100010))
            .border_2()
            .border_color(rgb(0x5000a0))
            .px(px(6.))
            .py(px(3.))
            .font_family("Minecraft Default")
            .text_size(px(13.))
            .text_color(rgb(0xffffff))
            .child(self.0.clone())
    }
}

pub struct HomePage {
    data: DataEntities,
    instance: Option<Entity<InstanceEntry>>,
    presentation: HomePresentation,
    loaded: bool,
    visual_resolved: bool,
    news_loaded: bool,
    news_previews: std::collections::BTreeMap<String, String>,
    startup_cached: bool,
    artwork_image: crate::component::retained_image::RetainedImage,
    logo_image: crate::component::retained_image::RetainedImage,
    requested: bool,
    refreshed: Instant,
    menu: bool,
    news_view: bool,
    news_page: Option<Entity<super::news::NewsPage>>,
    article: Option<String>,
    heads: std::collections::BTreeMap<String, std::path::PathBuf>,
    head_ids: Vec<String>,
    selected_server: Option<InstanceServerSummary>,
    selected_server_pending: bool,
    selected_server_checked: Option<Instant>,
    server_name_marquee: ServerMarquee,
    server_motd_marquee: ServerMarquee,
    timer: Option<Task<()>>,
    server_serial: AtomicOptionSerial,
}
fn embedded(name: &'static str) -> ImageSource {
    ImageSource::Resource(Resource::Embedded(name.into()))
}
fn picture(path: &Option<std::path::PathBuf>, fallback: &'static str) -> ImageSource {
    path.as_ref()
        .map(|p| ImageSource::Resource(Resource::Path(Arc::from(p.as_path()))))
        .unwrap_or_else(|| embedded(fallback))
}
fn shade(from: u32, to: u32) -> Background {
    linear_gradient(180., linear_color_stop(rgba(from), 0.), linear_color_stop(rgba(to), 1.)).into()
}

fn configure_home_server(instance: &InstanceEntry, data: &DataEntities, window: &mut Window, cx: &mut App) {
    use schema::instance::HomeServerSelection;
    let initial = instance.configuration.home_server.clone();
    let servers = instance.servers.read(cx).clone().unwrap_or_default();
    let initial_mode = match &initial {
        None => 0,
        Some(HomeServerSelection::Instance { .. }) => 1,
        Some(HomeServerSelection::Custom { .. }) => 2,
    };
    let initial_index = match &initial {
        Some(HomeServerSelection::Instance { address }) => servers.iter().position(|s| s.ip.as_ref() == address),
        _ => None,
    };
    let (initial_name, initial_address) = match &initial {
        Some(HomeServerSelection::Custom { name, address }) => (name.clone(), address.clone()),
        _ => (String::new(), String::new()),
    };
    let mode = cx.new(|_| initial_mode);
    let chosen = cx.new(|_| initial_index);
    let name = cx.new(|cx| InputState::new(window, cx).default_value(initial_name));
    let address = cx.new(|cx| InputState::new(window, cx).default_value(initial_address));
    let id = instance.id;
    let backend = data.backend_handle.clone();
    window.open_dialog(cx, move |dialog, _, cx| {
        let current_mode = *mode.read(cx);
        let current_choice = *chosen.read(cx);
        let entered_name = name.read(cx).value();
        let entered_address = address.read(cx).value();
        let mut modes = v_flex().gap(px(6.));
        for (index, label) in [
            "Detectar primer servidor de la instancia",
            "Elegir servidor de la instancia",
            "Servidor personalizado",
        ]
        .into_iter()
        .enumerate()
        {
            let mode = mode.clone();
            modes = modes.child(design::chip(("home-server-mode", index), label, current_mode == index).on_click(
                move |_, _, cx| {
                    mode.update(cx, |value, cx| {
                        *value = index;
                        cx.notify();
                    })
                },
            ));
        }
        let mut choices = v_flex().gap(px(4.));
        for (index, server) in servers.iter().enumerate() {
            let chosen = chosen.clone();
            choices = choices.child(
                design::chip(
                    ("home-server-choice", index),
                    SharedString::from(server.name.clone()),
                    current_choice == Some(index),
                )
                .on_click(move |_, _, cx| {
                    chosen.update(cx, |value, cx| {
                        *value = Some(index);
                        cx.notify();
                    })
                }),
            );
        }
        let valid = current_mode == 0
            || (current_mode == 1 && current_choice.is_some_and(|i| i < servers.len()))
            || (current_mode == 2 && !entered_name.trim().is_empty() && !entered_address.trim().is_empty());
        let selection = match current_mode {
            1 => current_choice.and_then(|i| servers.get(i)).map(|s| HomeServerSelection::Instance {
                address: s.ip.to_string(),
            }),
            2 => Some(HomeServerSelection::Custom {
                name: entered_name.trim().to_owned(),
                address: entered_address.trim().to_owned(),
            }),
            _ => None,
        };
        let backend = backend.clone();
        design::dialog(dialog)
            .title(design::dialog_title("Server"))
            .child(v_flex().gap(px(12.)).child(modes).when(current_mode == 1, |d| d.child(choices)).when(
                current_mode == 2,
                |d| {
                    d.child(crate::labelled("Nombre", design::text_field(&name)))
                        .child(crate::labelled("Dirección", design::text_field(&address)))
                },
            ))
            .footer(
                h_flex()
                    .gap(px(8.))
                    .justify_end()
                    .child(
                        design::action_button("home-server-cancel", "Cancelar", design::ButtonAction::Secondary)
                            .on_click(|_, w, c| w.close_dialog(c)),
                    )
                    .child(
                        design::action_button("home-server-save", "Guardar", design::ButtonAction::Primary)
                            .disabled(!valid)
                            .on_click(move |_, w, c| {
                                backend.send(MessageToBackend::SetHomeServer {
                                    id,
                                    selection: selection.clone(),
                                });
                                w.close_dialog(c);
                            }),
                    ),
            )
    });
}
#[cfg(feature = "ui-capture")]
pub(crate) fn capture_server_settings(data: &DataEntities, window: &mut Window, cx: &mut App) {
    let selected = InterfaceConfig::get(cx).home_instance.clone();
    let entries = data.instances.read(cx).entries.clone();
    let instance = selected
        .and_then(|name| entries.values().find(|entry| entry.read(cx).name == name).cloned())
        .or_else(|| entries.values().next().cloned());
    if let Some(instance) = instance {
        let instance = instance.read(cx).clone();
        configure_home_server(&instance, data, window, cx);
    }
}
fn server_frame(content: impl IntoElement) -> Div {
    div()
        .absolute()
        .top(px(60.))
        .right(px(18.))
        .w(relative(0.2))
        .bg(rgba(0x060607cc))
        .border_1()
        .border_color(rgba(0xffffff20))
        .px(px(10.))
        .py(px(8.))
        .child(content)
}
fn home_server_marquee(
    state: &mut ServerMarquee,
    text: &str,
    styles: &[schema::text_component::TextComponentRun],
    size: f32,
    available: f32,
    window: &mut Window,
    content: impl IntoElement,
) -> Div {
    let runs: Vec<TextRun> = text
        .char_indices()
        .map(|(index, ch)| {
            let mut run = window.text_style().to_run(ch.len_utf8());
            run.font = font("Minecraft Default");
            for style in styles.iter().filter(|run| run.range.contains(&index)) {
                if let Some(bold) = style.style.bold {
                    run.font.weight = if bold { FontWeight::BOLD } else { FontWeight::NORMAL };
                }
                // Match the existing styled MOTD renderer's font selection.
                if let Some(italic) = style.style.italic {
                    run.font.style = if italic { FontStyle::Normal } else { FontStyle::Italic };
                }
            }
            run
        })
        .collect();
    let width = window.text_system().layout_line(text, px(size), &runs, None).width.as_f32();
    let elapsed = state.elapsed(text, Instant::now()).as_secs_f32();
    let mut x = 0.;
    if width > available {
        let distance = (text.chars().count() as f32 * 8. - available).max(1.);
        let old_seconds = (distance / 24. + 2.).clamp(3., 30.);
        let speed = (distance / (old_seconds * 0.5)).max(1.);
        let gap = 20.;
        let offset = (elapsed * speed) % (width + available + gap);
        x = if offset <= width {
            -offset
        } else {
            available + gap - (offset - width)
        };
        window.request_animation_frame();
    }
    div()
        .relative()
        .w_full()
        .h(px(18.))
        .overflow_hidden()
        .child(div().absolute().top_0().left(px(x)).whitespace_nowrap().child(content))
}
pub fn add_instance(_data: &DataEntities, window: &mut Window, cx: &mut App) {
    root::switch_page(PageType::AddInstance, &[PageType::Instances], window, cx);
}
#[cfg(test)]
mod server_marquee_tests {
    use super::ServerMarquee;
    use std::time::{Duration, Instant};
    #[test]
    fn unchanged_ping_does_not_restart_the_text_clock() {
        let start = Instant::now();
        let mut marquee = ServerMarquee::default();
        assert_eq!(marquee.elapsed("motd", start), Duration::ZERO);
        assert_eq!(marquee.elapsed("motd", start + Duration::from_secs(31)), Duration::from_secs(31));
        assert_eq!(marquee.elapsed("motd", start + Duration::from_secs(61)), Duration::from_secs(61));
        assert_eq!(marquee.elapsed("changed", start + Duration::from_secs(62)), Duration::ZERO);
    }
}

impl HomePage {
    pub fn new(data: &DataEntities, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let entries = data.instances.read(cx).entries.clone();
        let selected = InterfaceConfig::get(cx).home_instance.clone();
        let instance = selected
            .and_then(|name| entries.values().find(|e| e.read(cx).name == name).cloned())
            .or_else(|| entries.values().next().cloned());
        if let Some(i) = &instance {
            InterfaceConfig::get_mut(cx).home_instance = Some(i.read(cx).name.clone());
            cx.observe(i, |_, _, cx| cx.notify()).detach();
            let servers = i.read(cx).servers.clone();
            cx.observe(&servers, |_, _, cx| cx.notify()).detach();
        }
        let (artwork_image, logo_image) = {
            let cache = cx.default_global::<HomeVisualCache>();
            (cache.artwork.clone(), cache.logo.clone())
        };
        let mut page = Self {
            data: data.clone(),
            instance,
            presentation: HomePresentation::default(),
            loaded: false,
            visual_resolved: false,
            news_loaded: false,
            news_previews: Default::default(),
            startup_cached: !std::env::var("WBOMBO_UI_CAPTURE_PAGE").is_ok_and(|page| page != "startup-live"),
            artwork_image,
            logo_image,
            requested: false,
            refreshed: Instant::now(),
            menu: std::env::var("WBOMBO_UI_CAPTURE_PAGE").is_ok_and(|p| p == "home-menu"),
            news_view: std::env::var("WBOMBO_UI_CAPTURE_PAGE")
                .is_ok_and(|p| p == "home-news" || p.starts_with("news-")),
            news_page: None,
            article: None,
            heads: Default::default(),
            head_ids: Vec::new(),
            selected_server: None,
            selected_server_pending: false,
            selected_server_checked: None,
            server_name_marquee: Default::default(),
            server_motd_marquee: Default::default(),
            timer: None,
            server_serial: AtomicOptionSerial::default(),
        };
        page.refresh(window, cx);
        page.timer = Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(30)).await;
                if this
                    .update(cx, |this, cx| {
                        if let Some(i) = &this.instance {
                            this.data.backend_handle.send(MessageToBackend::RefreshHomeServer { id: i.read(cx).id });
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
        page
    }
    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.requested {
            return;
        }
        self.requested = true;
        let include_news = !self.startup_cached;
        let (send, receive) = tokio::sync::oneshot::channel();
        self.data.backend_handle.send(MessageToBackend::GetHomePresentation {
            id: self.instance.as_ref().map(|i| i.read(cx).id),
            cache_only: self.startup_cached,
            include_news,
            channel: send,
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = receive.await;
            let _ = this.update(cx, |this, cx| {
                let presentation = result.ok().and_then(Result::ok);
                this.visual_resolved = presentation.is_some();
                this.presentation = presentation.unwrap_or_else(|| HomePresentation {
                    news_failed: true,
                    ..Default::default()
                });
                this.news_previews = this
                    .presentation
                    .news
                    .iter()
                    .map(|n| (n.id.clone(), super::news::preview(&n.body)))
                    .collect();
                log::info!("home-ready pid={}", std::process::id());
                this.loaded = true;
                this.news_loaded = include_news;
                this.requested = false;
                // The initial cache read never waits for network. Revalidate
                // cosmetic data independently after the first local scene.
                this.refreshed = if this.startup_cached {
                    this.startup_cached = false;
                    Instant::now() - Duration::from_secs(300)
                } else {
                    Instant::now()
                };
                cx.notify();
            });
        })
        .detach();
    }
    pub fn has_unread(&self, cx: &App) -> bool {
        self.presentation.news.iter().any(|n| {
            !InterfaceConfig::get(cx)
                .news_marks
                .get(&self.presentation.news_origin_key)
                .map(|s| &s.read)
                .unwrap_or(&InterfaceConfig::get(cx).home_read_news)
                .contains(&n.id)
        })
    }
    pub fn is_news(&self) -> bool {
        self.news_view
    }
    pub(crate) fn startup_ready(&self, window: &mut Window, cx: &mut App) -> bool {
        let mut ready = self.loaded;
        for source in [
            picture(&self.presentation.artwork, "images/artwork-vanilla.png"),
            picture(&self.presentation.logo, "images/logo-vanilla.png"),
        ]
        .into_iter()
        {
            if let ImageSource::Resource(resource) = source {
                ready &= window.use_asset::<ImgResourceLoader>(&resource, cx).is_some();
            }
        }
        ready
    }
    pub fn show_home(&mut self, cx: &mut Context<Self>) {
        self.news_view = false;
        self.article = None;
        cx.notify();
    }
    pub fn instance_id(&self, cx: &App) -> Option<bridge::instance::InstanceID> {
        self.instance.as_ref().map(|i| i.read(cx).id)
    }
    pub fn show_news(&mut self, cx: &mut Context<Self>) {
        self.news_view = true;
        self.article = None;
        self.news_page = None;
        cx.notify();
    }
    fn request_heads(&mut self, server: &InstanceServerSummary, window: &mut Window, cx: &mut Context<Self>) {
        if InterfaceConfig::get(cx).hide_skins {
            return;
        }
        let ids = server
            .status
            .as_ref()
            .and_then(|s| s.players.as_ref())
            .map(|p| p.sample.iter().take(12).map(|p| p.id.to_string()).collect::<Vec<_>>())
            .unwrap_or_default();
        if ids == self.head_ids {
            return;
        }
        self.head_ids = ids.clone();
        let (send, receive) = tokio::sync::oneshot::channel();
        self.data.backend_handle.send(MessageToBackend::GetHomePlayerHeads {
            uuids: ids,
            channel: send,
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(heads) = receive.await {
                let _ = this.update(cx, |this, cx| {
                    this.heads.extend(heads);
                    cx.notify();
                });
            }
        })
        .detach();
    }
    fn request_selected_server(
        &mut self,
        mut target: InstanceServerSummary,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> InstanceServerSummary {
        let changed = self.selected_server.as_ref().is_none_or(|server| server.ip != target.ip);
        if changed {
            self.selected_server = Some(target.clone());
            self.selected_server_pending = false;
            self.selected_server_checked = None;
        } else if let Some(current) = &self.selected_server {
            target.status = current.status.clone();
            target.ping = current.ping;
            target.pinging = current.pinging;
            if target.png_icon.is_none() {
                target.png_icon = current.png_icon.clone();
            }
        }
        let interval = if target.pinging {
            Duration::from_millis(500)
        } else {
            Duration::from_secs(30)
        };
        if !self.selected_server_pending && self.selected_server_checked.is_none_or(|time| time.elapsed() >= interval) {
            self.selected_server_pending = true;
            self.selected_server_checked = Some(Instant::now());
            target.pinging = target.status.is_none();
            let address = target.ip.clone();
            let (send, receive) = tokio::sync::oneshot::channel();
            if let Some(instance) = &self.instance {
                self.data.backend_handle.send(MessageToBackend::GetHomeServerStatus {
                    id: instance.read(cx).id,
                    address: address.clone(),
                    channel: send,
                });
                cx.spawn_in(window, async move |this, cx| {
                    let response = receive.await.ok().flatten();
                    let _ = this.update(cx, |this, cx| {
                        if let Some(current) = this.selected_server.as_mut().filter(|s| s.ip == address) {
                            if let Some(response) = response {
                                current.pinging = response.pinging;
                                current.status = response.status;
                                current.ping = response.ping;
                                current.png_icon = response.png_icon;
                            }
                            this.selected_server_pending = false;
                            cx.notify();
                        }
                    });
                    cx.background_executor().timer(Duration::from_millis(550)).await;
                    let _ = this.update(cx, |_, cx| cx.notify());
                })
                .detach();
            }
        }
        target
    }
    fn open_tab(&self, tab: InstanceSubpageType, window: &mut Window, cx: &mut App) {
        if let Some(i) = &self.instance {
            let name = i.read(cx).name.clone();
            InterfaceConfig::get_mut(cx).instance_subpage = tab;
            root::switch_page(PageType::InstancePage { name }, &[PageType::Instances], window, cx);
        }
    }
    fn server_header(&self) -> Div {
        let instance = self.instance.as_ref().unwrap().clone();
        let data = self.data.clone();
        h_flex()
            .justify_between()
            .items_center()
            .child(div().font_family("Minecraft Default").text_size(px(14.)).child("Server"))
            .child(
                DesignButton::new("home-server-settings")
                    .aria_label("Server")
                    .w(px(26.))
                    .h(px(26.))
                    .px_0()
                    .child(gpui_component::Icon::new(crate::icon::PandoraIcon::Settings).size(px(14.)))
                    .on_click(move |_, window, cx| {
                        let instance = instance.read(cx).clone();
                        configure_home_server(&instance, &data, window, cx);
                    }),
            )
    }
    fn server_panel(
        &mut self,
        server: &InstanceServerSummary,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let online = server.status.as_ref().and_then(|s| s.players.as_ref());
        let ip = server.ip.clone();
        let data = self.data.clone();
        let i = self.instance.as_ref().unwrap().read(cx).clone();
        let id = i.id;
        let name = i.name.clone();
        let text_width = ((window.viewport_size().width.as_f32() - 220.) * 0.2 - 20.).max(1.);
        let mut panel = v_flex().gap(px(5.)).child(self.server_header());
        let server_name = server.name.clone();
        let icon = server.png_icon.clone().map(|bytes| {
            crate::png_render_cache::render_with_transform(
                bytes,
                crate::png_render_cache::ImageTransformation::Resize { width: 32, height: 32 },
                cx,
            )
        });
        let detail_width = (text_width - if icon.is_some() { 40. } else { 0. }).max(1.);
        let mut details = v_flex().flex_1().min_w_0().child(home_server_marquee(
            &mut self.server_name_marquee,
            &server_name,
            &[],
            15.,
            detail_width,
            window,
            div()
                .font_family("Minecraft Default")
                .text_size(px(15.))
                .text_color(rgb(theme::INK))
                .child(SharedString::from(server_name.clone())),
        ));
        if let Some(status) = &server.status {
            let motd = schema::text_component::FlatTextComponent {
                content: status.description.content.replace(['\r', '\n'], " "),
                runs: status
                    .description
                    .runs
                    .iter()
                    .map(|run| schema::text_component::TextComponentRun {
                        range: run.range.clone(),
                        style: run.style.clone(),
                    })
                    .collect(),
            };
            details = details.child(home_server_marquee(
                &mut self.server_motd_marquee,
                &motd.content,
                &motd.runs,
                13.,
                detail_width,
                window,
                div()
                    .font_family("Minecraft Default")
                    .text_size(px(13.))
                    .child(crate::component::create_styled_text(&motd, false)),
            ));
        }
        panel = panel.child(
            h_flex()
                .gap(px(6.))
                .items_center()
                .when_some(icon, |d, icon| d.child(icon.size(px(32.))))
                .child(details),
        );
        panel = panel.child(
            h_flex()
                .justify_between()
                .items_center()
                .gap(px(4.))
                .child(
                    h_flex()
                        .items_center()
                        .gap(px(4.))
                        .when(server.status.is_none() && !server.pinging, |d| {
                            d.child(div().size(px(5.)).flex_shrink_0().bg(rgb(theme::INK_DISABLED)))
                        })
                        .child(
                            div()
                                .font_family("Minecraft Default")
                                .text_size(px(12.))
                                .text_color(rgb(theme::INK_MUTED))
                                .child(if server.pinging && server.status.is_none() {
                                    "CARGANDO"
                                } else if server.status.is_some() {
                                    "Servidor en línea"
                                } else {
                                    "Servidor sin respuesta"
                                }),
                        ),
                )
                .when_some(online, |d, players| {
                    d.child(
                        div()
                            .font_family("Minecraft Default")
                            .text_size(px(12.))
                            .child(format!("{} / {}", players.online, players.max)),
                    )
                }),
        );
        if let Some(players) = online.filter(|p| !p.sample.is_empty()) {
            let hide_names = InterfaceConfig::get(cx).hide_usernames;
            let hide_skins = InterfaceConfig::get(cx).hide_skins;
            panel = panel.child(h_flex().flex_wrap().gap(px(3.)).children(
                players.sample.iter().take(12).enumerate().map(|(ix, player)| {
                    let player_name = SharedString::from(player.name.clone());
                    let head = img(picture(
                        &if hide_skins {
                            None
                        } else {
                            self.heads.get(player.id.as_ref()).cloned()
                        },
                        "images/default_head.png",
                    ))
                    .size(px(20.));
                    div().id(("home-server-player", ix)).child(head).tooltip_show_delay(Duration::ZERO).when(
                        !hide_names,
                        |d| {
                            d.tooltip(move |window, cx| {
                                let _ = window;
                                cx.new(|_| ServerPlayerTooltip(player_name.clone())).into()
                            })
                        },
                    )
                }),
            ));
        }
        let join = DesignButton::new("quick-server")
            .aria_label("Entrar al servidor")
            .w_full()
            .h(px(24.))
            .px_0()
            .border_0()
            .paint(
                0.,
                [[0x666666ff; 2], [0x777777ff; 2], [0x555555ff; 2]],
                rgb(0xffffff).into(),
                rgb(0xffffff).into(),
            )
            .child(
                div()
                    .font_family("Minecraft Default")
                    .text_size(px(12.))
                    .text_color(rgb(0xffffff))
                    .child("Entrar al servidor"),
            )
            .on_click(move |_, w, c| {
                if data.accounts.read(c).selected_account.is_none() {
                    root::open_home_accounts(c);
                    return;
                }
                root::start_instance(
                    id,
                    name.clone(),
                    Some(QuickPlayLaunch::Multiplayer(ip.to_string().into())),
                    &data,
                    w,
                    c,
                )
            });
        let beveled_join = div().w_full().bg(rgb(0x101010)).p(px(2.)).child(
            div()
                .bg(rgb(0xaaaaaa))
                .pt(px(2.))
                .pl(px(2.))
                .child(div().bg(rgb(0x333333)).pb(px(2.)).pr(px(2.)).child(join)),
        );
        server_frame(panel.child(beveled_join)).into_any_element()
    }
}
impl Page for HomePage {
    fn controls(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
    fn scrollable(&self, _: &App) -> bool {
        false
    }
}
impl Render for HomePage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.loaded && !self.news_loaded
            || (self.refreshed.elapsed() >= Duration::from_secs(300) && !crate::startup::is_active(cx))
        {
            self.refresh(window, cx);
        }
        if self.news_view {
            if !self.loaded {
                return div().size_full().into_any_element();
            }
            if self.news_page.is_none() {
                let seed = self
                    .article
                    .as_ref()
                    .and_then(|id| self.presentation.news.iter().find(|n| &n.id == id))
                    .cloned();
                self.news_page = Some(cx.new(|c| {
                    super::news::NewsPage::new(&self.data, self.presentation.profile_id.clone(), seed, window, c)
                }));
            }
            return div().size_full().child(self.news_page.clone().unwrap()).into_any_element();
        }
        let instance = self.instance.as_ref().map(|i| i.read(cx).clone());
        #[cfg(feature = "ui-capture")]
        if instance.is_some()
            && std::env::var("WBOMBO_UI_CAPTURE_PAGE").is_ok_and(|p| p == "home-running" || p == "home-optimizing")
        {
            log::info!("home-fixture-ready pid={}", std::process::id());
        }
        let mut server = None;
        let mut server_loading = false;
        let mut selected_server_missing = false;
        if let Some(i) = &instance {
            i.servers_state.set_observed();
            if i.servers_state.should_load() {
                self.data
                    .backend_handle
                    .send_with_serial(MessageToBackend::RequestLoadServers { id: i.id }, &self.server_serial);
            }
            server = i.servers.read(cx).as_ref().and_then(|s| s.first()).cloned();
            server_loading = i.servers.read(cx).is_none();
        }
        if let Some(i) = &instance {
            use schema::instance::HomeServerSelection;
            let target = match &i.configuration.home_server {
                Some(HomeServerSelection::Instance { address }) => i
                    .servers
                    .read(cx)
                    .as_ref()
                    .and_then(|servers| servers.iter().find(|s| s.ip.as_ref() == address).cloned()),
                Some(HomeServerSelection::Custom { name, address }) => Some(InstanceServerSummary {
                    name: name.clone().into(),
                    ip: address.clone().into(),
                    png_icon: None,
                    pinging: true,
                    status: None,
                    ping: None,
                }),
                None => None,
            };
            if i.configuration.home_server.is_some() {
                selected_server_missing =
                    matches!(&i.configuration.home_server, Some(HomeServerSelection::Instance { .. }))
                        && i.servers.read(cx).is_some()
                        && target.is_none();
                server = target.map(|target| self.request_selected_server(target, window, cx));
                server_loading = i.servers.read(cx).is_none() && server.is_none();
            }
        }
        if let Some(server) = &server {
            self.request_heads(server, window, cx);
        }
        let mut header = h_flex()
            .window_control_area(WindowControlArea::Drag)
            .h(px(48.))
            .px(px(24.))
            .gap(px(20.))
            .items_center()
            .bg(shade(0x0b0b0ce6, 0x0b0b0c00));
        let mut tabs = h_flex().h_full().gap(px(20.)).flex_shrink_0();
        if let Some(i) = &instance {
            header = header.child(
                div()
                    .max_w(px(200.))
                    .flex_shrink_0()
                    .overflow_hidden()
                    .font_family(theme::DISPLAY)
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(crate::component::tracked_text::TrackedText::new(i.name.to_uppercase(), 0.16)),
            );
            for (index, label, tab) in [
                (0, "Jugar", None),
                (1, "Contenido", Some(InstanceSubpageType::Mods)),
                (2, "Sobrescrituras", Some(InstanceSubpageType::Overwrites)),
                (3, "Registros", Some(InstanceSubpageType::Logs)),
                (5, "Changelog", Some(InstanceSubpageType::Changelog)),
                (4, "Ajustes de la instancia", Some(InstanceSubpageType::Settings)),
            ] {
                if tab == Some(InstanceSubpageType::Changelog) && self.presentation.profile_id.is_none() {
                    continue;
                }
                tabs = tabs.child(design::tab(("home-tab", index as usize), label, index == 0).on_click(cx.listener(
                    move |this, _, w, c| {
                        if let Some(t) = tab {
                            this.open_tab(t, w, c);
                        }
                    },
                )));
            }
        }
        header = header.child(div().flex_1().min_w_0().h_full().overflow_x_scrollbar().child(tabs));
        if crate::root::should_render_custom_titlebar() && !cfg!(target_os = "macos") {
            use crate::component::generic_title_bar::WindowControl;
            header = header.child(
                h_flex()
                    .flex_shrink_0()
                    .child(WindowControl::Minimize)
                    .child(if window.is_maximized() {
                        WindowControl::Restore
                    } else {
                        WindowControl::Maximize
                    })
                    .child(WindowControl::Close),
            );
        }
        let resource = |path: &Option<std::path::PathBuf>, fallback: &'static str| {
            path.as_ref()
                .map(|path| Resource::Path(Arc::from(path.as_path())))
                .unwrap_or_else(|| Resource::Embedded(fallback.into()))
        };
        let hero_height = (window.viewport_size().height.as_f32() * 0.64).max(320.);
        let artwork = self.artwork_image.render(
            self.visual_resolved
                .then(|| resource(&self.presentation.artwork, "images/artwork-vanilla.png")),
            "images/artwork-vanilla.png",
            true,
            cx.entity().entity_id(),
            window,
            cx,
        );
        let logo = self.logo_image.render(
            self.visual_resolved.then(|| resource(&self.presentation.logo, "images/logo-vanilla.png")),
            "images/logo-vanilla.png",
            false,
            cx.entity().entity_id(),
            window,
            cx,
        );
        {
            let cache = cx.default_global::<HomeVisualCache>();
            cache.artwork = self.artwork_image.clone();
            cache.logo = self.logo_image.clone();
        }
        let hero = div()
            .relative()
            .h(px(hero_height))
            .w_full()
            .flex_shrink_0()
            .overflow_hidden()
            .child(div().absolute().size_full().child(artwork))
            .child(div().absolute().bottom_0().w_full().h(relative(0.55)).bg(shade(0x06060700, 0x060607ff)))
            .child(
                div()
                    .absolute()
                    .top(px(70.))
                    .left_0()
                    .w_full()
                    .pr(px(if instance.is_some() { 60. } else { 0. }))
                    .flex()
                    .justify_center()
                    .child(div().child(logo).w(px(370.)).h(px(70.))),
            )
            .when_some(server.as_ref(), |d, s| d.child(self.server_panel(s, window, cx)))
            .when(
                selected_server_missing || (instance.is_some() && server.is_none() && !server_loading),
                |d| d.child(server_frame(self.server_header())),
            )
            .when(server_loading, |d| {
                d.child(server_frame(
                    v_flex()
                        .gap(px(6.))
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_size(px(13.))
                                .line_height(px(18.))
                                .child("Server"),
                        )
                        .child(div().text_size(px(11.)).text_color(rgb(theme::INK_MUTED)).child("CARGANDO")),
                ))
            })
            .when_some(
                self.presentation.artwork_author.clone().filter(|_| self.presentation.artwork.is_some()),
                |d, a| {
                    d.child(
                        div()
                            .absolute()
                            .right(px(20.))
                            .bottom(px(18.))
                            .text_size(px(10.))
                            .text_color(rgb(theme::INK_DISABLED))
                            .child(format!("Artwork de la comunidad · {a}")),
                    )
                },
            );
        let data = self.data.clone();
        let mut play = PlayButton::new("home-play", if instance.is_some() { "Al día" } else { "" }, None);
        if let Some(i) = &instance {
            if self.visual_resolved
                && i.status == InstanceStatus::Running
                && InterfaceConfig::get(cx).last_played_artwork != self.presentation.artwork
            {
                let config = InterfaceConfig::get_mut(cx);
                config.last_played_artwork = self.presentation.artwork.clone();
                if matches!(config.app_background, crate::background::AppBackground::Artwork(_)) {
                    config.app_background =
                        crate::background::AppBackground::Artwork(config.last_played_artwork.clone());
                }
            }
            let id = i.id;
            let name = i.name.clone();
            #[allow(unused_mut)]
            let mut state = i.status;
            #[cfg(feature = "ui-capture")]
            match std::env::var("WBOMBO_UI_CAPTURE_PAGE").as_deref() {
                Ok("home-running") => state = InstanceStatus::Running,
                Ok("home-optimizing") => state = InstanceStatus::Optimizing,
                _ => {},
            }
            match state {
                InstanceStatus::Launching => {
                    play = PlayButton::new("home-play", "Abriendo Minecraft", None).title("Iniciando").disabled(true)
                },
                InstanceStatus::Stopping => {
                    play = PlayButton::new("home-play", "Cerrando Minecraft", None)
                        .title("Deteniendo")
                        .disabled(true)
                },
                InstanceStatus::Optimizing => {
                    let progress = i.optimization.map(|p| p.0).unwrap_or(0.).clamp(0., 1.);
                    play = PlayButton::new(
                        "home-play",
                        format!("Creando la caché de arranque · {:.0} %", progress * 100.),
                        None,
                    )
                    .title("Iniciando")
                    .tint(0x176daf60)
                    .progress(progress);
                },
                InstanceStatus::Running => {
                    play = PlayButton::new("home-play", "En marcha", None)
                        .title("Detener")
                        .stop_icon()
                        .tint(0xc52222b0)
                        .on_click(move |_, _, _| data.backend_handle.send(MessageToBackend::KillInstance { id }))
                },
                InstanceStatus::NotRunning => {
                    play = play.disabled(root::inherited_operation_active(id)).on_click(move |_, w, c| {
                        if data.accounts.read(c).selected_account.is_none() {
                            root::open_home_accounts(c);
                        } else {
                            root::start_instance(id, name.clone(), None, &data, w, c);
                        }
                    })
                },
            }
        } else {
            play = play.add_instance().on_click(move |_, w, c| add_instance(&data, w, c));
        }
        if let Some((progress, count, total)) = instance.as_ref().and_then(|i| root::home_update_progress(i.id)) {
            let detail = if total > 0 {
                format!("{:.0} % · {} de {} MB", progress * 100., count / 1024 / 1024, total / 1024 / 1024)
            } else {
                String::new()
            };
            play = PlayButton::new("home-play", detail, None).updating(progress);
            window.request_animation_frame();
        }
        let left = if let Some(i) = &instance {
            let text = if i.playtime.last_played_unix_ms.is_none() && i.playtime.total_secs == 0 {
                "Todavía no has jugado".to_string()
            } else {
                let date = i
                    .playtime
                    .last_played_unix_ms
                    .and_then(chrono::DateTime::from_timestamp_millis)
                    .map(|d| relative_date(&d.to_rfc3339()));
                [self.presentation.last_destination.clone(), date]
                    .into_iter()
                    .flatten()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · ")
            };
            h_flex()
                .w(px(300.))
                .gap(px(12.))
                .child(img(picture(&self.presentation.last_world_icon, "images/home-world-default.png")).size(px(44.)))
                .child(
                    v_flex()
                        .gap(px(3.))
                        .child(design::eyebrow("ÚLTIMA PARTIDA"))
                        .child(div().text_size(px(12.)).child(text))
                        .child(
                            h_flex()
                                .gap(px(4.))
                                .child(badge(format!(
                                    "{} {}",
                                    i.configuration.loader.pretty_name(),
                                    i.configuration.minecraft_version
                                )))
                                .when_some(self.presentation.mod_count, |d, n| d.child(badge(format!("{n} mods"))))
                                .when_some(
                                    i.configuration
                                        .memory
                                        .as_ref()
                                        .filter(|m| m.enabled)
                                        .map(|m| m.max)
                                        .or(self.presentation.recommended_memory_mib),
                                    |d, memory| d.child(badge(format!("{} GB", memory as f32 / 1024.))),
                                ),
                        ),
                )
        } else {
            h_flex().w(px(300.))
        };
        let hours = instance.as_ref().map(|i| i.playtime.total_secs / 3600).unwrap_or(0);
        let menu = if self.menu {
            let mut menu = design::notched_panel(0x0a0a0cf2)
                .flex()
                .flex_col()
                .absolute()
                .bottom(px(60.))
                .right_0()
                .w(px(235.))
                .p(px(6.));
            for (ix, label) in [
                "Abrir carpeta de la instancia",
                "Jugar sin actualizar",
                "Ver registros",
                "Reparar archivos del juego",
                "Crear acceso directo",
                "Ajustes de la instancia",
                "Detener el juego",
            ]
            .into_iter()
            .enumerate()
            {
                menu = menu.child(
                    DesignButton::new(("home-menu", ix))
                        .label_left(label)
                        .h(px(30.))
                        .text_size(px(13.))
                        .font_weight(FontWeight::NORMAL)
                        .justify_start()
                        .w_full()
                        .disabled(if ix == 6 {
                            instance.as_ref().is_none_or(|i| i.status != InstanceStatus::Running)
                        } else if [1, 3].contains(&ix) {
                            instance.as_ref().is_none_or(|i| i.status != InstanceStatus::NotRunning)
                        } else {
                            false
                        })
                        .on_click(cx.listener(move |this, _, w, c| {
                            this.menu = false;
                            if let Some(i) = &this.instance {
                                let i = i.read(c);
                                let id = i.id;
                                match ix {
                                    0 => crate::open_folder(&i.dot_minecraft_folder.clone(), w, c),
                                    1 => {
                                        if this.data.accounts.read(c).selected_account.is_none() {
                                            root::open_home_accounts(c);
                                        } else {
                                            root::start_instance_without_update(id, i.name.clone(), &this.data, w, c);
                                        }
                                    },
                                    3 => root::repair_game_files(id, &this.data.backend_handle, w, c),
                                    4 => crate::modals::create_shortcut::open(
                                        i.clone(),
                                        this.data.backend_handle.clone(),
                                        w,
                                        c,
                                    ),
                                    2 => this.open_tab(InstanceSubpageType::Logs, w, c),
                                    5 => this.open_tab(InstanceSubpageType::Settings, w, c),
                                    6 => this.data.backend_handle.send(MessageToBackend::KillInstance { id }),
                                    _ => this.open_tab(InstanceSubpageType::Settings, w, c),
                                }
                            }
                            c.notify();
                        })),
                );
            }
            Some(menu.on_mouse_down_out(cx.listener(|this, _, _, c| {
                this.menu = false;
                c.notify();
            })))
        } else {
            None
        };
        let game_bar =
            h_flex()
                .h(px(84.))
                .w_full()
                .px(px(24.))
                .gap(px(16.))
                .items_center()
                .justify_between()
                .child(left)
                .child(h_flex().gap(px(16.)).child(play).when(instance.is_some(), |d| {
                    d.child(
                        div()
                            .relative()
                            .child(
                                DesignButton::new("home-more")
                                    .notched()
                                    .paint(
                                        0.,
                                        [[0xffffff0a; 2], [0xffffff14; 2], [0xffffff08; 2]],
                                        rgb(theme::INK_MUTED).into(),
                                        rgb(theme::INK).into(),
                                    )
                                    .label("⋯")
                                    .w(px(44.))
                                    .h(px(52.))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.menu = !this.menu;
                                        cx.notify();
                                    })),
                            )
                            .when_some(menu, |d, menu| d.child(menu)),
                    )
                }))
                .child(h_flex().w(px(300.)).justify_end().gap(px(14.)).when_some(instance.as_ref(), |d, i| {
                    d.child(h_flex().h(px(28.)).gap(px(3.)).items_end().children(
                        i.playtime.weekly_secs.into_iter().map(|seconds| {
                            let max = *i.playtime.weekly_secs.iter().max().unwrap_or(&1);
                            div()
                                .w(px(6.))
                                .h(px(if seconds == 0 {
                                    3.
                                } else {
                                    (seconds as f32 / max.max(1) as f32 * 28.).max(3.)
                                }))
                                .bg(if seconds == 0 {
                                    rgba(0xffffff14)
                                } else {
                                    rgba(0x5bcef599)
                                })
                        }),
                    ))
                    .child(
                        v_flex()
                            .items_end()
                            .font_family(theme::DISPLAY)
                            .child(div().text_size(px(18.)).child(format!("{hours} h")))
                            .when(i.playtime.last_session_secs > 0, |d| {
                                d.child(div().text_size(px(11.)).text_color(rgb(theme::INK_DISABLED)).child(format!(
                                    "{} h {} min",
                                    i.playtime.last_session_secs / 3600,
                                    (i.playtime.last_session_secs % 3600) / 60
                                )))
                            }),
                    )
                }));
        let general = instance.is_none() || InterfaceConfig::get(cx).home_general_news;
        let news = self
            .presentation
            .news
            .iter()
            .filter(|n| {
                if instance.is_none() {
                    n.target_profile_id.is_none()
                } else {
                    general || n.target_profile_id.is_some()
                }
            })
            .take(if self.news_view { usize::MAX } else { 4 })
            .cloned()
            .collect::<Vec<_>>();
        let show_news = instance.is_some() || !news.is_empty() || !self.news_loaded;
        let mut news_section = v_flex().px(px(24.)).pt(px(18.)).gap(px(12.)).child(
            h_flex()
                .h(px(24.))
                .justify_between()
                .child(
                    h_flex()
                        .gap(px(10.))
                        .child(design::eyebrow("NOTICIAS"))
                        .when_some(instance.as_ref(), |d, i| d.child(format!("de {}", i.name))),
                )
                .child(
                    h_flex()
                        .gap(px(18.))
                        .child(
                            design::chip("home-general", "General", general)
                                .h(px(24.))
                                .disabled(instance.is_none())
                                .on_click(|_, w, c| {
                                    let conf = InterfaceConfig::get_mut(c);
                                    conf.home_general_news = !conf.home_general_news;
                                    w.refresh();
                                }),
                        )
                        .child(DesignButton::new("home-all-news").label("Ver todas →").h(px(24.)).px_0().on_click(
                            cx.listener(|this, _, _, c| {
                                this.news_view = true;
                                this.news_page = None;
                                this.article = None;
                                c.notify();
                            }),
                        )),
                ),
        );
        if !self.news_loaded {
            news_section = news_section.child(
                div()
                    .w_full()
                    .py(px(24.))
                    .text_center()
                    .font_family(theme::DISPLAY)
                    .text_size(px(11.))
                    .text_color(rgb(theme::INK_MUTED))
                    .child("CARGANDO"),
            );
        } else if news.is_empty() {
            news_section = news_section.child(div().py(px(24.)).text_color(rgb(theme::INK_MUTED)).child(
                if self.presentation.news_failed {
                    "No se han podido cargar las noticias."
                } else if instance.is_none() {
                    "No hay noticias recientes."
                } else {
                    "No hay noticias de esta instancia."
                },
            ));
        }
        news_section = news_section.child(h_flex().flex_wrap().items_start().gap(px(12.)).children(
            news.into_iter().enumerate().map(|(ix, n)| {
                let image = n.image.as_ref().map(|p| {
                    img(ImageSource::Resource(Resource::Path(Arc::from(p.as_path()))))
                        .w(px(242.))
                        .h(px(137.))
                        .object_fit(ObjectFit::Cover)
                        .into_any_element()
                });
                let unread = InterfaceConfig::get(cx)
                    .news_marks
                    .get(&self.presentation.news_origin_key)
                    .is_none_or(|marks| !marks.read.contains(&n.id));
                let card = crate::component::news_card::NewsCard {
                    width: 244.,
                    image,
                    category: n.category.clone(),
                    category_emphasis: n.target_profile_id.is_some(),
                    title: n.title.clone(),
                    summary: self.news_previews.get(&n.id).cloned().unwrap_or_default(),
                    date: relative_date(&n.published_at),
                    pinned: n.pinned,
                    unread,
                }
                .render();
                DesignButton::new(("home-news", ix))
                    .aria_label(n.title.clone())
                    .w(px(244.))
                    .h_auto()
                    .p_0()
                    .paint(0., [[0, 0]; 3], rgb(theme::INK).into(), rgb(theme::INK).into())
                    .child(div().w_full().whitespace_normal().text_left().child(card))
                    .on_click(cx.listener(move |this, _, _, c| {
                        this.news_view = true;
                        this.article = Some(n.id.clone());
                        this.news_page = None;
                        c.notify();
                    }))
                    .into_any_element()
            }),
        ));
        let content = v_flex()
            .relative()
            .size_full()
            .min_w_0()
            .overflow_y_scrollbar()
            .child(hero)
            .child(game_bar)
            .when(show_news, |d| d.child(news_section));

        div()
            .size_full()
            .relative()
            .text_color(rgb(theme::INK))
            .child(content)
            .child(div().absolute().top_0().w_full().child(header))
            .into_any_element()
    }
}

fn relative_date(value: &str) -> String {
    let Ok(date) = chrono::DateTime::parse_from_rfc3339(value) else {
        return String::new();
    };
    let elapsed = chrono::Utc::now().signed_duration_since(date);
    let days = elapsed.num_days();
    if days >= 1 {
        if days == 1 {
            "Ayer".into()
        } else {
            format!("Hace {days} días")
        }
    } else {
        format!("Hace {} h", elapsed.num_hours().max(0))
    }
}

fn badge(text: impl Into<SharedString>) -> Div {
    div()
        .h(px(20.))
        .px(px(6.))
        .bg(rgba(0xffffff0a))
        .font_family(theme::DISPLAY)
        .text_size(px(10.5))
        .text_color(rgb(theme::INK_MUTED))
        .child(text.into())
}
