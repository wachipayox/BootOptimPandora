//! Server-filtered news and a native, restricted Markdown reader.
use crate::{
    component::design::{self, DesignButton},
    entity::DataEntities,
    interface_config::InterfaceConfig,
    theme,
};
use bridge::{home::HomeNews, message::MessageToBackend};
use gpui::{prelude::*, *};
use gpui_component::{
    Disableable, WindowExt,
    checkbox::Checkbox,
    h_flex,
    input::{InputEvent, InputState},
    menu::{DropdownMenu, PopupMenuItem},
    scroll::ScrollableElement,
    text::{MarkdownExtensions, MarkdownNode, TextView, TextViewStyle},
    v_flex,
};
use schema::{
    news::*,
    news_markdown::{self, Block, Inline},
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

actions!(news, [FocusSearch, LeaveArticle]);
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-k", FocusSearch, Some("News")),
        KeyBinding::new("escape", LeaveArticle, Some("News")),
    ]);
}

#[derive(Clone, Copy, PartialEq)]
enum NewsTab {
    Recent,
    History,
    Saved,
}
struct Article {
    item: NewsItem,
    valid: bool,
    extensions: MarkdownExtensions,
}
pub struct NewsPage {
    data: DataEntities,
    focus: FocusHandle,
    feed: NewsFeed,
    loaded: bool,
    loading: bool,
    tab: NewsTab,
    category: Option<String>,
    profiles: Option<BTreeSet<String>>,
    search: Entity<InputState>,
    oldest: bool,
    visible: usize,
    pinned: usize,
    article: Option<Article>,
    seed: Option<HomeNews>,
    media: Arc<Mutex<BTreeMap<String, PathBuf>>>,
    media_ratios: Arc<Mutex<BTreeMap<String, f32>>>,
    requested: BTreeSet<String>,
    queue: VecDeque<NewsImageRequest>,
    media_loading: bool,
    previews: BTreeMap<String, String>,
    _timer: Task<()>,
}
fn category_chip(id: impl Into<ElementId>, label: impl Into<SharedString>, selected: bool) -> DesignButton {
    DesignButton::new(id)
        .label(label)
        .h(px(28.))
        .px(px(10.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(if selected { theme::INK } else { theme::INK_MUTED }))
        .when(selected, |d| d.border_b_2().border_color(rgb(theme::ACCENT_TEXT)))
}
fn preview(body: &str) -> String {
    fn text(nodes: &[Inline]) -> String {
        nodes
            .iter()
            .map(|n| match n {
                Inline::Text(s) => s.clone(),
                Inline::Emphasis(c) | Inline::Strong(c) | Inline::Link { content: c, .. } => text(c),
                Inline::Image { alt, .. } => alt.clone(),
            })
            .collect()
    }
    news_markdown::parse(body, None)
        .ok()
        .and_then(|d| {
            d.blocks.into_iter().find_map(|b| {
                if let Block::Paragraph(c) = b {
                    Some(text(&c))
                } else {
                    None
                }
            })
        })
        .unwrap_or_default()
}
fn new_tag() -> Div {
    h_flex()
        .gap(px(5.))
        .text_size(px(11.))
        .text_color(rgb(theme::ACCENT_TEXT))
        .child(div().size(px(6.)).bg(rgb(theme::ACCENT_TEXT)))
        .child("Nueva")
}
fn empty(text: &'static str) -> Div {
    div().w_full().py(px(48.)).text_center().text_color(rgb(theme::INK_MUTED)).child(text)
}
fn window_controls(window: &Window, inset: f32) -> Div {
    use crate::component::generic_title_bar::WindowControl;
    let controls = h_flex().absolute().top(px(8.)).right(px(inset)).flex_shrink_0();
    if crate::root::should_render_custom_titlebar() && !cfg!(target_os = "macos") {
        controls
            .child(WindowControl::Minimize)
            .child(if window.is_maximized() {
                WindowControl::Restore
            } else {
                WindowControl::Maximize
            })
            .child(WindowControl::Close)
    } else {
        controls
    }
}
fn cover_key(n: &NewsItem, kind: &str) -> String {
    format!("cover:{}:{}:{kind}", n.id, n.updated_at)
}
fn picture(path: Option<PathBuf>, width: f32, height: f32) -> AnyElement {
    match path {
        Some(p) => img(ImageSource::Resource(Resource::Path(Arc::from(p.as_path()))))
            .w(px(width))
            .flex_shrink_0()
            .h(px(height))
            .object_fit(ObjectFit::Cover)
            .into_any_element(),
        None => div()
            .w(px(width))
            .flex_shrink_0()
            .h(px(height))
            .bg(rgb(theme::PANEL_RAISED))
            .into_any_element(),
    }
}
fn date(value: &str, now: &str) -> String {
    let (Ok(date), Ok(now)) = (chrono::DateTime::parse_from_rfc3339(value), chrono::DateTime::parse_from_rfc3339(now))
    else {
        return String::new();
    };
    let elapsed = now.signed_duration_since(date);
    match elapsed.num_days() {
        0 => format!("Hace {} h", elapsed.num_hours().max(0)),
        1 => "Ayer".into(),
        n => format!("Hace {n} días"),
    }
}
fn collect_media(blocks: &[Block], out: &mut BTreeSet<String>) {
    fn inline(nodes: &[Inline], out: &mut BTreeSet<String>) {
        for node in nodes {
            match node {
                Inline::Image { src, .. } => {
                    out.insert(src.clone());
                },
                Inline::Emphasis(c) | Inline::Strong(c) | Inline::Link { content: c, .. } => inline(c, out),
                _ => {},
            }
        }
    }
    for node in blocks {
        match node {
            Block::Paragraph(c) | Block::Heading { content: c, .. } => inline(c, out),
            Block::Image { src, .. } => {
                out.insert(src.clone());
            },
            Block::Youtube { id } => {
                out.insert(format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg"));
            },
            Block::List { items, .. } => {
                for item in items {
                    collect_media(item, out);
                }
            },
            Block::Quote(c) => collect_media(c, out),
            _ => {},
        }
    }
}
impl NewsPage {
    pub fn new(
        data: &DataEntities,
        profile: Option<String>,
        seed: Option<HomeNews>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Buscar noticias"));
        cx.subscribe_in(&search, window, |this, _, event, window, cx| {
            if matches!(event, InputEvent::Change) {
                this.visible = 6;
                this.pinned = 0;
                this.prepare_media(window, cx);
                cx.notify();
            }
        })
        .detach();
        let timer = cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(300)).await;
                if this.update_in(cx, |this, window, cx| this.refresh(window, cx)).is_err() {
                    break;
                }
            }
        });
        let mut page = Self {
            data: data.clone(),
            focus: cx.focus_handle(),
            feed: NewsFeed::default(),
            loaded: false,
            loading: false,
            tab: NewsTab::Recent,
            category: None,
            profiles: profile.map(|p| BTreeSet::from([p])),
            search,
            oldest: false,
            visible: 6,
            pinned: 0,
            article: None,
            seed,
            media: Default::default(),
            media_ratios: Default::default(),
            requested: Default::default(),
            queue: Default::default(),
            media_loading: false,
            previews: Default::default(),
            _timer: timer,
        };
        page.focus.focus(window, cx);
        page.refresh(window, cx);
        page
    }
    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.loading = true;
        let (send, receive) = tokio::sync::oneshot::channel();
        self.data.backend_handle.send(MessageToBackend::GetNews { channel: send });
        cx.spawn_in(window, async move |this, cx| {
            let result = receive.await.ok().and_then(Result::ok);
            let _ = this.update_in(cx, |this, window, cx| {
                if let Some(feed) = result {
                    let config = InterfaceConfig::get_mut(cx);
                    let legacy = if config.news_marks.is_empty() {
                        std::mem::take(&mut config.home_read_news)
                    } else {
                        BTreeSet::new()
                    };
                    let state = config.news_marks.entry(feed.origin_key.clone()).or_default();
                    state.read.extend(legacy);
                    state.sync(&feed);
                    this.feed = feed;
                    if this.category.as_ref().is_some_and(|id| !this.feed.categories.iter().any(|c| &c.id == id)) {
                        this.category = None;
                    }

                    if let Some(article) = this.article.as_ref() {
                        let id = article.item.id.clone();
                        if let Some(item) = this.feed.items.iter().find(|n| n.id == id).cloned() {
                            this.set_article(item, window, cx);
                        } else if let Some(article) = &mut this.article {
                            article.valid = false;
                        }
                    }
                } else {
                    this.feed.load_failed = true;
                }
                this.loading = false;
                this.loaded = true;
                if let Some(seed) = this.seed.take() {
                    let item = this.feed.items.iter().find(|n| n.id == seed.id).cloned();
                    if let Some(item) = item {
                        this.set_article(item, window, cx);
                    } else {
                        this.article = Some(Article {
                            item: NewsItem {
                                id: seed.id,
                                title: seed.title,
                                published_at: seed.published_at,
                                author: seed.author,
                                ..Default::default()
                            },
                            valid: false,
                            extensions: MarkdownExtensions::default().commonmark(),
                        });
                    }
                }
                if let Ok(route) = std::env::var("WBOMBO_UI_CAPTURE_PAGE") {
                    match route.as_str() {
                        "news-history" => this.tab = NewsTab::History,
                        "news-saved" => this.tab = NewsTab::Saved,
                        "news-search" => this.search.update(cx, |s, c| s.set_value("dragones de hielo", window, c)),
                        "news-profiles" => this.profile_filter(window, cx),
                        _ => {},
                    }
                    if route.starts_with("news-article") || route == "news-changelog" {
                        let wanted = route.split_once(':').map(|(_, id)| id);
                        if let Some(item) = this.feed.items.iter().find(|n| wanted.is_none_or(|id| id == n.id)).cloned()
                        {
                            if route == "news-changelog" {
                                if let Some(link) = item.linked_revision.clone() {
                                    super::news_changelog::open(
                                        &this.data,
                                        link.profile_id,
                                        link.revision_id,
                                        window,
                                        cx,
                                    );
                                }
                            }
                            this.set_article(item, window, cx);
                        }
                    }
                }
                this.prepare_media(window, cx);
                window.refresh();
                cx.notify();
                log::info!("news-ready pid={}", std::process::id());
            });
        })
        .detach();
    }
    fn marks<'a>(&self, cx: &'a App) -> Option<&'a NewsLocalState> {
        InterfaceConfig::get(cx).news_marks.get(&self.feed.origin_key)
    }
    fn filtered(&self, cx: &App, category: bool) -> Vec<&NewsItem> {
        let query = self.search.read(cx).value().to_lowercase();
        let mut items = self
            .feed
            .items
            .iter()
            .filter(|n| self.feed.matches_profiles(n, self.profiles.as_ref()))
            .filter(|n| match self.tab {
                NewsTab::Recent => !n.expired(&self.feed.server_time),
                NewsTab::History => n.expired(&self.feed.server_time),
                NewsTab::Saved => self.marks(cx).is_some_and(|s| s.saved.contains(&n.id)),
            })
            .filter(|n| !category || self.category.as_ref().is_none_or(|id| id == &n.category_id))
            .filter(|n| {
                query.is_empty() || n.title.to_lowercase().contains(&query) || n.body.to_lowercase().contains(&query)
            })
            .collect::<Vec<_>>();
        items.sort_by_cached_key(|item| {
            (chrono::DateTime::parse_from_rfc3339(&item.published_at).ok(), item.id.clone())
        });
        if !self.oldest {
            items.reverse();
        }
        items
    }
    fn request(&mut self, request: NewsImageRequest) {
        if self.requested.insert(request.key.clone()) {
            self.queue.push_back(request);
        }
    }
    fn prepare_media(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut items = self.filtered(cx, true).into_iter().cloned().collect::<Vec<_>>();
        let pinned = items
            .iter()
            .filter(|n| n.pinned && !n.expired(&self.feed.server_time))
            .cloned()
            .collect::<Vec<_>>();
        items.retain(|n| !n.pinned || n.expired(&self.feed.server_time));
        self.pinned = self.pinned.min(pinned.len().saturating_sub(1));
        for n in pinned.iter().skip(self.pinned).take(1) {
            self.cover(n, "pinned", 1984, 330, ImageCorners::All);
        }
        for (ix, n) in items.iter().take(self.visible + 1).enumerate() {
            if ix == 0 {
                self.previews.entry(cover_key(n, "preview")).or_insert_with(|| preview(&n.body));
                self.cover(n, "featured", 1102, 622, ImageCorners::All);
            } else {
                self.cover(n, "grid", 640, 360, ImageCorners::Top);
            }
        }
        if let Some(article) = &self.article {
            let item = article.item.clone();
            let valid = article.valid;
            self.cover(&item, "article", 1984, 720, ImageCorners::All);
            if valid {
                if let Ok(doc) = news_markdown::parse(&item.body, None) {
                    let mut urls = BTreeSet::new();
                    collect_media(&doc.blocks, &mut urls);
                    for uri in urls {
                        let source = if let Some(digest) = uri.strip_prefix("cas:") {
                            NewsImageSource::CasDigest(digest.to_owned())
                        } else {
                            NewsImageSource::Reference(NewsImage::External { url: uri.clone() })
                        };
                        self.request(NewsImageRequest {
                            key: uri,
                            source,
                            width: 1020,
                            height: 2048,
                            cover: false,
                            corners: ImageCorners::None,
                        });
                    }
                }
            }
        }
        self.load_media(window, cx);
    }
    fn cover(&mut self, item: &NewsItem, kind: &str, width: u32, height: u32, corners: ImageCorners) {
        if let Some(image) = &item.image {
            self.request(NewsImageRequest {
                key: cover_key(item, kind),
                source: NewsImageSource::Reference(image.clone()),
                width,
                height,
                cover: true,
                corners,
            });
        }
    }
    fn load_media(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.media_loading {
            return;
        }
        if self.queue.is_empty() {
            log::info!("news-media-ready pid={}", std::process::id());
            return;
        }
        self.media_loading = true;
        let requests = self.queue.drain(..self.queue.len().min(16)).collect();
        let (send, receive) = tokio::sync::oneshot::channel();
        self.data.backend_handle.send(MessageToBackend::GetNewsImages {
            requests,
            channel: send,
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = receive.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if let Ok(images) = result {
                    if let Ok(mut map) = this.media.lock() {
                        for image in images {
                            if let Some(path) = image.path {
                                if image.width > 0 && image.height > 0 {
                                    if let Ok(mut ratios) = this.media_ratios.lock() {
                                        ratios.insert(image.key.clone(), image.width as f32 / image.height as f32);
                                    }
                                }
                                map.insert(image.key, path);
                            }
                        }
                    }
                }
                this.media_loading = false;
                this.load_media(window, cx);
                cx.notify();
            });
        })
        .detach();
    }
    fn image(&self, n: &NewsItem, kind: &str, w: f32, h: f32) -> AnyElement {
        picture(self.media.lock().ok().and_then(|m| m.get(&cover_key(n, kind)).cloned()), w, h)
    }
    fn category_name(&self, id: &str) -> String {
        self.feed
            .categories
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.name.clone())
            .unwrap_or_default()
    }
    fn set_article(&mut self, item: NewsItem, window: &mut Window, cx: &mut Context<Self>) {
        let valid = news_markdown::parse(&item.body, None).is_ok();
        InterfaceConfig::get_mut(cx)
            .news_marks
            .entry(self.feed.origin_key.clone())
            .or_default()
            .read
            .insert(item.id.clone());
        let media = self.media.clone();
        let body_media = self.media.clone();
        let body_ratios = self.media_ratios.clone();
        let extensions = MarkdownExtensions::default()
            .commonmark()
            .block_parser(|node, context| {
                if !matches!(node, gpui_component::text::markdown_ast::Node::Paragraph(_)) {
                    return None;
                }
                let doc = news_markdown::parse(context.node_source(node)?, None).ok()?;
                if let [Block::Youtube { id }] = doc.blocks.as_slice() {
                    Some(
                        MarkdownNode::new("news-youtube", id.clone())
                            .text(format!("https://www.youtube.com/watch?v={id}"))
                            .markdown(context.node_source(node)?.to_owned()),
                    )
                } else if let [Block::Image { alt, src, size, align }] = doc.blocks.as_slice() {
                    Some(MarkdownNode::new(
                        "news-body-image",
                        (alt.clone(), src.clone(), size.clone(), align.clone()),
                    ))
                } else {
                    None
                }
            })
            .block_renderer("news-body-image", move |node, _, _| {
                let (alt, src, size, align) =
                    node.data::<(String, String, String, String)>().cloned().unwrap_or_default();
                let path = body_media.lock().ok().and_then(|m| m.get(&src).cloned());
                let ratio = body_ratios.lock().ok().and_then(|m| m.get(&src).copied());
                div().w_full().flex().when(align == "center", |d| d.justify_center()).child(
                    div()
                        .w_full()
                        .when(size != "full", |d| {
                            d.max_w(px(match size.as_str() {
                                "small" => 180.,
                                "large" => 480.,
                                _ => 300.,
                            }))
                        })
                        .when_some(path.clone(), |d, path| {
                            d.relative()
                                .when_some(ratio, |d, ratio| d.aspect_ratio(ratio))
                                .child(img(path).absolute().size_full().object_fit(ObjectFit::Contain))
                        })
                        .when(path.is_none(), |d| d.child(alt)),
                )
            })
            .block_renderer("news-youtube", move |node, _, _| {
                let id = node.data::<String>().cloned().unwrap_or_default();
                let url = format!("https://www.youtube.com/watch?v={id}");
                let thumbnail = format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg");
                let path = media.lock().ok().and_then(|m| m.get(&thumbnail).cloned());
                div()
                    .id(SharedString::from(format!("youtube-{id}")))
                    .w_full()
                    .h(px(383.))
                    .relative()
                    .bg(rgb(theme::PANEL_SUNKEN))
                    .child(picture(path, 680., 383.))
                    .child(
                        DesignButton::new(SharedString::from(format!("youtube-play-{id}")))
                            .aria_label(url.clone())
                            .child(gpui_component::Icon::new(crate::icon::PandoraIcon::Play).size(px(22.)))
                            .absolute()
                            .left(px(312.))
                            .top(px(167.5))
                            .w(px(56.))
                            .h(px(48.))
                            .bg(rgba(0x000000cc))
                            .on_click(move |_, _, cx| cx.open_url(&url)),
                    )
            });
        self.article = Some(Article {
            item,
            valid,
            extensions,
        });
        self.prepare_media(window, cx);
        window.refresh();
        cx.notify();
    }
    fn profile_filter(&self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = Arc::new(Mutex::new(
            self.profiles
                .clone()
                .unwrap_or_else(|| self.feed.profiles.iter().map(|p| p.profile_id.clone()).collect()),
        ));
        let mut profiles = self.feed.profiles.clone();
        let lineage = NewsFeed {
            profiles: profiles.clone(),
            ..Default::default()
        };
        profiles.sort_by_key(|p| {
            let mut chain = Vec::new();
            let mut id = Some(p.profile_id.as_str());
            let mut seen = BTreeSet::new();
            while let Some(current) = id {
                if !seen.insert(current) || seen.len() > 9 {
                    break;
                }
                let Some(profile) = lineage.profiles.iter().find(|p| p.profile_id == current) else {
                    break;
                };
                chain.push(profile.name.to_lowercase());
                id = profile.parent_profile_id.as_deref();
            }
            chain.reverse();
            chain.join("/")
        });
        let page = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let mut content = v_flex().gap(px(12.)).child(
                div()
                    .text_color(rgb(theme::INK_MUTED))
                    .text_size(px(13.))
                    .child("Noticias de los perfiles marcados. Las generales salen siempre."),
            );
            let explicit = selected.lock().map(|s| s.clone()).unwrap_or_default();
            let effective: BTreeSet<String> = explicit.iter().flat_map(|id| lineage.lineage(id)).collect();
            for profile in &profiles {
                let id = profile.profile_id.clone();
                let selection = selected.clone();
                let checked = effective.contains(&id);
                let inherited = checked && !explicit.contains(&id);
                let depth = lineage.lineage(&id).len().saturating_sub(1).min(8);
                content = content.child(
                    div().pl(px(depth as f32 * 12.)).bg(rgba(0xffffff03)).child(
                        Checkbox::new(SharedString::from(format!("profile-{id}")))
                            .label(profile.name.clone())
                            .checked(checked)
                            .disabled(inherited)
                            .w_full()
                            .h(px(34.))
                            .px(px(10.))
                            .text_size(px(13.))
                            .on_click(move |checked, w, _| {
                                if let Ok(mut s) = selection.lock() {
                                    if *checked {
                                        s.insert(id.clone());
                                    } else {
                                        s.remove(&id);
                                    }
                                }
                                w.refresh();
                            }),
                    ),
                );
            }
            let clear = page.clone();
            let apply = page.clone();
            let selection = selected.clone();
            design::dialog(dialog).title("Filtrar por perfil").child(content).child(
                h_flex()
                    .justify_between()
                    .mt(px(20.))
                    .child(DesignButton::new("clear-news-profiles").label("Quitar filtro").on_click(move |_, w, c| {
                        let _ = clear.update(c, |p, c| {
                            p.profiles = None;
                            p.visible = 6;
                            p.pinned = 0;
                            p.prepare_media(w, c);
                            c.notify();
                        });
                        w.close_dialog(c);
                    }))
                    .child(DesignButton::new("apply-news-profiles").action().label("Aplicar").on_click(
                        move |_, w, c| {
                            let _ = apply.update(c, |p, c| {
                                p.profiles = selection.lock().ok().map(|s| s.clone());
                                p.visible = 6;
                                p.pinned = 0;
                                p.prepare_media(w, c);
                                c.notify();
                            });
                            w.close_dialog(c);
                        },
                    )),
            )
        });
    }
    fn story(
        &self,
        id: impl Into<ElementId>,
        item: &NewsItem,
        width: f32,
        content: impl IntoElement,
        cx: &Context<Self>,
    ) -> DesignButton {
        let item = item.clone();
        DesignButton::new(id)
            .aria_label(item.title.clone())
            .w(px(width))
            .h_auto()
            .p_0()
            .child(div().w_full().whitespace_normal().text_left().text_color(rgb(theme::INK)).child(content))
            .on_click(cx.listener(move |this, _, w, c| this.set_article(item.clone(), w, c)))
    }
    fn card(&self, n: &NewsItem, ix: usize, cx: &Context<Self>) -> AnyElement {
        let unread = self.marks(cx).is_none_or(|s| !s.read.contains(&n.id));
        let card = design::notched_panel(0x0f0f12ff)
            .id(("news-card", ix))
            .w(px(320.))
            .flex_shrink_0()
            .cursor_pointer()
            .when(n.image.is_some(), |d| d.child(self.image(n, "grid", 320., 180.)))
            .child(
                v_flex()
                    .px(px(12.))
                    .pt(px(10.))
                    .pb(px(12.))
                    .gap(px(6.))
                    .child(
                        h_flex()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(theme::INK_MUTED))
                                    .bg(rgba(0xffffff08))
                                    .px(px(6.))
                                    .py(px(2.))
                                    .child(self.category_name(&n.category_id)),
                            )
                            .when(unread, |d| d.child(new_tag())),
                    )
                    .child(
                        div()
                            .font_family(theme::DISPLAY)
                            .text_size(px(15.))
                            .line_height(px(20.))
                            .line_clamp(2)
                            .child(n.title.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(theme::INK_MUTED))
                            .child(date(&n.published_at, &self.feed.server_time)),
                    ),
            )
            .into_any_element();
        self.story(("news-card-open", ix), n, 320., card, cx).into_any_element()
    }
    fn article_view(&self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let article = self.article.as_ref().unwrap();
        let item = &article.item;
        let saved = self.marks(cx).is_some_and(|s| s.saved.contains(&item.id));
        let id = item.id.clone();
        let mut reader = v_flex()
            .w(px(680.))
            .mx_auto()
            .gap(px(16.))
            .child(
                DesignButton::new("article-back")
                    .label_left("← Noticias")
                    .w(px(110.))
                    .text_color(rgb(theme::ACCENT_TEXT))
                    .justify_start()
                    .px_0()
                    .on_click(cx.listener(|this, _, w, c| {
                        this.article = None;
                        this.focus.focus(w, c);
                        c.notify();
                    })),
            )
            .child(
                h_flex()
                    .gap(px(10.))
                    .text_size(px(12.))
                    .child(div().text_color(rgb(theme::ACCENT_TEXT)).child(self.category_name(&item.category_id)))
                    .child(div().text_color(rgb(theme::INK_MUTED)).child(format!(
                        "{} · por {} · {} min de lectura",
                        date(&item.published_at, &self.feed.server_time),
                        item.author,
                        (item.body.split_whitespace().count().div_ceil(200)).max(1)
                    ))),
            )
            .child(
                div()
                    .font_family(theme::DISPLAY)
                    .text_size(px(34.))
                    .line_height(px(36.))
                    .child(item.title.clone()),
            );
        if article.valid {
            let sources = self.media.lock().map(|m| Arc::new(m.clone())).unwrap_or_default();
            let style = TextViewStyle {
                image_sources: Some(sources),
                heading_font_family: Some(theme::DISPLAY.into()),
                heading_font_size: Some(Arc::new(|level, _| px(if level == 2 { 20. } else { 15. }))),
                ..Default::default()
            };
            reader = reader.child(
                div().w_full().text_size(px(15.)).line_height(px(24.)).child(
                    TextView::markdown(
                        SharedString::from(format!("article-{}-{}", item.id, item.updated_at)),
                        item.body.clone(),
                    )
                    .style(style)
                    .markdown_extensions(article.extensions.clone())
                    .on_link_click(|url, _, _, cx| {
                        if news_markdown::https_url(url) {
                            cx.open_url(url);
                        }
                    }),
                ),
            );
        } else if self.loaded {
            reader = reader.child(empty("No se ha podido abrir la noticia."));
        }
        if let Some(link) = &item.linked_revision {
            let link = link.clone();
            let data = self.data.clone();
            reader = reader.child(
                DesignButton::new("news-changelog")
                    .h(px(40.))
                    .w_full()
                    .bg(rgba(0xffffff05))
                    .border_l_2()
                    .border_color(rgb(0xc9a2ff))
                    .child(
                        h_flex()
                            .w_full()
                            .justify_between()
                            .child(
                                h_flex()
                                    .gap(px(10.))
                                    .child(
                                        gpui_component::Icon::new(crate::icon::PandoraIcon::NewsAttachment)
                                            .size(px(14.))
                                            .text_color(rgb(0xc9a2ff)),
                                    )
                                    .child(div().text_color(rgb(theme::INK_MUTED)).child("Changelog adjunto")),
                            )
                            .child(div().text_color(rgb(theme::ACCENT_TEXT)).child("Ver ›")),
                    )
                    .on_click(move |_, w, c| {
                        super::news_changelog::open(&data, link.profile_id.clone(), link.revision_id.clone(), w, c)
                    }),
            );
        }
        reader = reader.child(
            DesignButton::new("save-news")
                .label("Guardar")
                .w(px(110.))
                .disabled(!self.feed.items.iter().any(|n| n.id == item.id))
                .when(saved, |d| d.bg(rgba(0x5bcef51f)).text_color(rgb(theme::ACCENT_TEXT)))
                .on_click(cx.listener(move |this, _, w, c| {
                    InterfaceConfig::get_mut(c)
                        .news_marks
                        .entry(this.feed.origin_key.clone())
                        .or_default()
                        .toggle_saved(&id);
                    w.refresh();
                    c.notify();
                })),
        );
        v_flex()
            .w_full()
            .gap(px(24.))
            .when(item.image.is_some(), |d| d.child(self.image(item, "article", 992., 360.)))
            .child(reader)
            .into_any_element()
    }
}
impl NewsPage {
    fn focus_search(&mut self, _: &FocusSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.article = None;
        self.search.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }
    fn leave_article(&mut self, _: &LeaveArticle, window: &mut Window, cx: &mut Context<Self>) {
        if self.article.take().is_some() {
            self.focus.focus(window, cx);
            cx.notify();
        } else if !self.search.read(cx).value().is_empty() {
            self.search.update(cx, |input, cx| input.set_value("", window, cx));
        } else {
            cx.propagate();
        }
    }
}
impl Render for NewsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.article.is_some() {
            return v_flex()
                .relative()
                .min_w_0()
                .key_context("News")
                .track_focus(&self.focus)
                .on_action(cx.listener(Self::focus_search))
                .on_action(cx.listener(Self::leave_article))
                .size_full()
                .text_color(rgb(theme::INK))
                .child(div().w_full().h(px(24.)).flex_shrink_0().window_control_area(WindowControlArea::Drag))
                .child(
                    v_flex()
                        .w_full()
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scrollbar()
                        .px(px(24.))
                        .pb(px(24.))
                        .child(self.article_view(window, cx)),
                )
                .child(window_controls(window, 24.))
                .into_any_element();
        }
        let items = self.filtered(cx, true);
        let counts = self.filtered(cx, false);
        let query = self.search.read(cx).value();
        let mut categories = h_flex().gap(px(6.)).child(
            category_chip("category-all", format!("Todas  {}", counts.len()), self.category.is_none()).on_click(
                cx.listener(|this, _, w, c| {
                    this.category = None;
                    this.visible = 6;
                    this.pinned = 0;
                    this.prepare_media(w, c);
                    c.notify();
                }),
            ),
        );
        for category in &self.feed.categories {
            let id = category.id.clone();
            let count = counts.iter().filter(|n| n.category_id == id).count();
            categories = categories.child(
                category_chip(
                    SharedString::from(format!("category-{id}")),
                    format!("{}  {count}", category.name),
                    self.category.as_ref() == Some(&id),
                )
                .on_click(cx.listener(move |this, _, w, c| {
                    this.category = Some(id.clone());
                    this.visible = 6;
                    this.pinned = 0;
                    this.prepare_media(w, c);
                    c.notify();
                })),
            );
        }
        let mut header = v_flex()
            .relative()
            .w_full()
            .window_control_area(WindowControlArea::Drag)
            .gap(px(0.))
            .h(px(64.))
            .flex_shrink_0()
            .px(px(28.))
            .pt(px(8.))
            .child(design::eyebrow("COMUNIDAD"));
        let mut tabs = h_flex().gap(px(20.));
        for (ix, label, tab) in [
            (0, "Noticias", NewsTab::Recent),
            (1, "Historial", NewsTab::History),
            (2, "Guardadas", NewsTab::Saved),
        ] {
            tabs = tabs.child(design::tab(("news-tab", ix as usize), label, self.tab == tab).on_click(cx.listener(
                move |this, _, w, c| {
                    this.tab = tab;
                    this.visible = 6;
                    this.pinned = 0;
                    this.prepare_media(w, c);
                    c.notify();
                },
            )));
        }
        header = header.child(tabs).child(window_controls(window, 28.));
        let filters = h_flex()
            .gap(px(12.))
            .child(
                h_flex()
                    .id("news-category-strip")
                    .flex_1()
                    .min_w_0()
                    .h(px(32.))
                    .overflow_x_scroll()
                    .child(categories.flex_shrink_0()),
            )
            .child(
                DesignButton::new("news-profiles")
                    .aria_label("Perfiles")
                    .child(
                        h_flex()
                            .gap(px(7.))
                            .text_size(px(13.))
                            .child(
                                gpui_component::Icon::new(crate::icon::PandoraIcon::NewsFilter)
                                    .size(px(14.))
                                    .text_color(rgb(theme::ACCENT_TEXT)),
                            )
                            .child("Perfiles")
                            .when_some(self.profiles.as_ref(), |d, ids| {
                                let count =
                                    ids.iter().flat_map(|id| self.feed.lineage(id)).collect::<BTreeSet<_>>().len();
                                d.child(
                                    div()
                                        .bg(rgb(theme::ACCENT_SOFT))
                                        .text_color(rgb(theme::ACCENT_TEXT))
                                        .text_size(px(11.))
                                        .px(px(6.))
                                        .py(px(3.))
                                        .child(count.to_string()),
                                )
                            }),
                    )
                    .w(px(117.))
                    .h(px(32.))
                    .bg(rgba(0xffffff08))
                    .on_click(cx.listener(|this, _, w, c| this.profile_filter(w, c))),
            )
            .child(
                h_flex()
                    .w(px(210.))
                    .relative()
                    .child(
                        design::text_field(&self.search)
                            .prefix(gpui_component::Icon::new(crate::icon::PandoraIcon::Search).size(px(14.)))
                            .w_full(),
                    )
                    .child(
                        DesignButton::new("news-search-focus")
                            .label("Ctrl K")
                            .absolute()
                            .right(px(4.))
                            .h(px(32.))
                            .on_click(cx.listener(|this, _, w, c| {
                                this.search.read(c).focus_handle(c).focus(w, c);
                            })),
                    ),
            )
            .child(
                DesignButton::new("news-sort")
                    .label(if self.oldest {
                        "Más antiguas ▾"
                    } else {
                        "Más recientes ▾"
                    })
                    .h(px(32.))
                    .dropdown_menu({
                        let page = cx.entity().downgrade();
                        let oldest = self.oldest;
                        move |mut menu, _, _| {
                            for (old, label) in [(false, "Más recientes"), (true, "Más antiguas")] {
                                let page = page.clone();
                                menu = menu.item(PopupMenuItem::new(label).checked(old == oldest).on_click(
                                    move |_, w, c| {
                                        let _ = page.update(c, |this, c| {
                                            this.oldest = old;
                                            this.pinned = 0;
                                            this.prepare_media(w, c);
                                            c.notify();
                                        });
                                    },
                                ));
                            }
                            menu.min_w(px(168.))
                        }
                    }),
            );
        let mut body = v_flex().gap(px(24.));
        if self.feed.load_failed && self.loaded {
            body = body.child(empty("No se han podido cargar las noticias."));
        }
        if items.is_empty() && self.loaded {
            body = if !query.is_empty() {
                let category = self
                    .category
                    .as_deref()
                    .map(|id| self.category_name(id))
                    .unwrap_or_else(|| "Todas".into());
                body.child(v_flex().w_full().items_center().py(px(64.)).gap(px(16.)).child(div().font_family(theme::DISPLAY).text_size(px(22.)).child("Ninguna noticia coincide"))
                    .child(div().text_color(rgb(theme::INK_MUTED)).child(format!("No hay resultados para «{query}» en {category}. Prueba en todas las categorías o con otras palabras.")))
                    .child(DesignButton::new("news-search-all").label("Buscar en todas").on_click(cx.listener(|this,_,w,c|{this.category=None;this.prepare_media(w,c);c.notify();}))))
            } else if !self.feed.load_failed {
                body.child(empty(match self.tab {
                    NewsTab::Recent => "No hay noticias recientes.",
                    NewsTab::History => "No hay noticias anteriores.",
                    NewsTab::Saved => "No tienes noticias guardadas.",
                }))
            } else {
                body
            };
        } else {
            let pinned = items
                .iter()
                .filter(|n| n.pinned && !n.expired(&self.feed.server_time))
                .collect::<Vec<_>>();
            if !pinned.is_empty() {
                let ix = self.pinned.min(pinned.len() - 1);
                let n = pinned[ix];
                let mut dots = h_flex().gap(px(6.)).justify_center();
                for j in 0..pinned.len() {
                    dots = dots.child(
                        DesignButton::new(("news-pin-dot", j))
                            .p_0()
                            .child(div().size(px(6.)).rounded_full().bg(rgb(if ix == j {
                                theme::ACCENT_TEXT
                            } else {
                                theme::INK_DISABLED
                            })))
                            .aria_label(pinned[j].title.clone())
                            .text_color(rgb(if ix == j {
                                theme::ACCENT_TEXT
                            } else {
                                theme::INK_DISABLED
                            }))
                            .w(px(20.))
                            .h(px(20.))
                            .on_click(cx.listener(move |this, _, w, c| {
                                this.pinned = j;
                                this.prepare_media(w, c);
                                c.notify();
                            })),
                    );
                }
                body = body.child(
                    v_flex()
                        .gap(px(12.))
                        .child(design::eyebrow("Fijadas"))
                        .child(
                            self.story(
                                "news-pin-open",
                                n,
                                992.,
                                design::notched_panel(0x0f0f12ff)
                                    .id("pinned-news")
                                    .relative()
                                    .h(px(165.))
                                    .w(px(992.))
                                    .cursor_pointer()
                                    .when(n.image.is_some(), |d| d.child(self.image(n, "pinned", 992., 165.)))
                                    .child(div().absolute().inset_0().bg(rgba(0x06060788)))
                                    .child(
                                        v_flex()
                                            .absolute()
                                            .left(px(24.))
                                            .bottom(px(24.))
                                            .gap(px(8.))
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .text_color(rgb(theme::ACCENT_TEXT))
                                                    .child(self.category_name(&n.category_id)),
                                            )
                                            .child(
                                                div()
                                                    .font_family(theme::DISPLAY)
                                                    .text_size(px(24.))
                                                    .child(n.title.clone()),
                                            ),
                                    )
                                    .child(
                                        div().absolute().right(px(20.)).top(px(16.)).text_size(px(12.)).child(format!(
                                            "{}/{}",
                                            ix + 1,
                                            pinned.len()
                                        )),
                                    ),
                                cx,
                            ),
                        )
                        .child(dots),
                );
            }
            let unpinned = items
                .iter()
                .filter(|n| !n.pinned || n.expired(&self.feed.server_time))
                .collect::<Vec<_>>();
            if let Some(n) = unpinned.first() {
                let unread = self.marks(cx).is_none_or(|s| !s.read.contains(&n.id));
                body = body.child(
                    self.story(
                        "news-featured-open",
                        n,
                        992.,
                        design::notched_panel(0x0f0f12ff)
                            .id("news-featured")
                            .w(px(992.))
                            .when(n.image.is_some(), |d| d.h(px(311.)))
                            .flex()
                            .cursor_pointer()
                            .when(n.image.is_some(), |d| d.child(self.image(n, "featured", 551., 311.)))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .px(px(24.))
                                    .py(px(22.))
                                    .gap(px(12.))
                                    .child(
                                        h_flex()
                                            .justify_between()
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .text_color(rgb(theme::ACCENT_TEXT))
                                                    .child(self.category_name(&n.category_id)),
                                            )
                                            .when(unread, |d| d.child(new_tag())),
                                    )
                                    .child(
                                        div()
                                            .font_family(theme::DISPLAY)
                                            .text_size(px(17.))
                                            .line_height(px(22.))
                                            .line_clamp(3)
                                            .child(n.title.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_color(rgb(theme::INK_MUTED))
                                            .text_size(px(15.))
                                            .line_height(px(22.))
                                            .line_clamp(3)
                                            .child(
                                                self.previews
                                                    .get(&cover_key(n, "preview"))
                                                    .cloned()
                                                    .unwrap_or_default(),
                                            ),
                                    )
                                    .child(div().flex_1())
                                    .child(
                                        h_flex()
                                            .justify_between()
                                            .text_size(px(12.))
                                            .child(div().text_color(rgb(theme::INK_MUTED)).child(format!(
                                                "{} · por {}",
                                                date(&n.published_at, &self.feed.server_time),
                                                n.author
                                            )))
                                            .child(div().text_color(rgb(theme::INK_MUTED)).child("Leer")),
                                    ),
                            ),
                        cx,
                    ),
                );
            }
            if unpinned.len() > 1 {
                body = body
                    .child(
                        div()
                            .font_family(theme::DISPLAY)
                            .text_size(px(15.))
                            .line_height(px(20.))
                            .font_weight(FontWeight::BOLD)
                            .child("Últimas"),
                    )
                    .child(h_flex().flex_wrap().items_start().gap(px(16.)).children(
                        unpinned.iter().skip(1).take(self.visible).enumerate().map(|(ix, n)| self.card(n, ix, cx)),
                    ));
            }
            let shown = (self.visible + 1).min(unpinned.len());
            if !unpinned.is_empty() {
                body = body.child(
                    h_flex()
                        .justify_between()
                        .child(
                            div()
                                .text_color(rgb(theme::INK_MUTED))
                                .text_size(px(12.))
                                .child(format!("Mostrando {shown} de {}", unpinned.len())),
                        )
                        .when(shown < unpinned.len(), |d| {
                            d.child(DesignButton::new("news-more").label("Cargar más").on_click(cx.listener(
                                |this, _, w, c| {
                                    this.visible += 6;
                                    this.prepare_media(w, c);
                                    c.notify();
                                },
                            )))
                        }),
                );
            }
        }
        div()
            .min_w_0()
            .key_context("News")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::focus_search))
            .on_action(cx.listener(Self::leave_article))
            .size_full()
            .text_color(rgb(theme::INK))
            .font_family(theme::SANS)
            .text_size(px(13.))
            .child(
                v_flex()
                    .size_full()
                    .child(header)
                    .child(div().flex_shrink_0().bg(rgba(0x0f0f12e6)).px(px(28.)).py(px(16.)).child(filters))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scrollbar()
                            .px(px(28.))
                            .pt(px(24.))
                            .pb(px(40.))
                            .child(body),
                    ),
            )
            .into_any_element()
    }
}
