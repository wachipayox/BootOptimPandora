use crate::{
    component::design,
    entity::DataEntities,
    modals::{
        create_instance::CreateInstanceModalState,
        global_profiles::{family_roots, open_global_profile_family, profile_icon},
    },
    pages::{import::ImportPage, page::Page},
    theme,
};
use bridge::message::{GlobalProfileSummary, MessageToBackend};
use gpui::{prelude::*, *};
use gpui_component::{h_flex, spinner::Spinner, v_flex};

pub struct AddInstancePage {
    data: DataEntities,
    tab: usize,
    profiles: Option<Result<Vec<GlobalProfileSummary>, String>>,
    local: Entity<CreateInstanceModalState>,
    import: Entity<ImportPage>,
    task: Option<Task<()>>,
}
impl AddInstancePage {
    pub fn new(data: &DataEntities, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let local = cx.new(|cx| {
            CreateInstanceModalState::new(
                data.metadata.clone(),
                data.instances.clone(),
                data.backend_handle.clone(),
                window,
                cx,
            )
        });
        let import = cx.new(|cx| ImportPage::new(data, window, cx));
        let tab = match std::env::var("WBOMBO_UI_CAPTURE_PAGE").as_deref() {
            Ok("add-local" | "add-local-fabric" | "add-local-forge" | "add-local-neoforge") => 1,
            Ok("import") => 2,
            _ => 0,
        };
        let mut page = Self {
            data: data.clone(),
            tab,
            profiles: None,
            local,
            import,
            task: None,
        };
        page.load(cx);
        page
    }
    fn load(&mut self, cx: &mut Context<Self>) {
        self.profiles = None;
        let (send, receive) = tokio::sync::oneshot::channel();
        self.data.backend_handle.send(MessageToBackend::GetGlobalProfiles { channel: send });
        self.task = Some(cx.spawn(async move |entity, cx| {
            let result = receive.await.unwrap_or_else(|_| Err("backend closed".into()));
            _ = entity.update(cx, |page, cx| {
                if let Err(error) = &result {
                    log::warn!("Profile catalog failed: {error}");
                }
                page.profiles = Some(result);
                cx.notify();
            });
        }));
    }
}
impl Page for AddInstancePage {
    fn controls(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
    fn scrollable(&self, _: &App) -> bool {
        false
    }
}
impl Render for AddInstancePage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut tabs = h_flex().w_full().gap(px(28.)).px(px(20.)).border_b_1().border_color(rgba(0xffffff0b));
        for (index, label) in ["Perfil global", "Nuevo", "Importar"].into_iter().enumerate() {
            tabs = tabs.child(
                design::tab(("add-instance-tab", index), label, self.tab == index)
                    .h(px(32.))
                    .px(px(2.))
                    .on_click(cx.listener(move |page, _, _, cx| {
                        page.tab = index;
                        cx.notify();
                    })),
            );
        }
        let body = match self.tab {
            1 => v_flex()
                .w_full()
                .max_w(px(640.))
                .gap(px(16.))
                .p(px(24.))
                .child(div().font_family(theme::DISPLAY).text_size(px(15.)).child("Nueva instancia"))
                .child(self.local.clone())
                .into_any_element(),
            2 => div().w_full().child(self.import.clone()).into_any_element(),
            _ => {
                let mut content = v_flex()
                    .w_full()
                    .gap(px(16.))
                    .p(px(24.))
                    .child(div().font_family(theme::DISPLAY).text_size(px(15.)).child("Perfil global"))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(rgb(theme::INK_MUTED))
                            .child("Perfiles disponibles en el servidor."),
                    );
                match &self.profiles {
                    None => content = content.child(Spinner::new()),
                    Some(Err(_error)) => {
                        content = content.child("No se han podido cargar los perfiles.").child(
                            design::DesignButton::new("retry-add-catalog")
                                .label("Reintentar")
                                .on_click(cx.listener(|p, _, _, c| p.load(c))),
                        );
                    },
                    Some(Ok(profiles)) => {
                        let mut rows = v_flex().w_full().gap(px(8.));
                        for profile in family_roots(profiles) {
                            let family = profiles.clone();
                            let mut title = h_flex().w_full().gap(px(12.)).child(profile.name.clone());
                            if bridge::profile_family::is_test_profile(&profile.profile_id) {
                                title = title.child(
                                    div()
                                        .text_size(px(10.))
                                        .px(px(6.))
                                        .py(px(2.))
                                        .bg(rgba(0x5bcef51f))
                                        .text_color(rgb(theme::ACCENT_TEXT))
                                        .child("PRUEBA"),
                                );
                            }
                            rows = rows.child(
                                design::DesignButton::new(profile.profile_id.clone())
                                    .w_full()
                                    .h(px(54.))
                                    .child(
                                        h_flex()
                                            .w_full()
                                            .gap(px(12.))
                                            .items_center()
                                            .child(profile_icon(&profile, 36.))
                                            .child(v_flex().w_full().items_start().gap(px(4.)).child(title).child(
                                                div().text_size(px(11.)).text_color(rgb(theme::INK_MUTED)).child(
                                                    format!(
                                                        "{} {} · rev. {}",
                                                        profile.loader.pretty_name(),
                                                        profile.minecraft,
                                                        profile.stable_sequence.unwrap_or(profile.latest_sequence)
                                                    ),
                                                ),
                                            )),
                                    )
                                    .on_click(cx.listener(move |page, _, window, cx| {
                                        open_global_profile_family(
                                            profile.profile_id.clone(),
                                            family.clone(),
                                            page.data.backend_handle.clone(),
                                            window,
                                            cx,
                                        );
                                    })),
                            );
                        }
                        content = content.child(rows);
                    },
                }
                content.into_any_element()
            },
        };
        use gpui_component::scroll::ScrollableElement;
        v_flex()
            .size_full()
            .child(design::page_header("AÑADIR INSTANCIA", window))
            .child(tabs)
            .child(div().id("add-instance-body").flex_1().min_h_0().overflow_y_scrollbar().child(body))
    }
}
