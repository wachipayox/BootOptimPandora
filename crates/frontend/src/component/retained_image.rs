//! Hold the last decoded visual while a replacement loads; geometry never changes.
use gpui::{prelude::*, *};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Clone, Default)]
pub(crate) struct RetainedImage {
    current: Option<(Resource, Arc<RenderImage>)>,
    previous: Option<Arc<RenderImage>>,
    changed: Option<Instant>,
}
impl RetainedImage {
    fn accept_ready(&mut self, next: Option<(Resource, Arc<RenderImage>)>) {
        if let Some(next) = next
            && !self.current.as_ref().is_some_and(|(key, _)| key == &next.0)
        {
            self.previous = self.current.take().map(|(_, image)| image);
            self.current = Some(next);
            self.changed = Some(Instant::now());
        }
    }

    pub fn render(
        &mut self,
        source: Option<Resource>,
        fallback: &'static str,
        cover: bool,
        owner: EntityId,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let fallback_resource = Resource::Embedded(fallback.into());
        let next = match source {
            Some(source) => match window.use_asset::<ImgResourceLoader>(&source, cx) {
                Some(Ok(image)) => Some((source, image)),
                Some(Err(_)) => window
                    .use_asset::<ImgResourceLoader>(&fallback_resource, cx)
                    .and_then(|result| result.ok())
                    .map(|image| (fallback_resource, image)),
                None => None,
            },
            None => None,
        };
        self.accept_ready(next);
        let progress = self
            .changed
            .map(|time| (time.elapsed().as_secs_f32() / Duration::from_millis(300).as_secs_f32()).min(1.))
            .unwrap_or(1.);
        if progress >= 1. {
            self.previous = None;
        }
        let mut layer = div().relative().size_full();
        let fit = || if cover { ObjectFit::Cover } else { ObjectFit::Contain };
        if let Some(previous) = &self.previous {
            window.on_next_frame(move |_, cx| cx.notify(owner));
            layer = layer.child(
                div()
                    .absolute()
                    .inset_0()
                    .size_full()
                    .opacity(1. - progress)
                    .child(img(previous.clone()).size_full().object_fit(fit())),
            );
        }
        if let Some((_, image)) = &self.current {
            layer = layer.child(
                div()
                    .absolute()
                    .inset_0()
                    .size_full()
                    .opacity(if self.previous.is_some() { progress } else { 1. })
                    .child(img(image.clone()).size_full().object_fit(fit())),
            );
        }
        layer.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_replacement_retains_previous_across_view_recreation() {
        let image = |color| Arc::new(RenderImage::new([image::Frame::new(image::RgbaImage::from_pixel(1, 1, color))]));
        let a = image(image::Rgba([1, 2, 3, 255]));
        let b = image(image::Rgba([4, 5, 6, 255]));
        let mut first = RetainedImage::default();
        first.accept_ready(Some((Resource::Embedded("art-a".into()), a.clone())));
        let mut recreated = first.clone();
        recreated.accept_ready(None);
        assert!(Arc::ptr_eq(&recreated.current.as_ref().unwrap().1, &a));
        assert!(recreated.previous.is_none());
        recreated.accept_ready(Some((Resource::Embedded("art-b".into()), b.clone())));
        assert!(Arc::ptr_eq(recreated.previous.as_ref().unwrap(), &a));
        assert!(Arc::ptr_eq(&recreated.current.as_ref().unwrap().1, &b));
        assert!(recreated.changed.is_some());
    }
}
