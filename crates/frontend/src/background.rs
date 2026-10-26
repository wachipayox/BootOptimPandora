//! Position-aware launcher backdrop shared by the app and glass controls.
//! Only CPU texture preparation runs on the background executor; painting stays on GPUI.
use crate::{Assets, interface_config::InterfaceConfig, theme};
use gpui::{prelude::*, *};
use std::sync::Arc;

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppBackground {
    #[default]
    Cafe,
    Color(u32),
    Image(std::path::PathBuf),
    Artwork(Option<std::path::PathBuf>),
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Key {
    source: AppBackground,
    width: u32,
    height: u32,
}
#[derive(Default)]
struct BackdropCache {
    key: Option<Key>,
    image: Option<PreparedBackdrop>,
    pending: Option<Key>,
    failed: Option<Key>,
}
impl Global for BackdropCache {}

impl BackdropCache {
    fn request(&mut self, key: Key) -> Option<Key> {
        self.key = Some(key.clone());
        // Keep the last complete surface throughout a resize/source change.
        // A single worker follows the latest request instead of queuing every
        // intermediate resize or clearing the visible image between jobs.
        if self.pending.is_some()
            || self.failed.as_ref() == Some(&key)
            || self.image.as_ref().is_some_and(|image| image.key == key)
        {
            return None;
        }
        self.pending = Some(key.clone());
        Some(key)
    }

    fn complete(&mut self, key: &Key, image: Option<PreparedBackdrop>) {
        self.pending = None;
        if self.key.as_ref().is_some_and(|requested| requested.source == key.source) {
            if let Some(image) = image {
                self.image = Some(image);
                self.failed = None;
            } else {
                self.failed = Some(key.clone());
            }
        }
    }
}

#[derive(Clone)]
struct PreparedBackdrop {
    key: Key,
    surface: Arc<RenderImage>,
    glass: Arc<RenderImage>,
}

/// Both passes share the same prepared surface and cover transform.
pub fn surface(window: &mut Window, cx: &mut App) -> AnyElement {
    scene(window, cx, false)
}
fn prepare(key: &Key) -> Option<PreparedBackdrop> {
    let scene = match &key.source {
        AppBackground::Color(color) => image::RgbaImage::from_pixel(
            key.width,
            key.height,
            image::Rgba([
                ((color >> 16) & 255) as u8,
                ((color >> 8) & 255) as u8,
                (color & 255) as u8,
                255,
            ]),
        ),
        source => {
            let source = match source {
                AppBackground::Cafe => image::load_from_memory(&Assets::get("images/cafe-ambient.png")?.data).ok()?,
                AppBackground::Image(path) => image::open(path).ok()?,
                AppBackground::Artwork(path) => {
                    path.as_ref().and_then(|path| image::open(path).ok()).or_else(|| {
                        Assets::get("images/artwork-vanilla.png")
                            .and_then(|asset| image::load_from_memory(&asset.data).ok())
                    })?
                },
                _ => unreachable!(),
            };
            let mut scene = source
                .resize_to_fill(key.width, key.height, image::imageops::FilterType::Triangle)
                .to_rgba8();
            if !matches!(key.source, AppBackground::Cafe) {
                scene = image::DynamicImage::ImageRgba8(scene).blur(90.).to_rgba8();
                for pixel in scene.pixels_mut() {
                    let [r, g, b, _] = pixel.0;
                    let luminance = 0.213 * r as f32 + 0.715 * g as f32 + 0.072 * b as f32;
                    for channel in &mut pixel.0[..3] {
                        *channel = (luminance + (*channel as f32 - luminance) * 1.3).round().clamp(0., 255.) as u8;
                    }
                }
            }
            for pixel in scene.pixels_mut() {
                let opacity = if matches!(key.source, AppBackground::Cafe) {
                    1.0
                } else {
                    0.15
                };
                let alpha = pixel[3] as f32 / 255. * opacity;
                for (channel, base) in pixel.0[..3].iter_mut().zip([6., 6., 7.]) {
                    *channel = (*channel as f32 * alpha + base * (1. - alpha)).round() as u8;
                }
                pixel[3] = 255;
            }
            scene
        },
    };
    let glass = crate::component::design::prepare_play_artwork(&image::DynamicImage::ImageRgba8(scene.clone()));
    fn render_image(mut image: image::RgbaImage) -> Arc<RenderImage> {
        for pixel in image.pixels_mut() {
            pixel.0.swap(0, 2);
        }
        Arc::new(RenderImage::new([image::Frame::new(image)]))
    }
    Some(PreparedBackdrop {
        key: key.clone(),
        surface: render_image(scene),
        glass: render_image(glass),
    })
}
/// Draw in window coordinates, clipped to the actual glass control bounds.
pub fn glass(window: &mut Window, cx: &mut App) -> AnyElement {
    scene(window, cx, true)
}
fn scene(window: &mut Window, cx: &mut App, glass: bool) -> AnyElement {
    let size = window.viewport_size();
    let key = Key {
        source: InterfaceConfig::get(cx).app_background.clone(),
        width: size.width.as_f32().ceil().clamp(1., 8192.) as u32,
        height: size.height.as_f32().ceil().clamp(1., 8192.) as u32,
    };
    // Solid colors are already ready. Neither a resize nor a theme change
    // needs to wait for a full-sized bitmap to be prepared on a worker.
    if let AppBackground::Color(color) = key.source {
        cx.default_global::<BackdropCache>().key = Some(key);
        let color = if glass { saturated_color(color, 1.5) } else { color };
        return div().absolute().top_0().left_0().size_full().bg(rgb(color)).into_any_element();
    }
    let state = cx.default_global::<BackdropCache>();
    let work = state.request(key.clone());
    let image = state.image.as_ref().map(|prepared| {
        (
            if glass {
                prepared.glass.clone()
            } else {
                prepared.surface.clone()
            },
            prepared.key.width,
            prepared.key.height,
        )
    });
    if let Some(key) = work {
        let work_key = key.clone();
        let work = cx.background_executor().spawn(async move { prepare(&work_key) });
        cx.spawn(async move |cx| {
            let result = work.await;
            let _ = cx.update_global::<BackdropCache, _>(|cache, cx| {
                let ready = result.is_some();
                cache.complete(&key, result);
                if ready {
                    log::info!("glass-ready pid={}", std::process::id());
                }
                cx.refresh_windows();
            });
        })
        .detach();
    }
    // Cafe's embedded center pixel (33,16,12) is available immediately, before
    // its first decode. Retained surfaces cover subsequent pending requests.
    let fallback = if matches!(key.source, AppBackground::Cafe) {
        0x21100c
    } else {
        theme::BG
    };
    canvas(
        move |bounds, _, _| bounds,
        move |bounds, _, window, _| {
            if let Some((image, width, height)) = &image {
                let full = cover_bounds(size, *width, *height);
                window.with_content_mask(Some(ContentMask { bounds }), |window| {
                    let _ = window.paint_image(full, full, Default::default(), image.clone(), 0, false);
                });
            } else {
                window.paint_quad(fill(bounds, rgb(fallback)));
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
    .into_any_element()
}

fn cover_bounds(viewport: Size<Pixels>, width: u32, height: u32) -> Bounds<Pixels> {
    let scale = (viewport.width.as_f32() / width as f32).max(viewport.height.as_f32() / height as f32);
    let size = size(px(width as f32 * scale), px(height as f32 * scale));
    Bounds::new(point((viewport.width - size.width) / 2., (viewport.height - size.height) / 2.), size)
}

fn saturated_color(color: u32, saturation: f32) -> u32 {
    let channels = [
        ((color >> 16) & 255) as f32,
        ((color >> 8) & 255) as f32,
        (color & 255) as f32,
    ];
    let luminance = 0.213 * channels[0] + 0.715 * channels[1] + 0.072 * channels[2];
    channels.into_iter().fold(0, |value, channel| {
        (value << 8) | (luminance + (channel - luminance) * saturation).round().clamp(0., 255.) as u32
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(width: u32) -> Key {
        Key {
            source: AppBackground::Cafe,
            width,
            height: 600,
        }
    }

    fn prepared(key: Key) -> PreparedBackdrop {
        let image = Arc::new(RenderImage::new([image::Frame::new(image::RgbaImage::new(1, 1))]));
        PreparedBackdrop {
            key,
            surface: image.clone(),
            glass: image,
        }
    }

    #[test]
    fn rapid_resizes_keep_a_surface_and_coalesce_to_the_latest_size() {
        let mut cache = BackdropCache::default();
        let original = key(1280);
        let first = cache.request(original.clone()).unwrap();
        cache.complete(&first, Some(prepared(first.clone())));
        let resize = cache.request(key(1100)).unwrap();
        assert!(cache.request(key(1000)).is_none());
        assert!(cache.request(key(960)).is_none());
        assert_eq!(cache.image.as_ref().unwrap().key, original);
        cache.complete(&resize, Some(prepared(resize.clone())));
        assert_eq!(cache.image.as_ref().unwrap().key, resize);
        assert_eq!(cache.request(key(960)), Some(key(960)));
    }

    #[test]
    fn obsolete_source_cannot_replace_a_new_selection() {
        let mut cache = BackdropCache::default();
        let old = cache.request(key(1280)).unwrap();
        let selected = Key {
            source: AppBackground::Color(0x0b1a20),
            ..key(1280)
        };
        cache.key = Some(selected.clone());
        cache.complete(&old, Some(prepared(old.clone())));
        assert!(cache.image.is_none());
        assert_eq!(cache.key, Some(selected));
    }

    #[test]
    fn failed_replacement_keeps_the_last_good_image_without_retrying_every_frame() {
        let mut cache = BackdropCache::default();
        cache.image = Some(prepared(key(1280)));
        let failed = Key {
            source: AppBackground::Image("missing.png".into()),
            ..key(1280)
        };
        let _ = cache.request(failed.clone());
        cache.complete(&failed, None);
        assert!(cache.image.is_some());
        assert!(cache.request(failed).is_none());
        assert!(cache.request(key(1000)).is_some());
    }

    #[test]
    fn retained_surface_and_glass_share_centered_cover_coordinates() {
        let bounds = cover_bounds(size(px(960.), px(600.)), 1280, 860);
        assert_eq!(bounds.origin.x, px(0.));
        assert_eq!(bounds.origin.y, px(-22.5));
        assert_eq!(bounds.size, size(px(960.), px(645.)));
        assert_eq!(saturated_color(0x202020, 1.5), 0x202020);
    }
}

pub struct BackgroundSelector {
    color: Entity<gpui_component::input::InputState>,
    artwork_source: Option<Option<std::path::PathBuf>>,
    artwork_preview: Option<Arc<RenderImage>>,
    artwork_task: Option<Task<()>>,
}
impl BackgroundSelector {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let color = match InterfaceConfig::get(cx).app_background {
            AppBackground::Color(c) => c,
            _ => 0x0b1a20,
        };
        let color =
            cx.new(|cx| gpui_component::input::InputState::new(window, cx).default_value(format!("#{color:06x}")));
        cx.subscribe(&color, |_, field, event: &gpui_component::input::InputEvent, cx| {
            if matches!(event, gpui_component::input::InputEvent::Change) {
                let value = field.read(cx).value();
                let value = value.trim().trim_start_matches('#');
                if value.len() == 6 {
                    if let Ok(color) = u32::from_str_radix(value, 16) {
                        InterfaceConfig::get_mut(cx).app_background = AppBackground::Color(color);
                        cx.refresh_windows();
                    }
                }
            }
        })
        .detach();
        Self {
            color,
            artwork_source: None,
            artwork_preview: None,
            artwork_task: None,
        }
    }
    fn artwork_preview(&mut self, cx: &mut Context<Self>) -> Option<Arc<RenderImage>> {
        let source = InterfaceConfig::get(cx).last_played_artwork.clone();
        if self.artwork_source.as_ref() != Some(&source) {
            self.artwork_source = Some(source.clone());
            self.artwork_preview = None;
            let expected = source.clone();
            let work = cx.background_executor().spawn(async move {
                let image = source.as_ref().and_then(|path| image::open(path).ok()).or_else(|| {
                    Assets::get("images/artwork-vanilla.png")
                        .and_then(|asset| image::load_from_memory(&asset.data).ok())
                })?;
                let mut pixels = image
                    .resize_to_fill(1280, 720, image::imageops::FilterType::Triangle)
                    .blur(90.)
                    .resize_to_fill(400, 225, image::imageops::FilterType::Triangle)
                    .to_rgba8();
                for pixel in pixels.pixels_mut() {
                    let [r, g, b, _] = pixel.0;
                    let luminance = 0.213 * r as f32 + 0.715 * g as f32 + 0.072 * b as f32;
                    for channel in &mut pixel.0[..3] {
                        *channel = (luminance + (*channel as f32 - luminance) * 1.3).round().clamp(0., 255.) as u8;
                    }
                    pixel.0.swap(0, 2);
                }
                Some(Arc::new(RenderImage::new([image::Frame::new(pixels)])))
            });
            self.artwork_task = Some(cx.spawn(async move |entity, cx| {
                let preview = work.await;
                _ = entity.update(cx, |selector, cx| {
                    if selector.artwork_source.as_ref() == Some(&expected) {
                        selector.artwork_preview = preview;
                        cx.notify();
                    }
                });
            }));
        }
        self.artwork_preview.clone()
    }
}
impl Render for BackgroundSelector {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use crate::component::design::{self, DesignButton};
        use gpui_component::{WindowExt, h_flex, v_flex};
        let active = InterfaceConfig::get(cx).app_background.clone();
        let mut choices = h_flex().gap(px(10.)).w_full();
        let preview_width = ((window.viewport_size().width.as_f32() - 220. - 48. - 36. - 30.) / 4.).clamp(60., 400.);
        let preview_height = preview_width * 9. / 16.;
        for (index, title, note) in [
            (0usize, "Fondo café 18:00", "El de siempre"),
            (1, "Último artwork", "De la última instancia que jugaste"),
            (2, "Color específico", ""),
            (3, "Imagen propia", "Elegir archivo…"),
        ] {
            let note = if index == 2 {
                format!(
                    "#{:06x}",
                    match active {
                        AppBackground::Color(color) => color,
                        _ => 0x0b1a20,
                    }
                )
            } else {
                note.to_owned()
            };
            let selected = matches!(
                (&active, index),
                (AppBackground::Cafe, 0)
                    | (AppBackground::Artwork(_), 1)
                    | (AppBackground::Color(_), 2)
                    | (AppBackground::Image(_), 3)
            );
            let preview = div().w_full().h(px(preview_height)).overflow_hidden().bg(rgb(0x060607));
            let preview = match index {
                0 => preview.child(
                    img(ImageSource::Resource(Resource::Embedded("images/cafe-ambient.png".into())))
                        .size_full()
                        .object_fit(ObjectFit::Cover),
                ),
                1 => preview.when_some(self.artwork_preview(cx), |preview, image| {
                    preview.child(img(image).size_full().object_fit(ObjectFit::Cover))
                }),
                2 => preview.bg(rgb(match active {
                    AppBackground::Color(color) => color,
                    _ => 0x0b1a20,
                })),
                _ => {
                    if let AppBackground::Image(path) = &active {
                        preview.child(
                            img(ImageSource::Resource(Resource::Path(Arc::from(path.as_path()))))
                                .size_full()
                                .object_fit(ObjectFit::Cover),
                        )
                    } else {
                        preview
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(24.))
                            .text_color(rgb(theme::ACCENT_TEXT))
                            .child("+")
                    }
                },
            };
            choices =
                choices.child(
                    DesignButton::new(("app-background", index))
                        .toggled(selected)
                        .paint(
                            180.,
                            if selected {
                                [[0x5bcef51c, 0x5bcef505]; 3]
                            } else {
                                [[0xffffff05; 2]; 3]
                            },
                            rgb(theme::INK).into(),
                            rgb(theme::INK).into(),
                        )
                        .when(selected, |button| button.border_t_2().border_color(rgb(theme::ACCENT_TEXT)))
                        .flex_1()
                        .min_w_0()
                        .h(px(preview_height + 72.))
                        .px(px(8.))
                        .pt(px(8.))
                        .pb(px(12.))
                        .child(
                            v_flex()
                                .w_full()
                                .items_start()
                                .gap(px(8.))
                                .child(preview)
                                .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(title))
                                .child(div().text_size(px(12.)).text_color(rgb(theme::INK_MUTED)).child(note)),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if index == 3 {
                                let handle = window.window_handle();
                                let path = cx.prompt_for_paths(PathPromptOptions {
                                    files: true,
                                    directories: false,
                                    multiple: false,
                                    prompt: Some("Elegir archivo…".into()),
                                });
                                cx.spawn(async move |_, cx| {
                                    if let Ok(Ok(Some(paths))) = path.await {
                                        if let Some(path) = paths.into_iter().next() {
                                            let validation_path = path.clone();
                                            let valid = cx
                                                .background_executor()
                                                .spawn(async move {
                                                    image::ImageReader::open(validation_path)
                                                        .ok()
                                                        .and_then(|reader| reader.with_guessed_format().ok())
                                                        .and_then(|reader| reader.into_dimensions().ok())
                                                        .is_some_and(|(width, height)| {
                                                            width > 0 && height > 0 && width <= 16384 && height <= 16384
                                                        })
                                                })
                                                .await;
                                            if !valid {
                                                _ =
                                                    cx.update_window(handle, |_, window, cx| {
                                                        window.push_notification(
                                                gpui_component::notification::Notification::new()
                                                    .with_type(gpui_component::notification::NotificationType::Error)
                                                    .title("No se ha podido abrir la imagen."),cx)
                                                    });
                                                return;
                                            }
                                            let _ = cx.update(|cx| {
                                                InterfaceConfig::get_mut(cx).app_background =
                                                    AppBackground::Image(path);
                                                cx.refresh_windows();
                                            });
                                        }
                                    }
                                })
                                .detach();
                            } else {
                                let color =
                                    u32::from_str_radix(this.color.read(cx).value().trim().trim_start_matches('#'), 16)
                                        .unwrap_or(0x0b1a20)
                                        & 0xffffff;
                                let cfg = InterfaceConfig::get_mut(cx);
                                cfg.app_background = match index {
                                    0 => AppBackground::Cafe,
                                    1 => AppBackground::Artwork(cfg.last_played_artwork.clone()),
                                    _ => AppBackground::Color(color),
                                };
                                cx.refresh_windows();
                            }
                        })),
                );
        }
        let mut colors = h_flex()
            .gap(px(10.))
            .items_center()
            .child(div().text_size(px(11.)).text_color(rgb(theme::INK_MUTED)).child("Color específico"));
        for color in [0x060607u32, 0x0b1a20, 0x14100c, 0x120b16, 0x0b140c] {
            colors = colors.child(
                DesignButton::new(("background-color", color as usize))
                    .size(px(26.))
                    .bg(rgb(color))
                    .border_1()
                    .border_color(rgba(0xffffff1f))
                    .when(active == AppBackground::Color(color), |button| {
                        button.border_2().border_color(rgb(theme::ACCENT_TEXT))
                    })
                    .on_click(cx.listener(move |this, _, w, cx| {
                        InterfaceConfig::get_mut(cx).app_background = AppBackground::Color(color);
                        this.color.update(cx, |s, cx| s.set_value(format!("#{color:06x}"), w, cx));
                        cx.refresh_windows();
                    })),
            );
        }
        v_flex()
            .gap(px(12.))
            .w_full()
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(rgb(theme::INK_MUTED))
                    .child("La imagen difuminada que se ve detrás de todas las pantallas."),
            )
            .child(choices)
            .child(
                colors.child(
                    design::text_field(&self.color)
                        .w(px(90.))
                        .h(px(30.))
                        .text_size(px(13.))
                        .font_family("Consolas"),
                ),
            )
    }
}
