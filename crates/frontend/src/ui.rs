use std::{collections::VecDeque, sync::Arc};

use bridge::instance::InstanceID;
use gpui::{prelude::*, *};
use gpui_component::{h_flex, scroll::ScrollableElement, v_flex};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

use crate::{
    component::{main_title_bar::MainTitleBar, page_path::PagePath},
    entity::{
        DataEntities,
        account::AccountExt,
        instance::{
            InstanceAddedEvent, InstanceEntries, InstanceModifiedEvent, InstanceMovedToTopEvent, InstanceRemovedEvent,
        },
    },
    icon::PandoraIcon,
    interface_config::InterfaceConfig,
    pages::{
        curseforge_page::CurseforgeSearchPage, import::ImportPage, instance::instance_page::InstancePage,
        instances_page::InstancesPage, modrinth_page::ModrinthSearchPage, modrinth_project_page::ModrinthProjectPage,
        page::Page, skins_page::SkinsPage, syncing_page::SyncingPage,
    },
    png_render_cache,
};

pub struct LauncherUI {
    data: DataEntities,
    page: LauncherPage,
    pub(crate) home_account_menu: bool,
    pub(crate) browser_manual_view: bool,
    recent_instances: heapless::Vec<(InstanceID, SharedString), 3>,
    page_history_backwards: VecDeque<(PageType, Arc<[PageType]>)>,
    page_history_forwards: Vec<(PageType, Arc<[PageType]>)>,
    previous_pages: FxHashMap<PageType, LauncherPage>,
    pending_page: Option<(PageType, Arc<[PageType]>)>,
    _instance_added_subscription: Subscription,
    _instance_modified_subscription: Subscription,
    _instance_removed_subscription: Subscription,
    _instance_moved_to_top_subscription: Subscription,
}

#[derive(Default, Clone, Debug, PartialEq, Eq, Deserialize, Serialize, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum PageType {
    #[default]
    Instances,
    Skins,
    LauncherSettings,
    AddInstance,
    Modrinth {
        installing_for: Option<SharedString>,
    },
    Curseforge {
        installing_for: Option<SharedString>,
    },
    Import,
    Syncing,
    ModrinthProject {
        project_id: SharedString,
        project_title: SharedString,
        install_for: Option<SharedString>,
    },
    InstancePage {
        name: SharedString,
    },
}

impl PageType {
    pub fn title(&self, data: &DataEntities, cx: &App) -> SharedString {
        match self {
            PageType::Instances => t::instance::title().into(),
            PageType::Skins => "Aspecto".into(),
            PageType::LauncherSettings => "Ajustes del launcher".into(),
            PageType::AddInstance => "Añadir instancia".into(),
            PageType::Modrinth { installing_for } => {
                if installing_for.is_some() {
                    t::instance::content::install::from_modrinth().into()
                } else {
                    t::modrinth::name().into()
                }
            },
            PageType::Curseforge { installing_for } => {
                if installing_for.is_some() {
                    t::instance::content::install::from_curseforge().into()
                } else {
                    t::curseforge::name().into()
                }
            },
            PageType::Import => t::import::label().into(),
            PageType::Syncing => t::instance::sync::label().into(),
            PageType::ModrinthProject { project_title, .. } => project_title.clone(),
            PageType::InstancePage { name } => {
                InstanceEntries::find_title_by_name(&data.instances, name, cx).unwrap_or_else(|| name.clone())
            },
        }
    }
}

#[derive(Clone)]
pub enum LauncherPage {
    Home(Entity<crate::pages::home::HomePage>),
    Instances(Entity<InstancesPage>),
    Skins(Entity<SkinsPage>),
    LauncherSettings(Entity<crate::settings::SettingsRoot>),
    AddInstance(Entity<crate::pages::add_instance::AddInstancePage>),
    Modrinth(Entity<ModrinthSearchPage>),
    Curseforge(Entity<CurseforgeSearchPage>),
    Import(Entity<ImportPage>),
    Syncing(Entity<SyncingPage>),
    ModrinthProject(Entity<ModrinthProjectPage>),
    InstancePage(Entity<InstancePage>),
}

