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
    image: Option<Arc<RenderImage>>,
    pending: bool,
    attempted: bool,
}
impl Global for BackdropCache {}

/// The regular background uses precisely the same object-fit-cover transform as glass.
pub fn surface(cx: &App) -> AnyElement {
    let source = InterfaceConfig::get(cx).app_background.clone();
    let base = div().absolute().size_full();
    match source {
        AppBackground::Cafe => base
            .child(
                img(ImageSource::Resource(Resource::Embedded("images/cafe-ambient.png".into())))
                    .size_full()
                    .object_fit(ObjectFit::Cover),
            )
            .into_any_element(),
        AppBackground::Color(color) => base.bg(rgb(color & 0xffffff)).into_any_element(),
        AppBackground::Artwork(None) => base
            .child(
                img(ImageSource::Resource(Resource::Embedded("images/artwork-vanilla.png".into())))
                    .size_full()
                    .object_fit(ObjectFit::Cover)
                    .opacity(0.15),
            )
            .into_any_element(),
        AppBackground::Artwork(Some(path)) | AppBackground::Image(path) => base
            .child(
                img(ImageSource::Resource(Resource::Path(Arc::from(path.as_path()))))
                    .size_full()
                    .object_fit(ObjectFit::Cover)
                    .opacity(0.15),
            )
            .into_any_element(),
    }
}
fn prepare(key: &Key) -> Option<Arc<RenderImage>> {
    let mut scene = match &key.source {
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
    scene = crate::component::design::prepare_play_artwork(&image::DynamicImage::ImageRgba8(scene));
    for pixel in scene.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Some(Arc::new(RenderImage::new([image::Frame::new(scene)])))
}
/// Draw the full prepared scene in window coordinates, clipped to the actual control bounds.
/// This avoids stretching an unrelated crop into every glass button.
pub fn glass(window: &mut Window, cx: &mut App) -> AnyElement {
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
    let image = state.image.clone();
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use crate::component::design::{self, DesignButton};
        use gpui_component::{h_flex, v_flex};
        let active = InterfaceConfig::get(cx).app_background.clone();
        let mut choices = h_flex().gap(px(10.)).w_full();
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
            choices = choices.child(
                DesignButton::new(("app-background", index))
                    .toggled(selected)
                    .flex_1()
                    .min_w_0()
                    .h(px(92.))
                    .px(px(8.))
                    .child(
                        v_flex()
                            .gap(px(8.))
                            .child(title)
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
        let mut colors = h_flex().gap(px(10.));
        for color in [0x060607u32, 0x0b1a20, 0x17100e, 0x10101a] {
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
            .child(choices)
            .when(matches!(active, AppBackground::Color(_)), |d| {
                d.child(colors.child(design::text_field(&self.color).w(px(120.)).font_family("Consolas")))
            })
    }
}
