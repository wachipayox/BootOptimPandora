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
use gpui_component::{Disableable, h_flex, scroll::ScrollableElement, v_flex};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
pub struct HomePage {
    data: DataEntities,
    instance: Option<Entity<InstanceEntry>>,
    presentation: HomePresentation,
    loaded: bool,
    news_loaded: bool,
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
fn server_frame(content: impl IntoElement) -> Div {
    design::notched_panel(0x060607cc)
        .absolute()
        .top(px(60.))
        .right(px(18.))
        .w(px(212.))
        .child(
            canvas(
                |_, _, _| (),
                |bounds, _, window, _| {
                    let x = bounds.origin.x + px(0.5);
                    let y = bounds.origin.y + px(0.5);
                    let w = bounds.size.width - px(1.);
                    let h = bounds.size.height - px(1.);
                    let n = px(6.);
                    let mut path = PathBuilder::stroke(px(1.));
                    path.move_to(point(x + n, y));
                    path.line_to(point(x + w - n, y));
                    path.line_to(point(x + w, y + n));
                    path.line_to(point(x + w, y + h - n));
                    path.line_to(point(x + w - n, y + h));
                    path.line_to(point(x + n, y + h));
                    path.line_to(point(x, y + h - n));
                    path.line_to(point(x, y + n));
                    path.close();
                    if let Ok(path) = path.build() {
                        window.paint_path(path, rgba(0xffffff0b));
                    }
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
        .child(div().w_full().px(px(14.)).py(px(12.)).child(content))
}
pub fn add_instance(_data: &DataEntities, window: &mut Window, cx: &mut App) {
    root::switch_page(PageType::AddInstance, &[PageType::Instances], window, cx);
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
        let mut page = Self {
            data: data.clone(),
            instance,
            presentation: HomePresentation::default(),
            loaded: false,
            news_loaded: false,
            startup_cached: !std::env::var("WBOMBO_UI_CAPTURE_PAGE").is_ok_and(|page| page != "startup-live"),
            artwork_image: Default::default(),
            logo_image: Default::default(),
            requested: false,
            refreshed: Instant::now(),
            menu: std::env::var("WBOMBO_UI_CAPTURE_PAGE").is_ok_and(|p| p == "home-menu"),
            news_view: std::env::var("WBOMBO_UI_CAPTURE_PAGE")
                .is_ok_and(|p| p == "home-news" || p.starts_with("news-")),
            news_page: None,
            article: None,
            heads: Default::default(),
            head_ids: Vec::new(),
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
                this.presentation = result.ok().and_then(Result::ok).unwrap_or_else(|| HomePresentation {
                    news_failed: true,
                    ..Default::default()
                });
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
    fn open_tab(&self, tab: InstanceSubpageType, window: &mut Window, cx: &mut App) {
        if let Some(i) = &self.instance {
            let name = i.read(cx).name.clone();
            InterfaceConfig::get_mut(cx).instance_subpage = tab;
            root::switch_page(PageType::InstancePage { name }, &[PageType::Instances], window, cx);
        }
    }
    fn server_panel(&self, server: &InstanceServerSummary, cx: &mut Context<Self>) -> AnyElement {
        let online = server.status.as_ref().and_then(|s| s.players.as_ref());
        let ip = server.ip.clone();
        let data = self.data.clone();
        let i = self.instance.as_ref().unwrap().read(cx);
        let id = i.id;
        let name = i.name.clone();
        let mut panel = div().flex().flex_col().gap(px(6.)).child(
            div()
                .font_family(theme::SANS)
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(px(13.))
                .line_height(px(18.))
                .child("Jugadores conectados"),
        );
        panel = panel.child(
            h_flex()
                .justify_between()
                .text_size(px(12.))
                .line_height(px(20.))
                .text_color(rgb(theme::INK_MUTED))
                .child(
                    h_flex()
                        .gap(px(6.))
                        .child(div().size(px(6.)).flex_shrink_0().rounded_full().bg(rgb(if server.status.is_some() {
                            theme::SUCCESS
                        } else {
                            theme::INK_DISABLED
                        })))
                        .child(if server.pinging && server.status.is_none() {
                            "CARGANDO"
                        } else if server.status.is_some() {
                            "Servidor en línea"
                        } else {
                            "Servidor sin respuesta"
                        }),
                )
                .when_some(online, |d, p| {
                    d.child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(theme::INK))
                            .child(format!("{} / {}", p.online, p.max)),
                    )
                }),
        );
        if let Some(players) = online.filter(|p| !p.sample.is_empty()) {
            let shown = players.sample.len().min(15);
            let extra = players.online.saturating_sub(shown as i32);
            panel = panel.child(
                h_flex()
                    .flex_wrap()
                    .gap(px(3.))
                    .pt(px(2.))
                    .children(players.sample.iter().take(shown).map(|p| {
                        img(picture(
                            &if InterfaceConfig::get(cx).hide_skins {
                                None
                            } else {
                                self.heads.get(p.id.as_ref()).cloned()
                            },
                            "images/default_head.png",
                        ))
                        .size(px(20.))
                    }))
                    .when(extra > 0, |d| {
                        d.child(
                            div()
                                .size(px(20.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(rgba(0xffffff0a))
                                .text_size(px(10.))
                                .text_color(rgb(theme::INK_MUTED))
                                .child(format!("+{extra}")),
                        )
                    }),
            );
            panel = panel.child(div().text_size(px(11.)).text_color(rgb(theme::INK_MUTED)).child(
                if InterfaceConfig::get(cx).hide_usernames {
                    String::new()
                } else {
                    let names = players.sample.iter().take(3).map(|p| p.name.as_ref()).collect::<Vec<_>>().join(", ");
                    let remaining = players.online.saturating_sub(players.sample.len().min(3) as i32);
                    if remaining > 0 {
                        format!("{names} y {remaining} más")
                    } else {
                        names
                    }
                },
            ));
        }
        let panel = panel.child(
            DesignButton::new("quick-server")
                .notched()
                .aria_label("Entrar al servidor")
                .child(
                    h_flex()
                        .w_full()
                        .gap(px(8.))
                        .child(div().text_size(px(12.)).child("▶"))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(12.))
                                .whitespace_nowrap()
                                .child("Entrar al servidor"),
                        )
                        .when_some(server.ping, |d, p| {
                            d.child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgba(0x86d96f99))
                                    .child(format!("{} ms", p.as_millis())),
                            )
                        }),
                )
                .paint(
                    90.,
                    [[0x86d96f1f; 2], [0x86d96f2e; 2], [0x86d96f1f; 2]],
                    rgb(0x86d96f).into(),
                    rgb(0x86d96f).into(),
                )
                .text_size(px(12.))
                .h(px(32.))
                .w_full()
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
                }),
        );
        server_frame(panel).into_any_element()
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
            resource(&self.presentation.artwork, "images/artwork-vanilla.png"),
            "images/artwork-vanilla.png",
            true,
            cx.entity().entity_id(),
            window,
            cx,
        );
        let logo = self.logo_image.render(
            resource(&self.presentation.logo, "images/logo-vanilla.png"),
            "images/logo-vanilla.png",
            false,
            cx.entity().entity_id(),
            window,
            cx,
        );
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
            .child(div().absolute().top_0().w_full().child(header))
            .when_some(server.as_ref(), |d, s| d.child(self.server_panel(s, cx)))
            .when(server_loading, |d| {
                d.child(server_frame(
                    v_flex()
                        .gap(px(6.))
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_size(px(13.))
                                .line_height(px(18.))
                                .child("Jugadores conectados"),
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
            if i.status == InstanceStatus::Running
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
                        .tint(0xb330305e)
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
        let general = InterfaceConfig::get(cx).home_general_news;
        let news = self
            .presentation
            .news
            .iter()
            .filter(|n| general || n.target_profile_id.is_some())
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
                        .child(design::chip("home-general", "General", general).h(px(24.)).on_click(|_, w, c| {
                            let conf = InterfaceConfig::get_mut(c);
                            conf.home_general_news = !conf.home_general_news;
                            w.refresh();
                        }))
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
                } else {
                    "No hay noticias de esta instancia."
                },
            ));
        }
        news_section = news_section.child(h_flex().items_start().gap(px(12.)).children(
            news.into_iter().enumerate().map(|(ix, n)| {
                let image = n.image.as_ref().map(|p| {
                    img(ImageSource::Resource(Resource::Path(Arc::from(p.as_path()))))
                        .w_full()
                        .h(px(137.))
                        .object_fit(ObjectFit::Cover)
                        .into_any_element()
                });
                design::notched_panel(0x111114ff)
                    .flex()
                    .flex_col()
                    .id(("home-news", ix))
                    .w(px(244.))
                    .flex_shrink_0()
                    .min_w_0()
                    .relative()
                    .when_some(image, |d, image| d.child(image))
                    .when(n.pinned, |d| {
                        d.child(
                            div()
                                .when(n.image.is_some(), |d| d.absolute().top_0())
                                .w_full()
                                .h(px(15.))
                                .bg(rgba(0x80808080))
                                .text_center()
                                .font_family(theme::DISPLAY)
                                .text_size(px(9.))
                                .child("FIJADA"),
                        )
                    })
                    .child(
                        v_flex()
                            .p(px(11.))
                            .gap(px(3.))
                            .child(
                                div().text_size(px(11.)).text_color(rgb(theme::ACCENT_TEXT)).child(n.category.clone()),
                            )
                            .child(div().text_size(px(13.)).line_height(px(18.)).line_clamp(2).child(n.title.clone()))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(theme::INK_DISABLED))
                                    .child(relative_date(&n.published_at)),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, _, c| {
                        this.news_view = true;
                        this.article = Some(n.id.clone());
                        this.news_page = None;
                        c.notify();
                    }))
                    .into_any_element()
            }),
        ));
        let mut content = v_flex()
            .relative()
            .size_full()
            .min_w_0()
            .overflow_y_scrollbar()
            .child(hero)
            .child(game_bar)
            .when(show_news, |d| d.child(news_section));
        if self.menu {
            let mut menu = design::notched_panel(0x0a0a0cf2)
                .flex()
                .flex_col()
                .absolute()
                .top(px(235.))
                .left(relative(0.42))
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
                                    4 => create_shortcut(id, i.name.clone(), this.data.backend_handle.clone(), c),
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
            content = content.child(menu.on_mouse_down_out(cx.listener(|this, _, _, c| {
                this.menu = false;
                c.notify();
            })));
        }
        div().size_full().relative().text_color(rgb(theme::INK)).child(content).into_any_element()
    }
}

fn create_shortcut(
    id: bridge::instance::InstanceID,
    name: SharedString,
    backend: bridge::handle::BackendHandle,
    cx: &mut App,
) {
    let user_dirs = directories::UserDirs::new();
    let directory = user_dirs
        .as_ref()
        .and_then(directories::UserDirs::desktop_dir)
        .unwrap_or(std::path::Path::new("."));
    #[cfg(target_os = "windows")]
    let suggested = format!("{name}.lnk");
    #[cfg(target_os = "linux")]
    let suggested = format!("{name}.desktop");
    #[cfg(target_os = "macos")]
    let suggested = format!("{name}.app");
    let receiver = cx.prompt_for_new_path(directory, Some(&suggested));
    cx.spawn(async move |_| {
        if let Ok(Ok(Some(path))) = receiver.await {
            backend.send(MessageToBackend::CreateInstanceShortcut { id, path });
        }
    })
    .detach();
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