impl LauncherPage {
    fn render(self, ui: &LauncherUI, window: &mut Window, cx: &mut App) -> impl IntoElement {
        if let LauncherPage::Home(page) = &self {
            return div().size_full().child(page.clone()).into_any_element();
        }
        if let LauncherPage::InstancePage(page) = &self {
            return div().size_full().child(page.clone()).into_any_element();
        }
        if let LauncherPage::LauncherSettings(page) = &self {
            return div().size_full().child(page.clone()).into_any_element();
        }
        if let LauncherPage::Skins(page) = &self {
            return div().size_full().child(page.clone()).into_any_element();
        }
        if let LauncherPage::AddInstance(page) = &self {
            return div().size_full().child(page.clone()).into_any_element();
        }
        fn process(entity: Entity<impl Page>, window: &mut Window, cx: &mut App) -> (bool, AnyElement, AnyElement) {
            entity.update(cx, |page, cx| {
                (
                    page.scrollable(cx),
                    page.controls(window, cx).into_any_element(),
                    page.render(window, cx).into_any_element(),
                )
            })
        }

        let (scrollable, controls, page) = match self {
            LauncherPage::Home(_) | LauncherPage::LauncherSettings(_) | LauncherPage::AddInstance(_) => unreachable!(),
            LauncherPage::Instances(entity) => process(entity, window, cx),
            LauncherPage::Skins(entity) => process(entity, window, cx),
            LauncherPage::Modrinth(entity) => process(entity, window, cx),
            LauncherPage::Curseforge(entity) => process(entity, window, cx),
            LauncherPage::Import(entity) => process(entity, window, cx),
            LauncherPage::Syncing(entity) => process(entity, window, cx),
            LauncherPage::ModrinthProject(entity) => process(entity, window, cx),
            LauncherPage::InstancePage(_) => unreachable!(),
        };

        let config = InterfaceConfig::get(cx);
        let page_path = PagePath::new(ui.data.clone(), config.main_page.clone(), config.page_path.clone());
        let title_bar = MainTitleBar { page_path, controls };
        let browser = matches!(config.main_page, PageType::Modrinth { .. } | PageType::Curseforge { .. });
        let page = if browser && ui.browser_manual_view {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(13.))
                .text_color(rgb(crate::theme::INK_MUTED))
                .child("No hay descargas manuales pendientes.")
                .into_any_element()
        } else {
            page
        };
        let title_bar = if browser {
            v_flex()
                .w_full()
                .flex_shrink_0()
                .child(crate::component::design::page_header("EXPLORAR CONTENIDO", window))
                .child(
                    h_flex()
                        .px(px(20.))
                        .border_b_1()
                        .border_color(rgba(0xffffff0b))
                        .child(
                            crate::component::design::tab("browser-explore", "Explorar", !ui.browser_manual_view)
                                .h(px(32.))
                                .px(px(2.))
                                .on_click(|_, _, c| crate::root::set_browser_manual_view(false, c)),
                        )
                        .child(
                            crate::component::design::tab(
                                "browser-manual",
                                "Descargas manuales",
                                ui.browser_manual_view,
                            )
                            .on_click(|_, _, c| crate::root::set_browser_manual_view(true, c)),
                        ),
                )
                .into_any_element()
        } else if config.main_page == PageType::Syncing {
            v_flex()
                .w_full()
                .flex_shrink_0()
                .child(crate::component::design::page_header("ARCHIVOS", window))
                .child(
                    h_flex().px(px(20.)).border_b_1().border_color(rgba(0xffffff0b)).child(
                        crate::component::design::tab("sync-settings-tab", "Sincronización", true)
                            .h(px(32.))
                            .px(px(2.)),
                    ),
                )
                .into_any_element()
        } else if config.main_page == PageType::Import {
            v_flex()
                .w_full()
                .flex_shrink_0()
                .child(crate::component::design::page_header("AÑADIR INSTANCIA", window))
                .child(
                    h_flex()
                        .px(px(20.))
                        .border_b_1()
                        .border_color(rgba(0xffffff0b))
                        .child(crate::component::design::tab("add-global-tab", "Perfil global", false).on_click({
                            let data = ui.data.clone();
                            move |_, w, c| {
                                crate::modals::global_profiles::open_global_profiles(data.backend_handle.clone(), w, c)
                            }
                        }))
                        .child(crate::component::design::tab("add-new-tab", "Nuevo", false).on_click({
                            let data = ui.data.clone();
                            move |_, w, c| {
                                crate::modals::create_instance::open_create_instance(
                                    data.metadata.clone(),
                                    data.instances.clone(),
                                    data.backend_handle.clone(),
                                    w,
                                    c,
                                )
                            }
                        }))
                        .child(crate::component::design::tab("add-import-tab", "Importar", true)),
                )
                .into_any_element()
        } else {
            title_bar.into_any_element()
        };

