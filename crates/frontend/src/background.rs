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
#[derive(Clone, PartialEq, Eq)]
struct Key {
    source: AppBackground,
    width: u32,
    height: u32,
}
#[derive(Default)]
struct BackdropCache {
    key: Option<Key>,
    image: Option<PreparedBackdrop>,
    pending: bool,
    attempted: bool,
}
impl Global for BackdropCache {}

#[derive(Clone)]
struct PreparedBackdrop {
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
                AppBackground::Image(path) | AppBackground::Artwork(Some(path)) => image::open(path).ok()?,
                AppBackground::Artwork(None) => {
                    image::load_from_memory(&Assets::get("images/artwork-vanilla.png")?.data).ok()?
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
    let state = cx.default_global::<BackdropCache>();
    if state.key.as_ref() != Some(&key) {
        state.key = Some(key.clone());
        state.image = None;
        state.attempted = false;
    }
    let image = state.image.as_ref().map(|prepared| {
        if glass {
            prepared.glass.clone()
        } else {
            prepared.surface.clone()
        }
    });
    if image.is_none() && !state.pending && !state.attempted {
        state.pending = true;
        state.attempted = true;
        let work_key = key.clone();
        let work = cx.background_executor().spawn(async move { prepare(&work_key) });
        cx.spawn(async move |cx| {
            let result = work.await;
            let _ = cx.update_global::<BackdropCache, _>(|cache, cx| {
                cache.pending = false;
                if cache.key.as_ref() == Some(&key) {
                    cache.image = result;
                    log::info!("glass-ready pid={}", std::process::id());
                }
                cx.refresh_windows();
            });
        })
        .detach();
    }
    if !glass {
        let base = div().absolute().top_0().left_0().size_full();
        return if let Some(image) = image {
            base.child(img(image).size_full().object_fit(ObjectFit::Cover)).into_any_element()
        } else {
            base.bg(rgb(theme::BG)).into_any_element()
        };
    }
    canvas(
        move |bounds, _, _| bounds,
        move |bounds, _, window, _| {
            if let Some(image) = &image {
                let full = Bounds::new(point(px(0.), px(0.)), size);
                window.with_content_mask(Some(ContentMask { bounds }), |window| {
                    let _ = window.paint_image(full, full, Default::default(), image.clone(), 0, false);
                });
            } else {
                window.paint_quad(fill(bounds, rgb(theme::BG)));
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
    .into_any_element()
}

pub struct BackgroundSelector {
    color: Entity<gpui_component::input::InputState>,
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
        Self { color }
    }
}
impl Render for BackgroundSelector {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use crate::component::design::{self, DesignButton};
        use gpui_component::{h_flex, v_flex};
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
                1 => {
                    let source = InterfaceConfig::get(cx)
                        .last_played_artwork
                        .as_ref()
                        .map(|path| ImageSource::Resource(Resource::Path(Arc::from(path.as_path()))))
                        .unwrap_or_else(|| {
                            ImageSource::Resource(Resource::Embedded("images/artwork-vanilla.png".into()))
                        });
                    preview.child(img(source).size_full().object_fit(ObjectFit::Cover))
                },
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
            choices = choices.child(
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
                    .on_click(cx.listener(move |_, _, _, cx| {
                        if index == 3 {
                            let path = cx.prompt_for_paths(PathPromptOptions {
                                files: true,
                                directories: false,
                                multiple: false,
                                prompt: Some("Elegir archivo…".into()),
                            });
                            cx.spawn(async move |_, cx| {
                                if let Ok(Ok(Some(paths))) = path.await {
                                    if let Some(path) = paths.into_iter().next() {
                                        let _ = cx.update(|cx| {
                                            InterfaceConfig::get_mut(cx).app_background = AppBackground::Image(path);
                                            cx.refresh_windows();
                                        });
                                    }
                                }
                            })
                            .detach();
                        } else {
                            let cfg = InterfaceConfig::get_mut(cx);
                            cfg.app_background = match index {
                                0 => AppBackground::Cafe,
                                1 => AppBackground::Artwork(cfg.last_played_artwork.clone()),
                                _ => AppBackground::Color(0x0b1a20),
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
