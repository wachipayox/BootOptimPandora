//! Fixed-size PC news cards with the translucent surfaces used by Android.
use crate::theme;
use gpui::{prelude::*, *};
use gpui_component::{h_flex, v_flex};

pub struct NewsCard {
    pub width: f32,
    pub image: Option<AnyElement>,
    pub category: String,
    pub category_emphasis: bool,
    pub title: String,
    pub summary: String,
    pub date: String,
    pub pinned: bool,
    pub unread: bool,
}

impl NewsCard {
    pub fn render(self) -> Div {
        let has_image = self.image.is_some();
        v_flex()
            .w(px(self.width))
            .h(px(self.width + 80.))
            .flex_shrink_0()
            .min_w_0()
            .relative()
            .overflow_hidden()
            .border_1()
            .border_color(rgba(0xffffff1a))
            .bg(rgba(0xffffff14))
            .when_some(self.image, |d, image| d.child(div().w_full().flex_shrink_0().child(image)))
            .when(self.pinned, |d| {
                d.child(
                    div()
                        .absolute()
                        .top_0()
                        .w_full()
                        .h(px(15.))
                        .bg(rgba(0x80808080))
                        .text_center()
                        .font_family(theme::DISPLAY)
                        .text_size(px(9.))
                        .child("FIJADA"),
                )
            })
            .child(
                v_flex()
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .p(px(12.))
                    .when(self.pinned && !has_image, |d| d.pt(px(27.)))
                    .gap(px(8.))
                    .child(
                        h_flex()
                            .w_full()
                            .gap(px(8.))
                            .child(
                                h_flex()
                                    .min_w_0()
                                    .gap(px(4.))
                                    .text_size(px(11.))
                                    .py(px(2.))
                                    .text_color(rgb(if self.category_emphasis {
                                        theme::ACCENT_TEXT
                                    } else {
                                        theme::INK_MUTED
                                    }))
                                    .when(self.category_emphasis, |d| {
                                        d.child(div().size(px(5.)).flex_shrink_0().bg(rgb(theme::ACCENT_TEXT)))
                                    })
                                    .when(!self.category_emphasis, |d| d.bg(rgba(0xffffff0a)).px(px(6.)))
                                    .child(div().min_w_0().truncate().child(self.category)),
                            )
                            .child(div().flex_1())
                            .when(self.unread, |d| {
                                d.child(
                                    h_flex()
                                        .flex_shrink_0()
                                        .gap(px(5.))
                                        .text_size(px(11.))
                                        .text_color(rgb(theme::ACCENT_TEXT))
                                        .child(div().size(px(6.)).bg(rgb(theme::ACCENT_TEXT)))
                                        .child("Nueva"),
                                )
                            }),
                    )
                    .child(
                        div()
                            .font_family(theme::DISPLAY)
                            .text_size(px(15.))
                            .line_height(px(20.))
                            .line_clamp(2)
                            .child(self.title),
                    )
                    .child(div().text_size(px(12.)).line_height(px(18.)).line_clamp(3).child(self.summary))
                    .child(div().flex_1())
                    .child(div().text_size(px(11.)).text_color(rgb(theme::INK_MUTED)).child(self.date)),
            )
    }
}
