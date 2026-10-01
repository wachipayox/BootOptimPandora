use std::sync::Arc;

use gpui::{
    App, AppContext, AvailableSpace, Bounds, Element, Entity, IntoElement, RenderImage, Size, Style, Task, px, size,
};
use schema::{minecraft_profile::SkinVariant, unique_bytes::UniqueBytes};

use crate::interface_config::InterfaceConfig;

pub const DEFAULT_YAW: f64 = 22.5;
pub const DEFAULT_PITCH: f64 = 10.5;
pub const DEFAULT_ANIMATION: f64 = 1.0 / 16.0;

struct RenderedPlayerModel {
    image: Arc<RenderImage>,
    skin: UniqueBytes,
    cape: Option<UniqueBytes>,
    variant: SkinVariant,
    yaw: f64,
    pitch: f64,
    animation: f64,
    zoom: f64,
    width: u32,
    height: u32,
}

pub struct PlayerModelState {
    pub skin: UniqueBytes,
    pub cape: Option<UniqueBytes>,
    pub variant: SkinVariant,
    pub yaw: f64,
    pub pitch: f64,
    pub animation: f64,
    textures: Option<(UniqueBytes, Option<UniqueBytes>, Arc<crate::skin_renderer::SkinTextures>)>,
    failed_textures: Option<(UniqueBytes, Option<UniqueBytes>)>,
    rendered: Option<RenderedPlayerModel>,
    render_task: Option<Task<()>>,
    render_scratch: crate::skin_renderer::SkinRenderScratch,
}

impl PlayerModelState {
    pub fn new(cx: &mut App, skin: UniqueBytes, variant: SkinVariant) -> Entity<Self> {
        let entity = cx.new(|_| Self {
            skin,
            cape: None,
            variant,
            yaw: DEFAULT_YAW,
            pitch: DEFAULT_PITCH,
            animation: DEFAULT_ANIMATION,
            textures: None,
            failed_textures: None,
            rendered: None,
            render_task: None,
            render_scratch: Default::default(),
        });
        cx.observe_release(&entity, |entity, cx| {
            if let Some(rendered) = entity.rendered.take() {
                cx.drop_image(rendered.image, None);
            }
        })
        .detach();
        entity
    }

    pub fn needs_rerender(&self, width: u32, height: u32, zoom: f64) -> bool {
        let Some(rendered) = &self.rendered else {
            return true;
        };
        return rendered.width != width
            || rendered.height != height
            || rendered.yaw != self.yaw
            || rendered.pitch != self.pitch
            || rendered.animation != self.animation
            || rendered.variant != self.variant
            || rendered.skin != self.skin
            || rendered.cape != self.cape
            || rendered.zoom != zoom;
    }
}

pub struct PlayerModel {
    state: Entity<PlayerModelState>,
}

impl PlayerModel {
    pub fn new(state: &Entity<PlayerModelState>) -> Self {
        Self { state: state.clone() }
    }
}

