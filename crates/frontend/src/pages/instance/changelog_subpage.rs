//! Revision history keeps cosmetic changes separate from the signed installation gate.
use crate::{
    Assets,
    component::design::{self, DesignButton},
    theme,
};
use bridge::{handle::BackendHandle, instance::InstanceID, message::MessageToBackend};
use gpui::{prelude::*, *};
use gpui_component::{checkbox::Checkbox, h_flex, scroll::ScrollableElement, v_flex};
use schema::changelog::{ChangelogEntry, ChangelogQuery, ProfileChangelog};
use std::{collections::BTreeSet, sync::Arc};

pub struct InstanceChangelogSubpage {
    backend: BackendHandle,
    history: ProfileChangelog,
    hidden: BTreeSet<String>,
    kind: usize,
    period: usize,
    failed: bool,
    loaded: bool,
    stones: Entity<StoneField>,
}
fn color(depth: u32) -> u32 {
    [theme::ACCENT_TEXT, 0xb899f0, 0x86d96f, 0xf0a23a][depth.min(3) as usize]
}
fn has_mods(entry: &ChangelogEntry) -> bool {
    !entry.mods.added.is_empty() || !entry.mods.removed.is_empty() || !entry.mods.updated.is_empty()
}
fn recent(entry: &ChangelogEntry, period: usize) -> bool {
    let days = match period {
        0 => 30,
        1 => 183,
        _ => return true,
    };
    chrono::DateTime::parse_from_rfc3339(&entry.created_at)
        .is_ok_and(|date| chrono::Utc::now().signed_duration_since(date).num_days() < days)
}
fn relative_date(value: &str) -> String {
    let Ok(date) = chrono::DateTime::parse_from_rfc3339(value) else {
        return String::new();
    };
    let days = chrono::Utc::now().signed_duration_since(date).num_days().max(0);
    match days {
        0 => "hoy".into(),
        1 => "ayer".into(),
        2..=6 => format!("hace {days} días"),
        7..=13 => "hace 1 semana".into(),
        14..=29 => format!("hace {} semanas", days / 7),
        30..=59 => "hace 1 mes".into(),
        _ => format!("hace {} meses", days / 30),
    }
}
impl InstanceChangelogSubpage {
    pub fn new(id: InstanceID, backend: BackendHandle, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let stones = cx.new(|cx| StoneField::new(window, cx));
        let (send, receive) = tokio::sync::oneshot::channel();
        backend.send(MessageToBackend::GetInstanceProfileInfo { id, channel: send });
        let loader = backend.clone();
        cx.spawn(async move |this, cx| {
            let profile = match receive.await {
                Ok(Ok(info)) => info.global_profile_id,
                _ => None,
            };
            let Some(profile_id) = profile else {
                let _ = this.update(cx, |this, cx| {
                    this.loaded = true;
                    cx.notify();
                });
                log::info!("instance-changelog-ready pid={}", std::process::id());
                return;
            };
            let mut cursor = None;
            let mut visited = BTreeSet::new();
            loop {
                let (send, receive) = tokio::sync::oneshot::channel();
                loader.send(MessageToBackend::GetProfileChangelog {
                    profile_id: profile_id.clone(),
                    query: ChangelogQuery {
                        cursor: cursor.clone(),
                        ..Default::default()
                    },
                    channel: send,
                });
                match receive.await {
                    Ok(Ok(page)) => {
                        let next = page.next_cursor.clone();
                        if this
                            .update(cx, |this, cx| {
                                this.history.profile_id = page.profile_id;
                                this.history.lineage = page.lineage;
                                this.history.entries.extend(page.entries);
                                this.loaded = true;
                                cx.notify();
                                log::info!("instance-changelog-ready pid={}", std::process::id());
                            })
                            .is_err()
                        {
                            break;
                        }
                        match next {
                            Some(next) if visited.insert(next.clone()) => cursor = Some(next),
                            _ => break,
                        }
                    },
                    _ => {
                        let _ = this.update(cx, |this, cx| {
                            this.loaded = true;
                            this.failed = true;
                            cx.notify();
                            log::info!("instance-changelog-ready pid={}", std::process::id());
                        });
                        break;
                    },
                }
            }
        })
        .detach();
        Self {
            backend,
            history: ProfileChangelog::default(),
            hidden: BTreeSet::new(),
            kind: 0,
            period: 2,
            failed: false,
            loaded: false,
            stones,
        }
    }
    fn visible(&self, entry: &ChangelogEntry) -> bool {
        !self.hidden.contains(&entry.origin.profile_id)
            && recent(entry, self.period)
            && (self.kind == 0 || (self.kind == 1) == has_mods(entry))
    }
    fn filters(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut panel = v_flex()
            .w(px(260.))
            .flex_shrink_0()
            .h_full()
            .px(px(18.))
            .py(px(22.))
            .gap(px(10.))
            .bg(rgba(0x08080af2))
            .border_l_1()
            .border_color(rgba(0xffffff0b))
            .child("Publicada desde");
        for profile in &self.history.lineage {
            let id = profile.profile_id.clone();
            let count = self
                .history
                .entries
                .iter()
                .filter(|e| {
                    e.origin.profile_id == id
                        && recent(e, self.period)
                        && (self.kind == 0 || (self.kind == 1) == has_mods(e))
                })
                .count();
            let origin = if profile.depth == 0 { "Esta instancia" } else { "Madre" };
            panel = panel.child(
                v_flex()
                    .gap(px(3.))
                    .child(
                        h_flex()
                            .w_full()
                            .gap(px(8.))
                            .child(div().size(px(5.)).bg(rgb(color(profile.depth))))
                            .child(
                                Checkbox::new(SharedString::from(format!("changelog-source-{id}")))
                                    .label(profile.name.clone())
                                    .checked(!self.hidden.contains(&id))
                                    .on_click(cx.listener(move |this, checked, _, cx| {
                                        if *checked {
                                            this.hidden.remove(&id);
                                        } else {
                                            this.hidden.insert(id.clone());
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(div().flex_1())
                            .child(div().text_color(rgb(theme::INK_MUTED)).child(count.to_string())),
                    )
                    .child(div().pl(px(29.)).text_size(px(11.)).text_color(rgb(theme::INK_MUTED)).child(origin)),
            );
        }
        panel = panel.child(div().mt(px(14.)).child("Cambios"));
        let mut kinds = h_flex().gap(px(2.));
        for (index, label) in ["Todo", "Con mods", "Sin mods"].into_iter().enumerate() {
            kinds = kinds.child(design::chip(("changes-filter", index), label, self.kind == index).on_click(
                cx.listener(move |this, _, _, cx| {
                    this.kind = index;
                    cx.notify();
                }),
            ));
        }
        panel = panel.child(kinds).child(div().mt(px(14.)).child("Periodo"));
        let mut periods = h_flex().gap(px(2.));
        for (index, label) in ["Mes", "6 meses", "Todo"].into_iter().enumerate() {
            periods = periods.child(design::chip(("period-filter", index), label, self.period == index).on_click(
                cx.listener(move |this, _, _, cx| {
                    this.period = index;
                    cx.notify();
                }),
            ));
        }
        panel.child(periods).into_any_element()
    }
    fn row(&self, entry: &ChangelogEntry, index: usize) -> AnyElement {
        let source = self.history.lineage.iter().find(|p| p.profile_id == entry.origin.profile_id);
        let tint = color(source.map(|p| p.depth).unwrap_or(0));
        let version = entry.revision.as_ref().map(|r| r.sequence).unwrap_or(entry.origin.sequence);
        let name = source.map(|p| p.name.as_str()).unwrap_or("");
        let exact = chrono::DateTime::parse_from_rfc3339(&entry.created_at)
            .map(|date| date.with_timezone(&chrono::Local).format("%d/%m/%Y, %H:%M").to_string())
            .unwrap_or_else(|_| entry.created_at.clone());
        let tooltip = format!("{}\nv{version} · {exact}\nPublicada desde {name}", entry.title);
        let profile = entry.origin.profile_id.clone();
        let revision = entry.origin.revision_id.clone();
        let backend = self.backend.clone();
        let first = h_flex()
            .gap(px(10.))
            .child(
                div()
                    .font_family(theme::DISPLAY)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(tint))
                    .child(format!("v{version}")),
            )
            .child(div().flex_1().min_w_0().text_ellipsis().child(entry.title.clone()))
            .when(!entry.mods.added.is_empty(), |d| {
                d.child(div().text_size(px(11.)).text_color(rgb(theme::SUCCESS)).child(format!(
                    "+{} {}",
                    entry.mods.added.len(),
                    if entry.mods.added.len() == 1 { "mod" } else { "mods" }
                )))
            })
            .when(!entry.mods.removed.is_empty(), |d| {
                d.child(div().text_size(px(11.)).text_color(rgb(theme::DANGER)).child(format!(
                    "−{} {}",
                    entry.mods.removed.len(),
                    if entry.mods.removed.len() == 1 { "mod" } else { "mods" }
                )))
            });
        let card = v_flex()
            .w_full()
            .min_w_0()
            .gap(px(6.))
            .px(px(14.))
            .py(px(9.))
            .bg(rgba(0x0b0b0de6))
            .border_l_2()
            .border_color(rgb(tint))
            .child(first)
            .child(
                h_flex()
                    .justify_between()
                    .text_size(px(12.))
                    .text_color(rgb(theme::INK_MUTED))
                    .child(name.to_owned())
                    .child(relative_date(&entry.created_at)),
            );
        h_flex()
            .w_full()
            .max_w(px(560.))
            .h(px(72.))
            .gap(px(18.))
            .child(revision_diamond(tint))
            .child(
                DesignButton::new(("changelog-row", index))
                    .flex_1()
                    .min_w_0()
                    .h(px(54.))
                    .px_0()
                    .tooltip(tooltip)
                    .paint(
                        0.,
                        [[0x00000000; 2], [0xffffff05; 2], [0xffffff08; 2]],
                        rgb(theme::INK).into(),
                        rgb(theme::INK).into(),
                    )
                    .child(card)
                    .on_click(move |_, w, cx| {
                        crate::pages::news_changelog::open_revision(
                            backend.clone(),
                            profile.clone(),
                            revision.clone(),
                            w,
                            cx,
                        )
                    }),
            )
            .into_any_element()
    }
}
impl Render for InstanceChangelogSubpage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.history.entries.iter().filter(|entry| self.visible(entry)).count();
        let mut rows = v_flex()
            .relative()
            .w_full()
            .min_h_full()
            .px(px(28.))
            .pt(px(22.))
            .pb(px(96.))
            .child(self.stones.clone())
            .child(
                div()
                    .absolute()
                    .left(px(36.5))
                    .top(px(96.))
                    .h(px(count.saturating_sub(1) as f32 * 72.))
                    .w(px(2.))
                    .bg(rgba(0xffffff1f)),
            )
            .child(
                div()
                    .relative()
                    .font_family(theme::DISPLAY)
                    .text_size(px(20.))
                    .font_weight(FontWeight::BOLD)
                    .mb(px(18.))
                    .child("Revisiones"),
            );
        for (index, entry) in self.history.entries.iter().enumerate() {
            if self.visible(entry) {
                rows = rows.child(self.row(entry, index));
            }
        }
        if self.failed {
            rows = rows.child(
                div()
                    .relative()
                    .py(px(24.))
                    .text_center()
                    .text_color(rgb(theme::INK_MUTED))
                    .child("No se ha podido cargar el changelog."),
            );
        }
        h_flex()
            .size_full()
            .items_start()
            .child(
                div()
                    .id("revision-history-scroll")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scrollbar()
                    .child(rows),
            )
            .child(self.filters(cx))
    }
}

/// Textures are enlarged on CPU with nearest-neighbour sampling at the window DPI.
/// Painting remains native; only tiles intersecting the viewport are submitted.
struct StoneField {
    textures: Vec<Arc<RenderImage>>,
    scale: f32,
    loading: bool,
}
impl StoneField {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            textures: Vec::new(),
            scale: 0.,
            loading: false,
        };
        this.load(window.scale_factor(), cx);
        this
    }
    fn load(&mut self, scale: f32, cx: &mut Context<Self>) {
        if self.loading || self.scale == scale {
            return;
        }
        self.loading = true;
        self.scale = scale;
        let work = cx.background_executor().spawn(async move {
            [
                "piedra", "carbon", "hierro", "cobre", "oro", "lapis", "diamante", "bedrock",
            ]
            .into_iter()
            .map(|name| {
                let asset = Assets::get(&format!("images/changelog/{name}.png"))?;
                let image = image::load_from_memory(&asset.data).ok()?;
                let size = (96. * scale).round().max(1.) as u32;
                let mut image = image.resize_exact(size, size, image::imageops::FilterType::Nearest).to_rgba8();
                for p in image.pixels_mut() {
                    p.0.swap(0, 2);
                }
                Some(Arc::new(RenderImage::new([image::Frame::new(image)])))
            })
            .collect::<Option<Vec<_>>>()
            .unwrap_or_default()
        });
        cx.spawn(async move |this, cx| {
            let textures = work.await;
            let _ = this.update(cx, |this, cx| {
                for old in std::mem::replace(&mut this.textures, textures) {
                    cx.drop_image(old, None);
                }
                this.loading = false;
                cx.notify();
                log::info!("instance-stone-ready pid={}", std::process::id());
            });
        })
        .detach();
    }
}
impl Render for StoneField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.load(window.scale_factor(), cx);
        let textures = self.textures.clone();
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .child(
                canvas(
                    |bounds, _, _| bounds,
                    move |bounds, _, window, _| {
                        if textures.len() != 8 {
                            return;
                        }
                        let mask = window.content_mask().bounds;
                        let tile = 96.;
                        let columns = (bounds.size.width.as_f32() / tile).ceil() as usize;
                        let rows = (bounds.size.height.as_f32() / tile).ceil() as usize;
                        let first = ((mask.origin.y - bounds.origin.y).as_f32() / tile).floor().max(0.) as usize;
                        let last = (((mask.origin.y + mask.size.height - bounds.origin.y).as_f32() / tile)
                            .ceil()
                            .max(0.) as usize)
                            .min(rows);
                        for y in first..last {
                            for x in 0..columns {
                                let hash = (x as u64).wrapping_mul(0x9e3779b97f4a7c15)
                                    ^ (y as u64).wrapping_mul(0xbf58476d1ce4e5b9);
                                let hash = hash ^ (hash >> 29);
                                let texture = if y + 1 == rows {
                                    7
                                } else if hash % 100 < 11 {
                                    1 + (hash / 100 % 6) as usize
                                } else {
                                    0
                                };
                                let rect = Bounds::new(
                                    bounds.origin + point(px(x as f32 * tile), px(y as f32 * tile)),
                                    size(px(tile), px(tile)),
                                );
                                let _ = window.paint_image(
                                    rect,
                                    rect,
                                    Default::default(),
                                    textures[texture].clone(),
                                    0,
                                    false,
                                );
                            }
                        }
                    },
                )
                .size_full(),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(linear_gradient(
                        90.,
                        linear_color_stop(rgba(0x060607d8), 0.),
                        linear_color_stop(rgba(0x060607b0), 0.7),
                    ))
                    .child(div().absolute().right_0().top_0().h_full().w(relative(0.3)).bg(linear_gradient(
                        90.,
                        linear_color_stop(rgba(0x06060700), 0.),
                        linear_color_stop(rgba(0x06060766), 1.),
                    ))),
            )
    }
}

fn revision_diamond(tint: u32) -> AnyElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let center = bounds.origin + point(bounds.size.width / 2., bounds.size.height / 2.);
            for (side, fill) in [
                (35., rgba((tint << 8) | 8)),
                (31., rgba((tint << 8) | 15)),
                (27., rgba((tint << 8) | 32)),
                (25., rgb(0x0b0b0d).into()),
                (17., rgb(tint).into()),
            ] {
                let radius = px(side / std::f32::consts::SQRT_2);
                let mut path = PathBuilder::fill();
                path.move_to(center + point(px(0.), -radius));
                path.line_to(center + point(radius, px(0.)));
                path.line_to(center + point(px(0.), radius));
                path.line_to(center + point(-radius, px(0.)));
                path.close();
                if let Ok(path) = path.build() {
                    window.paint_path(path, fill);
                }
            }
        },
    )
    .w(px(17.))
    .h(px(36.))
    .into_any_element()
}