        if scrollable {
            v_flex()
                .size_full()
                .child(title_bar)
                .child(
                    div()
                        .flex_1()
                        .overflow_hidden()
                        .child(v_flex().size_full().overflow_y_scrollbar().child(page)),
                )
                .into_any_element()
        } else {
            v_flex().size_full().child(title_bar).child(page).into_any_element()
        }
    }
}

impl LauncherUI {
    pub fn new(data: &DataEntities, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let recent_instances = data
            .instances
            .read(cx)
            .entries
            .iter()
            .take(3)
            .map(|(id, ent)| (*id, ent.read(cx).name.clone()))
            .collect();

        let _instance_added_subscription =
            cx.subscribe_in::<_, InstanceAddedEvent>(&data.instances, window, |this, _, event, window, cx| {
                if this.recent_instances.is_full() {
                    this.recent_instances.pop();
                }
                let _ = this.recent_instances.insert(0, (event.instance.id, event.instance.name.clone()));
                InterfaceConfig::get_mut(cx).home_instance = Some(event.instance.name.clone());
                this.switch_page_without_history(PageType::Instances, [].into(), window, cx);
                cx.notify();
            });
        let _instance_modified_subscription =
            cx.subscribe_in::<_, InstanceModifiedEvent>(&data.instances, window, |this, _, event, window, cx| {
                if matches!(&this.page,LauncherPage::Home(p) if p.read(cx).instance_id(cx)==Some(event.instance.id)) {
                    InterfaceConfig::get_mut(cx).home_instance = Some(event.instance.name.clone());
                }
                if let Some((_, name)) = this.recent_instances.iter_mut().find(|(id, _)| *id == event.instance.id) {
                    *name = event.instance.name.clone();
                    cx.notify();
                }
                if let LauncherPage::InstancePage(page) = &this.page
                    && page.read(cx).instance.read(cx).id == event.instance.id
                {
                    let page_path = InterfaceConfig::get_mut(cx).page_path.clone();
                    this.switch_page(
                        PageType::InstancePage {
                            name: event.instance.name.clone(),
                        },
                        &*page_path,
                        window,
                        cx,
                    );
                }
                cx.notify();
            });
        let _instance_removed_subscription =
            cx.subscribe_in::<_, InstanceRemovedEvent>(&data.instances, window, |this, _, event, window, cx| {
                this.recent_instances.retain(|entry| entry.0 != event.id);
                if matches!(&this.page,LauncherPage::Home(p) if p.read(cx).instance_id(cx)==Some(event.id)) {
                    this.switch_page_without_history(PageType::Instances, [].into(), window, cx);
                }

                if let LauncherPage::InstancePage(page) = &this.page
                    && page.read(cx).instance.read(cx).id == event.id
                {
                    this.switch_page(PageType::Instances, &[], window, cx);
                }
                cx.notify();
            });
        let _instance_moved_to_top_subscription =
            cx.subscribe::<_, InstanceMovedToTopEvent>(&data.instances, |this, _, event, cx| {
                this.recent_instances.retain(|entry| entry.0 != event.instance.id);
                if this.recent_instances.is_full() {
                    this.recent_instances.pop();
                }
                let _ = this.recent_instances.insert(0, (event.instance.id, event.instance.name.clone()));
                cx.notify();
            });

        let config = InterfaceConfig::get(cx);

        let main_page = config.main_page.clone();
        let original_page_path = config.page_path.clone();

        // If main_page failed to deserialize, also reset the path
        if main_page == PageType::Instances {
            let config = InterfaceConfig::get_mut(cx);
            config.page_path = [].into();
        }

        let mut pending_page = None;

        let page = match Self::create_page(&data, main_page.clone(), window, cx) {
            Ok(page) => page,
            Err(page_type) => {
                pending_page = Some((main_page, original_page_path));

                let config = InterfaceConfig::get_mut(cx);
                config.main_page = page_type.clone();
                config.page_path = [].into();
                Self::create_page(&data, page_type, window, cx).unwrap()
            },
        };

        Self {
            data: data.clone(),
            page,
            home_account_menu: false,
            browser_manual_view: false,
            recent_instances,
            page_history_backwards: VecDeque::with_capacity(32),
            page_history_forwards: Vec::new(),
            previous_pages: FxHashMap::default(),
            pending_page,
            _instance_added_subscription,
            _instance_modified_subscription,
            _instance_removed_subscription,
            _instance_moved_to_top_subscription,
        }
    }

