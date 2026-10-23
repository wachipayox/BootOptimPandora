//! Letter spacing for short design labels, using GPUI's existing shaped glyphs.
//! Text stays on the UI thread; glyph metrics, kerning and grapheme clusters stay intact.
use gpui::*;
use std::sync::Arc;
use unicode_segmentation::UnicodeSegmentation;

pub struct TrackedText {
    text: SharedString,
    em: f32,
}
impl TrackedText {
    pub fn new(text: impl Into<SharedString>, em: f32) -> Self {
        Self { text: text.into(), em }
    }
}
impl IntoElement for TrackedText {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for TrackedText {
    type RequestLayoutState = (ShapedLine, Pixels);
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        _: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let height = style.line_height.to_pixels(font_size.into(), window.rem_size());
        let mut shaped = window
            .text_system()
            .shape_line(self.text.clone(), font_size, &[style.to_run(self.text.len())], None)
            .with_len(self.text.len());
        let starts: Vec<_> = self.text.grapheme_indices(true).map(|(i, _)| i).collect();
        let tracking = font_size * self.em;
        // with_len creates a unique layout, preserving the shaped runs/decorations.
        let layout = Arc::get_mut(&mut *shaped).expect("new shaped layout is unique");
        for run in &mut layout.runs {
            for glyph in &mut run.glyphs {
                let cluster = starts.partition_point(|start| *start <= glyph.index).saturating_sub(1);
                glyph.position.x += tracking * cluster as f32;
            }
        }
        layout.width += tracking * starts.len() as f32;
        let width = layout.width;
        let id = window.request_measured_layout(Style::default(), move |known, _, _, _| {
            size(known.width.unwrap_or(width), known.height.unwrap_or(height))
        });
        (id, (shaped, height))
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Window,
        _: &mut App,
    ) {
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        _ = state.0.paint(bounds.origin, state.1, TextAlign::Left, None, window, cx);
    }
}
