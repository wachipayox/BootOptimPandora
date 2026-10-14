use std::time::{Duration, Instant};
use bridge::{handle::BackendHandle, message::{GlobalProfileSummary, MessageToBackend}};
use gpui::{prelude::*, *};
use gpui_component::{ActiveTheme, StyledExt, WindowExt, h_flex, v_flex};

pub struct GlobalProfileCarousel {
    backend: BackendHandle,
    profiles: Option<Result<Vec<GlobalProfileSummary>, String>>,
    hovered: bool,
    offset: f32,
    card_width: f32,
    last_visible: Instant,
    last_tick: Instant,
}
impl GlobalProfileCarousel {
    pub fn new(backend: BackendHandle, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (send, receive) = tokio::sync::oneshot::channel();
        backend.send(MessageToBackend::GetGlobalProfiles { channel: send });
        cx.spawn(async move |entity, cx| {
            let profiles = receive.await.unwrap_or_else(|_| Err("Launcher backend stopped".into()));
            _ = entity.update(cx, |this, cx| { this.profiles = Some(profiles); cx.notify(); });
        }).detach();
        // A small independent entity handles motion, without rebuilding the local instance list.
        cx.spawn_in(window, async move |entity, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(16)).await;
                if entity.update_in(cx, |this, window, cx| {
                    let now = Instant::now();
                    let elapsed = now.saturating_duration_since(this.last_tick).as_secs_f32();
                    this.last_tick = now;
                    if this.last_visible.elapsed() < Duration::from_millis(200) && window.is_window_active() && !window.has_active_dialog(cx) && !this.hovered
                        && let Some(Ok(profiles)) = &this.profiles && !profiles.is_empty()
                    {
                        let loop_width = profiles.len() as f32 * this.card_width;
                        this.offset = (this.offset + 40.0 * elapsed).rem_euclid(loop_width);
                        cx.notify();
                    }
                }).is_err() { break; }
            }
        }).detach();
        Self { backend, profiles: None, hovered: false, offset: 0.0, card_width: 300.0, last_visible: Instant::now(), last_tick: Instant::now() }
    }
}
impl Render for GlobalProfileCarousel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.last_visible = Instant::now();
        let sidebar = crate::interface_config::InterfaceConfig::get(cx).sidebar_width;
        let sidebar = if sidebar <= 0.0 { 150.0 } else { sidebar };
        let width = (window.viewport_size().width.as_f32() - sidebar - 34.0).max(360.0);
        self.card_width = width / 3.0;
        let mut section = v_flex().gap_3().child(div().font_semibold().child("Global profiles"));
        match &self.profiles {
            None => section = section.child(div().text_sm().child("Loading global profiles…")),
            Some(Err(error)) => section = section.child(div().text_sm().text_color(cx.theme().danger).child(error.clone())),
            Some(Ok(profiles)) if profiles.is_empty() => section = section.child(div().text_sm().child("No global profiles published")),
            Some(Ok(profiles)) => {
                let count = profiles.len();
                let cards = (0..count + 4).map(|i| {
                    let profile = profiles[i % count].clone();
                    let backend = self.backend.clone();
                    let icon = match &profile.icon_path {
                        Some(path) => gpui::img(path.clone()).size_16().rounded_md().into_any_element(),
                        None => gpui::img(ImageSource::Resource(Resource::Embedded("images/default_mod.png".into()))).size_16().rounded_md().into_any_element(),
                    };
                    div().id(("global-profile-card", i)).w(px(self.card_width)).flex_shrink_0().pr_3()
                        .child(v_flex().id(("global-profile", i)).p_4().h(px(164.0)).gap_2().rounded_lg().border_1().border_color(cx.theme().border)
                            .bg(cx.theme().secondary).cursor_pointer()
                            .child(h_flex().gap_3().items_center().child(icon).child(v_flex().flex_1().min_w_0()
                                .child(div().font_semibold().truncate().child(profile.name.clone()))
                                .child(div().text_xs().child(format!("Minecraft {} · NeoForge {}", profile.minecraft, profile.neoforge)))))
                            .child(div().text_sm().truncate().child(profile.description.clone()))
                            .on_click(move |_, window, cx| crate::modals::global_profiles::open_global_profile_details(profile.clone(), backend.clone(), window, cx)))
                }).collect::<Vec<_>>();
                section = section.child(div().id("global-carousel").relative().h(px(164.0)).w_full().overflow_hidden()
                    .on_hover(cx.listener(|this, hovered, _, cx| { this.hovered = *hovered; cx.notify(); }))
                    .child(h_flex().absolute().left(px(-self.offset)).top_0().children(cards)));
            },
        }
        section
    }
}