    fn create_page(
        data: &DataEntities,
        page: PageType,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<LauncherPage, PageType> {
        match page {
            PageType::Instances => {
                Ok(LauncherPage::Home(cx.new(|cx| crate::pages::home::HomePage::new(data, window, cx))))
            },
            PageType::AddInstance => Ok(LauncherPage::AddInstance(
                cx.new(|cx| crate::pages::add_instance::AddInstancePage::new(data, window, cx)),
            )),
            PageType::LauncherSettings => Ok(LauncherPage::LauncherSettings(crate::settings::create(data, window, cx))),
            PageType::Skins => Ok(LauncherPage::Skins(cx.new(|cx| SkinsPage::new(data, window, cx)))),
            PageType::Modrinth { installing_for } => {
                let installing_for = installing_for
                    .as_ref()
                    .map(|name| InstanceEntries::find_id_by_name(&data.instances, name, cx));

                if let Some(None) = installing_for {
                    return Err(PageType::Modrinth { installing_for: None });
                }

                let page = cx.new(|cx| ModrinthSearchPage::new(installing_for.flatten(), data, window, cx));
                Ok(LauncherPage::Modrinth(page))
            },
            PageType::Curseforge { installing_for } => {
                let installing_for = installing_for
                    .as_ref()
                    .map(|name| InstanceEntries::find_id_by_name(&data.instances, name, cx));

                if let Some(None) = installing_for {
                    return Err(PageType::Curseforge { installing_for: None });
                }

                let page = cx.new(|cx| CurseforgeSearchPage::new(installing_for.flatten(), data, window, cx));
                Ok(LauncherPage::Curseforge(page))
            },
            PageType::Import => Ok(LauncherPage::Import(cx.new(|cx| ImportPage::new(data, window, cx)))),
            PageType::Syncing => Ok(LauncherPage::Syncing(cx.new(|cx| SyncingPage::new(data, window, cx)))),
            PageType::ModrinthProject {
                project_id,
                install_for,
                project_title,
            } => {
                let install_for_id = install_for
                    .as_ref()
                    .map(|name| InstanceEntries::find_id_by_name(&data.instances, name, cx));

                if let Some(None) = install_for_id {
                    return Err(PageType::ModrinthProject {
                        project_id,
                        install_for: None,
                        project_title,
                    });
                }

                let project_id = project_id.clone();
                let page =
                    cx.new(|cx| ModrinthProjectPage::new(project_id, install_for_id.flatten(), data, window, cx));
                Ok(LauncherPage::ModrinthProject(page))
            },
            PageType::InstancePage { ref name } => {
                if InterfaceConfig::get(cx).instance_subpage
                    == crate::pages::instance::instance_page::InstanceSubpageType::Quickplay
                {
                    InterfaceConfig::get_mut(cx).home_instance = Some(name.clone());
                    return Ok(LauncherPage::Home(cx.new(|cx| crate::pages::home::HomePage::new(data, window, cx))));
                }
                let Some(id) = InstanceEntries::find_id_by_name(&data.instances, name, cx) else {
                    return Err(PageType::Instances);
                };

                Ok(LauncherPage::InstancePage(cx.new(|cx| InstancePage::new(id, data, window, cx))))
            },
        }
    }

    pub fn switch_page(&mut self, page: PageType, page_path: &[PageType], window: &mut Window, cx: &mut Context<Self>) {
        let page_path: Arc<[PageType]> = page_path.into();
        self.browser_manual_view = false;

        let config = InterfaceConfig::get(cx);
        if config.main_page == page {
            return;
        }

        self.page_history_forwards.clear();
        if self.page_history_backwards.len() >= 32 {
            self.page_history_backwards.pop_back();
        }
        self.page_history_backwards
            .push_front((config.main_page.clone(), config.page_path.clone()));

        self.switch_page_without_history(page, page_path, window, cx);
    }

    fn switch_page_without_history(
        &mut self,
        page: PageType,
        page_path: Arc<[PageType]>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pending_page = None;

        let config = InterfaceConfig::get_mut(cx);
        let previous_page_type = std::mem::replace(&mut config.main_page, page.clone());
        config.main_page = page.clone();
        config.page_path = page_path.clone();

        if let Some(previous_page) = self.previous_pages.remove(&page) {
            self.page = previous_page;
            self.previous_pages.retain(|k, _| page_path.contains(k));
            cx.notify();
            return;
        }

        match Self::create_page(&self.data, page, window, cx) {
            Ok(page) => {
                let previous_page = std::mem::replace(&mut self.page, page);
                if page_path.contains(&previous_page_type) {
                    self.previous_pages.insert(previous_page_type, previous_page);
                }
                self.previous_pages.retain(|k, _| page_path.contains(k));
            },
            Err(fallback) => {
                let config = InterfaceConfig::get_mut(cx);
                config.main_page = fallback.clone();
                config.page_path = [].into();
                self.previous_pages.clear();
                self.page = Self::create_page(&self.data, fallback, window, cx).unwrap();
            },
        }

        cx.notify();
    }

    pub fn nav_backwards(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((page, page_path)) = self.page_history_backwards.pop_front() else {
            return;
        };

        let config = InterfaceConfig::get(cx);
        self.page_history_forwards.push((config.main_page.clone(), config.page_path.clone()));

        self.switch_page_without_history(page, page_path, window, cx);
    }

    pub fn nav_forwards(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((page, page_path)) = self.page_history_forwards.pop() else {
            return;
        };

        let config = InterfaceConfig::get(cx);
        self.page_history_backwards
            .push_front((config.main_page.clone(), config.page_path.clone()));

        self.switch_page_without_history(page, page_path, window, cx);
    }
}

impl Render for LauncherUI {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(pending_page) = self.pending_page.clone() {
            if let Ok(page) = Self::create_page(&self.data, pending_page.0.clone(), window, cx) {
                self.pending_page = None;
                self.previous_pages.clear();
                self.page_history_forwards.clear();
                self.page_history_backwards.clear();

                let config = InterfaceConfig::get_mut(cx);
                config.main_page = pending_page.0.clone();
                config.page_path = pending_page.1.clone();

                self.page = page;
            }
        }

