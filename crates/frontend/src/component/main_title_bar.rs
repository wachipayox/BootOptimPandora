use gpui::*;

use crate::component::{generic_title_bar::TitleBar, page_path::PagePath};

#[derive(IntoElement)]
pub struct MainTitleBar {
    pub page_path: PagePath,
    pub controls: AnyElement,
}

impl RenderOnce for MainTitleBar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        TitleBar {
            left_content: vec![
                div().overflow_hidden().pr_8().child(self.page_path).into_any_element(),
                self.controls,
            ],
            right_content: vec![crate::launcher_updates::indicator(cx)],
            content_only: !crate::root::should_render_custom_titlebar(),
        }
    }
}
