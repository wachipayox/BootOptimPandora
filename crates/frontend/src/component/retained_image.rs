//! Hold the last decoded visual while a replacement loads; geometry never changes.
use gpui::{prelude::*, *};
use std::{
    cell::Cell,
    rc::Rc,
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
        foreground: Option<Background>,
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
                None if self.current.is_none() => window
                    .use_asset::<ImgResourceLoader>(&fallback_resource, cx)
                    .and_then(|result| result.ok())
                    .map(|image| (fallback_resource, image)),
                None => None,
            },
            None if self.current.is_none() => window
                .use_asset::<ImgResourceLoader>(&fallback_resource, cx)
                .and_then(|result| result.ok())
                .map(|image| (fallback_resource, image)),
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
        let geometry = Rc::new(Cell::new(None));
        let image_element = |image: Arc<RenderImage>| {
            let child = img(image).size_full().object_fit(fit()).into_any_element();
            if foreground.is_some() {
                ImageGeometry {
                    child,
                    geometry: geometry.clone(),
                }
                .into_any_element()
            } else {
                child
            }
        };
        if let Some(previous) = &self.previous {
            window.on_next_frame(move |_, cx| cx.notify(owner));
            layer = layer.child(
                div()
                    .absolute()
                    .inset_0()
                    .size_full()
                    .opacity(1. - progress)
                    .child(image_element(previous.clone())),
            );
        }
        if let Some((_, image)) = &self.current {
            layer = layer.child(
                div()
                    .absolute()
                    .inset_0()
                    .size_full()
                    .opacity(if self.previous.is_some() { progress } else { 1. })
                    .child(image_element(image.clone())),
            );
        }
        if let Some(foreground) = foreground {
            layer = layer.child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, _| {
                        // Use the image's actual layout rectangle, including its
                        // device-pixel rounding, for the foreground as well.
                        window.paint_quad(fill(geometry.get().unwrap_or(bounds), foreground));
                    },
                )
                .absolute()
                .inset_0(),
            );
        }
        layer.into_any_element()
    }
}

/// Preserve the normal image element (including animated frames) while
/// sharing its final geometry with a foreground painted after both fade layers.
struct ImageGeometry {
    child: AnyElement,
    geometry: Rc<Cell<Option<Bounds<Pixels>>>>,
}
impl IntoElement for ImageGeometry {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for ImageGeometry {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.geometry.set(Some(bounds));
        self.child.prepaint(window, cx);
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::RetainedImage;
    use gpui::{RenderImage, Resource};
    use std::sync::Arc;

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