        self.render_wachiland(window, cx)
    }
}

impl LauncherUI {
    fn render_wachiland(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        #[cfg(feature = "ui-capture")]
        if std::env::var("WBOMBO_UI_CAPTURE_PAGE").is_ok_and(|page| page == "backdrop-probe") {
            return div().size_full().child(crate::background::surface(window, cx)).into_any_element();
        }
        use crate::{
            component::design::{self, DesignButton},
            theme,
        };
        let selected = InterfaceConfig::get(cx).home_instance.clone();
        let mut instances = v_flex()
            .gap(px(4.))
            .child(div().px(px(8.)).pb(px(8.)).child(design::eyebrow("INSTANCIAS")));
        let entries = self.data.instances.read(cx).entries.clone();
        for (ix, (_id, entity)) in entries.iter().enumerate() {
            let i = entity.read(cx).clone();
            let name = i.name.clone();
            let active = selected.as_ref() == Some(&name);
            let icon = i
                .icon
                .clone()
                .map(|bytes| {
                    png_render_cache::render_with_transform(
                        bytes,
                        png_render_cache::ImageTransformation::Resize { width: 28, height: 28 },
                        cx,
                    )
                    .into_any_element()
                })
                .unwrap_or_else(|| {
                    gpui_component::Icon::new(PandoraIcon::InstanceDefault).size(px(28.)).into_any_element()
                });
            instances = instances.child(
                DesignButton::new(("sidebar-instance", ix))
                    .aria_label(name.clone())
                    .w_full()
                    .h(px(42.))
                    .when(active, |d| d.border_l_2().border_color(rgb(theme::ACCENT_TEXT)))
                    .justify_start()
                    .px(px(8.))
                    .paint(
                        90.,
                        if active {
                            [[0x5bcef51f, 0x5bcef500]; 3]
                        } else {
                            [[0x00000000; 2], [0xffffff0d; 2], [0xffffff14; 2]]
                        },
                        rgb(theme::INK).into(),
                        rgb(theme::INK).into(),
                    )
                    .child(h_flex().gap(px(8.)).child(icon).child(
                        v_flex().items_start().child(div().text_size(px(12.)).child(name.clone())).child(
                            div().text_size(px(10.)).text_color(rgb(theme::INK_DISABLED)).child(format!(
                                "{} {}",
                                i.configuration.loader.pretty_name(),
                                i.configuration.minecraft_version
                            )),
                        ),
                    ))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        InterfaceConfig::get_mut(cx).home_instance = Some(name.clone());
                        this.switch_page_without_history(PageType::Instances, [].into(), window, cx);
                    })),
            )
        }
        let data = self.data.clone();
        #[cfg(feature = "ui-capture")]
        if std::env::var("WBOMBO_UI_CAPTURE_PAGE").as_deref() == Ok("sidebar-scroll") {
            // Cosmetic rows exercise the real sidebar scrollbar without
            // creating backend instances or writing any additional folders.
            for index in 0usize..32 {
                instances = instances.child(
                    DesignButton::new(("capture-sidebar-row", index))
                        .label_left(format!("agente-274-scroll-{index:02}"))
                        .w_full()
                        .h(px(42.))
                        .on_click(|_, _, _| {}),
                );
            }
        }
        instances = instances.child(
            DesignButton::new("sidebar-add")
                .label_left("+   Añadir instancia")
                .w_full()
                .h(px(34.))
                .on_click(move |_, w, c| crate::pages::home::add_instance(&data, w, c)),
        );
        let unread = matches!(&self.page,LauncherPage::Home(p) if p.read(cx).has_unread(cx));
        let news_active = matches!(&self.page,LauncherPage::Home(p) if p.read(cx).is_news());
        let mut footer = v_flex().gap(px(4.));
        for (id, label, page) in [
            ("home", "Inicio", PageType::Instances),
            ("skin", "Aspecto", PageType::Skins),
            ("news", "Noticias", PageType::Instances),
        ] {
            footer = footer.child(
                DesignButton::new(id)
                    .aria_label(label)
                    .child(
                        h_flex()
                            .w_full()
                            .text_size(px(13.))
                            .font_family(theme::DISPLAY)
                            .gap(px(10.))
                            .child(
                                gpui_component::Icon::new(match id {
                                    "home" => PandoraIcon::HomeHouse,
                                    "skin" => PandoraIcon::HomeHanger,
                                    _ => PandoraIcon::HomeNews,
                                })
                                .size(px(16.)),
                            )
                            .child(label)
                            .when(id == "news" && unread, |d| {
                                d.child(div().size(px(6.)).rounded_full().bg(rgb(theme::ACCENT_TEXT)).with_animation(
                                    "news-pulse",
                                    Animation::new(std::time::Duration::from_secs(2)).repeat(),
                                    |d, t| d.opacity(0.45 + 0.55 * (t * std::f32::consts::TAU).cos().abs()),
                                ))
                            }),
                    )
                    .w_full()
                    .h(px(34.))
                    .when(
                        (id == "home"
                            && matches!(InterfaceConfig::get(cx).main_page, PageType::Instances)
                            && !news_active)
                            || (id == "skin" && matches!(InterfaceConfig::get(cx).main_page, PageType::Skins))
                            || (id == "news" && news_active),
                        |d| d.border_l_2().border_color(rgb(theme::ACCENT_TEXT)),
                    )
                    .justify_start()
                    .on_click(cx.listener(move |this, _, w, c| {
                        this.switch_page(page.clone(), &[], w, c);
                        if id == "home" {
                            if let LauncherPage::Home(home) = &this.page {
                                home.update(c, |p, c| p.show_home(c));
                            }
                        }
                        if id == "news" {
                            if let LauncherPage::Home(home) = &this.page {
                                home.update(c, |p, c| p.show_news(c));
                            }
                        }
                    })),
            );
        }
        let accounts = self.data.accounts.read(cx);
        let selected_account = accounts.selected_account.clone();
        let account_name = selected_account
            .as_ref()
            .map(|a| a.username(InterfaceConfig::get(cx).hide_usernames))
            .unwrap_or_else(|| "Sin cuenta".into());
        let hide_skins = InterfaceConfig::get(cx).hide_skins;
        let head = selected_account.as_ref().filter(|_| !hide_skins).and_then(|a| a.head.clone());
        let head = head
            .map(|b| {
                png_render_cache::render_with_transform(
                    b,
                    png_render_cache::ImageTransformation::Resize { width: 26, height: 26 },
                    cx,
                )
                .into_any_element()
            })
            .unwrap_or_else(|| {
                if selected_account.is_some() {
                    img(ImageSource::Resource(Resource::Embedded(
                        if hide_skins {
                            "images/hidden_head.png"
                        } else {
                            "images/default_head.png"
                        }
                        .into(),
                    )))
                    .size(px(26.))
                    .into_any_element()
                } else {
                    gpui_component::Icon::new(PandoraIcon::CircleUser).size(px(26.)).into_any_element()
                }
            });
        let saved_accounts = self.data.accounts.read(cx).accounts.clone();
        footer = footer.child(
            h_flex()
                .border_t_1()
                .border_color(rgba(0xffffff0b))
                .pt(px(8.))
                .gap(px(4.))
                .child(
                    DesignButton::new("home-account")
                        .aria_label(account_name.clone())
                        .flex_1()
                        .h(px(47.))
                        .child(
                            h_flex().gap(px(8.)).child(head).child(
                                v_flex().items_start().child(account_name).child(
                                    div().text_size(px(10.)).text_color(rgb(theme::INK_DISABLED)).child(
                                        selected_account
                                            .as_ref()
                                            .map(|a| if a.offline { "Sin conexión" } else { "Microsoft" })
                                            .unwrap_or("Añadir cuenta"),
                                    ),
                                ),
                            ),
                        )
                        .on_click(cx.listener(|this, _, _, c| {
                            this.home_account_menu = !this.home_account_menu;
                            c.notify();
                        })),
                )
                .child(
                    DesignButton::new("home-settings")
                        .aria_label("Ajustes del launcher")
                        .child(gpui_component::Icon::new(PandoraIcon::Settings).size(px(16.)))
                        .w(px(32.))
                        .px_0()
                        .on_click(cx.listener(|this, _, w, c| this.switch_page(PageType::LauncherSettings, &[], w, c))),
                ),
        );
        let sidebar = v_flex()
            .window_control_area(WindowControlArea::Drag)
            .w(px(220.))
            .min_w(px(220.))
            .h_full()
            .bg(rgba(0x060607e6))
            .px(px(10.))
            .pb(px(12.))
            .child(
                div()
                    .h(px(80.))
                    .px(px(8.))
                    .pt(px(16.))
                    .window_control_area(WindowControlArea::Drag)
                    .child(
                        h_flex()
                            .w_full()
                            .h(px(40.))
                            .items_center()
                            .justify_between()
                            .child(
                                img(ImageSource::Resource(Resource::Embedded(
                                    "images/wachiland-launcher-ancho.svg".into(),
                                )))
                                .w(px(140.))
                                .h(px(40.))
                                .object_fit(ObjectFit::Contain),
                            )
                            .when(crate::launcher_updates::available(cx), |row| {
                                row.child(crate::launcher_updates::indicator(cx))
                            }),
                    ),
            )
            .child(div().flex_1().min_h_0().overflow_y_scrollbar().child(instances))
            .child(footer);
        let mut account_menu = design::notched_panel(0x0a0a0cf2)
            .flex()
            .flex_col()
            .absolute()
            .left(px(10.))
            .bottom(px(64.))
            .w(px(240.))
            .p(px(6.));
        for (ix, account) in saved_accounts.iter().enumerate() {
            let uuid = account.uuid;
            let active = selected_account.as_ref().is_some_and(|a| a.uuid == uuid);
            let name = account.username(InterfaceConfig::get(cx).hide_usernames);
            account_menu = account_menu.child(
                DesignButton::new(("account-choice", ix))
                    .label_left(format!("{name}{}", if active { "  ✓" } else { "" }))
                    .w_full()
                    .justify_start()
                    .when(active, |d| d.border_l_2().border_color(rgb(theme::ACCENT_TEXT)))
                    .on_click(cx.listener(move |this, _, _, c| {
                        this.data.backend_handle.send(bridge::message::MessageToBackend::SelectAccount { uuid });
                        this.home_account_menu = false;
                        c.notify();
                    })),
            );
        }
        account_menu = account_menu
            .child(
                DesignButton::new("add-microsoft-home")
                    .label_left("Añadir cuenta de Microsoft")
                    .w_full()
                    .on_click(cx.listener(|this, _, w, c| {
                        this.home_account_menu = false;
                        crate::root::start_new_account_login(&this.data.backend_handle, w, c);
                    })),
            )
            .child(
                DesignButton::new("add-offline-home")
                    .label_left("Añadir cuenta sin conexión")
                    .w_full()
                    .on_click(cx.listener(|this, _, w, c| {
                        this.home_account_menu = false;
                        crate::modals::accounts::open_offline_account(this.data.backend_handle.clone(), w, c);
                    })),
            );
        if let Some(account) = selected_account {
            let uuid = account.uuid;
            account_menu = account_menu.child(
                DesignButton::new("logout-home")
                    .label_left(format!(
                        "Cerrar sesión en {}",
                        account.username(InterfaceConfig::get(cx).hide_usernames)
                    ))
                    .w_full()
                    .text_color(rgb(theme::DANGER))
                    .on_click(cx.listener(move |this, _, _, c| {
                        this.home_account_menu = false;
                        this.data.backend_handle.send(bridge::message::MessageToBackend::DeleteAccount { uuid });
                        c.notify();
                    })),
            );
        }
        h_flex()
            .size_full()
            .relative()
            .bg(rgb(theme::BG))
            .text_color(rgb(theme::INK))
            .child(crate::background::surface(window, cx))
            .child(sidebar)
            .child(div().flex_1().min_w_0().h_full().child(self.page.clone().render(self, window, cx)))
            .when(self.home_account_menu, |d| {
                d.child(account_menu.on_mouse_down_out(cx.listener(|this, _, _, c| {
                    this.home_account_menu = false;
                    c.notify();
                })))
            })
            .into_any_element()
    }
}

