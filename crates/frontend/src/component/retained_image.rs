//! Hold the last decoded visual while a replacement loads; geometry never changes.
use gpui::{prelude::*, *};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Default)]
pub(crate) struct RetainedImage {
    current: Option<(Resource, Arc<RenderImage>)>,
    previous: Option<Arc<RenderImage>>,
    changed: Option<Instant>,
}
impl RetainedImage {
    pub fn render(
        &mut self,
        source: Resource,
        fallback: &'static str,
        fit: ObjectFit,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let requested = window.use_asset::<ImgResourceLoader>(&source, cx);
        let fallback_resource = Resource::Embedded(fallback.into());
        let next = match requested {
            Some(Ok(image)) => Some((source, image)),
            Some(Err(_)) => window
                .use_asset::<ImgResourceLoader>(&fallback_resource, cx)
                .and_then(|result| result.ok())
                .map(|image| (fallback_resource, image)),
            None if self.current.is_none() => window
                .use_asset::<ImgResourceLoader>(&fallback_resource, cx)
                .and_then(|result| result.ok())
                .map(|image| (fallback_resource, image)),
            None => None,
        };
        if let Some(next) = next {
            if !self.current.as_ref().is_some_and(|(key, _)| key == &next.0) {
                self.previous = self.current.take().map(|(_, image)| image);
                self.current = Some(next);
                self.changed = Some(Instant::now());
            }
        }
        let progress = self
            .changed
            .map(|time| (time.elapsed().as_secs_f32() / Duration::from_millis(300).as_secs_f32()).min(1.))
            .unwrap_or(1.);
        if progress >= 1. {
            self.previous = None;
        }
        let mut layer = div().relative().size_full();
        if let Some(previous) = &self.previous {
            window.request_animation_frame();
            layer = layer.child(
                div()
                    .absolute()
                    .inset_0()
                    .size_full()
                    .opacity(1. - progress)
                    .child(img(previous.clone()).size_full().object_fit(fit.clone())),
            );
        }
        if let Some((_, image)) = &self.current {
            layer = layer.child(
                div()
                    .absolute()
                    .inset_0()
                    .size_full()
                    .opacity(if self.previous.is_some() { progress } else { 1. })
                    .child(img(image.clone()).size_full().object_fit(fit)),
            );
        }
        layer.into_any_element()
    }
}