impl IntoElement for PlayerModel {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for PlayerModel {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _global_id: Option<&gpui::GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut gpui::Window,
        _cx: &mut gpui::App,
    ) -> (gpui::LayoutId, Self::RequestLayoutState) {
        let layout_id =
            window.request_measured_layout(Style::default(), move |known, available_space, _window, _cx| {
                let height = if let Some(height) = known.height {
                    height
                } else {
                    match available_space.height {
                        AvailableSpace::Definite(pixels) => pixels,
                        AvailableSpace::MinContent => px(0.0),
                        AvailableSpace::MaxContent => px(1000.0),
                    }
                };

                let width = px(height.as_f32() * crate::skin_renderer::ASPECT_RATIO as f32);

                size(width, height)
            });

        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _global_id: Option<&gpui::GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        _bounds: gpui::Bounds<gpui::Pixels>,
        _element_size: &mut Self::RequestLayoutState,
        _window: &mut gpui::Window,
        _cx: &mut gpui::App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _global_id: Option<&gpui::GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: gpui::Bounds<gpui::Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) {
        let element_height = bounds.size.height.as_f32().round();
        let element_width = (element_height as f32 * crate::skin_renderer::ASPECT_RATIO as f32).round();
        let window_scale = window.scale_factor();
        let physical_height = (element_height * window_scale) as u32;
        let zoom = InterfaceConfig::get(cx).player_model_zoom.clamp(50, 400) as f64 / 100.0;
        self.state.update(cx, |state, cx| {
            // Preserve native physical resolution, including the window DPI scale.
            let image_height = physical_height;
            let image_width = (image_height as f64 * crate::skin_renderer::ASPECT_RATIO).round() as u32;
            let invalid_texture = state.failed_textures.as_ref()
                .is_some_and(|(skin, cape)| skin == &state.skin && cape == &state.cape);
            if image_width > 0 && image_height > 0 && !invalid_texture
                && state.render_task.is_none() && state.needs_rerender(image_width, image_height, zoom) {
                let skin = state.skin.clone();
                let cape = state.cape.clone();
                let yaw = state.yaw;
                let pitch = state.pitch;
                let animation = state.animation;
                let variant = state.variant;
                let mut scratch = std::mem::take(&mut state.render_scratch);
                let textures = state.textures.as_ref()
                    .filter(|(cached_skin, cached_cape, _)| cached_skin == &skin && cached_cape == &cape)
                    .map(|(_, _, textures)| textures.clone());

                let (send, recv) = tokio::sync::oneshot::channel();

                cx.background_executor()
                    .spawn(async move {
                        let textures = textures.or_else(|| crate::skin_renderer::SkinTextures::decode(&skin, cape.as_deref()).map(Arc::new));
                        let data = textures.as_ref().and_then(|textures| crate::skin_renderer::render_skin_textures_with_scratch(
                            textures,
                            variant,
                            image_width,
                            image_height,
                            yaw,
                            pitch,
                            animation,
                            0.0,
                            zoom,
                            &mut scratch,
                        )).map(|mut data| {
                            // GPUI expects BGRA. Convert the full native framebuffer on the worker,
                            // not on the UI thread while it is handling input and drawing controls.
                            for pixel in data.chunks_exact_mut(4) {
                                pixel.swap(0, 2);
                            }
                            Arc::new(RenderImage::new([image::Frame::new(data)]))
                        });
                        send.send((textures, data, scratch))
                    })
                    .detach();

                let skin = state.skin.clone();
                let cape = state.cape.clone();
                state.render_task = Some(cx.spawn(async move |state, cx| {
                    let result = recv.await;

                    _ = state.update(cx, |state, cx| {
                        state.render_task = None;
                        let Ok((textures, data, scratch)) = result else { return; };
                        state.render_scratch = scratch;
                        if textures.is_none() {
                            if state.skin == skin && state.cape == cape {
                                if let Some(rendered) = state.rendered.take() {
                                    cx.drop_image(rendered.image, None);
                                }
                            }
                            state.failed_textures = Some((skin, cape));
                            cx.notify();
                            return;
                        }
                        let Some(render_image) = data else { return; };
                        if let Some(textures) = textures {
                            state.textures = Some((skin.clone(), cape.clone(), textures));
                            state.failed_textures = None;
                        }
                        // A queued result may belong to a skin replaced while the worker ran.
                        if state.skin != skin || state.cape != cape {
                            cx.notify();
                            return;
                        }
                        if let Some(rendered) = state.rendered.take() {
                            cx.drop_image(rendered.image, None);
                        }
                        state.rendered = Some(RenderedPlayerModel {
                            image: render_image,
                            skin,
                            cape,
                            variant,
                            yaw,
                            pitch,
                            animation,
                            zoom,
                            width: image_width,
                            height: image_height,
                        });
                        cx.notify();
                    });
                }));
            }

            if let Some(rendered) = &state.rendered {
                let image_bounds = Bounds {
                    origin: bounds.origin,
                    size: Size::new(px(element_width), px(element_height)),
                };
                _ = window.paint_image(
                    image_bounds,
                    image_bounds,
                    Default::default(),
                    rendered.image.clone(),
                    0,
                    false,
                );
            }
        });
    }
}