pub(crate) fn browser_sources(data: &DataEntities, install_for: Option<InstanceID>, modrinth: bool, cx: &App) -> Div {
    use crate::component::design;
    let selected = install_for.and_then(|id| InstanceEntries::find_name_by_id(&data.instances, id, cx));
    let mut controls = h_flex().gap(px(8.)).items_center();
    for (index, label) in ["Modrinth", "CurseForge"].into_iter().enumerate() {
        let target = selected.clone();
        controls = controls.child(design::chip(("browser-provider", index), label, modrinth == (index == 0)).on_click(
            move |_, w, c| {
                crate::root::switch_page(
                    if index == 0 {
                        PageType::Modrinth {
                            installing_for: target.clone(),
                        }
                    } else {
                        PageType::Curseforge {
                            installing_for: target.clone(),
                        }
                    },
                    &[],
                    w,
                    c,
                )
            },
        ));
    }
    if let Some(name) = selected {
        let mut choices = v_flex().gap(px(4.));
        for (_, entry) in &data.instances.read(cx).entries {
            let target = entry.read(cx).name.clone();
            choices = choices.child(
                design::DesignButton::new(SharedString::from(format!("browser-target-{target}")))
                    .label(target.clone())
                    .w_full()
                    .on_click(move |_, w, c| {
                        crate::root::switch_page(
                            if modrinth {
                                PageType::Modrinth {
                                    installing_for: Some(target.clone()),
                                }
                            } else {
                                PageType::Curseforge {
                                    installing_for: Some(target.clone()),
                                }
                            },
                            &[],
                            w,
                            c,
                        )
                    }),
            );
        }
        controls = controls.child(
            gpui_component::popover::Popover::new("browser-targets")
                .trigger(
                    design::DesignButton::new("browser-target-trigger")
                        .label(format!("Para: {name}"))
                        .child(PandoraIcon::ChevronDown),
                )
                .child(choices),
        );
    }
    controls
}
