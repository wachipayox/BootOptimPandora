use crate::component::design::YesNo as Switch;
use std::rc::Rc;

use bridge::{handle::BackendHandle, message::MessageToBackend};
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, h_flex,
    input::{InputEvent, InputState},
    spinner::Spinner,
    v_flex,
};
use schema::backend_config::{BackendConfig, DistributionConfig, LauncherChannel, ProxyConfig};
use schema::ignored_profile_paths::IgnoredProfilePaths;

use crate::{component::design, entity::DataEntities, icon::PandoraIcon, interface_config::InterfaceConfig};

mod appearance;
mod distribution;
mod general;
mod ignored_paths;
mod network;
#[cfg(windows)]
mod windows_security;

pub(crate) struct SettingsRoot {
    settings: Settings,
    search_state: Entity<InputState>,
    ignored_path_input: Entity<InputState>,
    ignored_filter: Entity<InputState>,
    ignored_path_error: Option<String>,
    group_list_state: ListState,
    backend_handle: BackendHandle,
    get_configuration_task: Option<Task<()>>,
    actual_backend_config: Option<BackendConfig>,
    temp_backend_config: Option<BackendConfig>,
    on_receive_backend_config: OnReceiveBackendConfig,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OnReceiveBackendConfig {
    DoNothing,
    ClearTemp,
    RequestAgainThenClearTemp,
}

impl SettingsRoot {
    fn backend_config(&self) -> Option<&BackendConfig> {
        self.temp_backend_config.as_ref().or(self.actual_backend_config.as_ref())
    }

    pub fn set_proxy_settings(&mut self, proxy_settings: ProxyConfig, cx: &mut Context<Self>) {
        if self.actual_backend_config.is_some() {
            self.on_receive_backend_config = if self.temp_backend_config.is_some() {
                OnReceiveBackendConfig::RequestAgainThenClearTemp
            } else {
                OnReceiveBackendConfig::ClearTemp
            };
            let pending = self
                .temp_backend_config
                .get_or_insert_with(|| self.actual_backend_config.clone().unwrap());
            pending.proxy = proxy_settings.clone();
        }

        self.backend_handle
            .send(MessageToBackend::SetProxyConfiguration { config: proxy_settings });

        self.update_backend_configuration(cx);
    }

    pub fn set_distribution_settings(&mut self, config: DistributionConfig, cx: &mut Context<Self>) {
        if self.actual_backend_config.is_some() {
            self.on_receive_backend_config = if self.temp_backend_config.is_some() {
                OnReceiveBackendConfig::RequestAgainThenClearTemp
            } else {
                OnReceiveBackendConfig::ClearTemp
            };
            let pending = self
                .temp_backend_config
                .get_or_insert_with(|| self.actual_backend_config.clone().unwrap());
            pending.distribution = config.clone();
        }
        self.backend_handle.send(MessageToBackend::SetDistributionConfiguration { config });
        self.update_backend_configuration(cx);
    }

    pub fn set_launcher_channel(&mut self, channel: LauncherChannel, cx: &mut Context<Self>) {
        if self.actual_backend_config.is_some() {
            self.on_receive_backend_config = if self.temp_backend_config.is_some() {
                OnReceiveBackendConfig::RequestAgainThenClearTemp
            } else {
                OnReceiveBackendConfig::ClearTemp
            };
            let pending = self
                .temp_backend_config
                .get_or_insert_with(|| self.actual_backend_config.clone().unwrap());
            pending.channel = channel;
        }
        self.backend_handle.send(MessageToBackend::SetLauncherChannel { channel });
        self.update_backend_configuration(cx);
    }

    fn set_ignored_profile_paths(&mut self, paths: Vec<String>, cx: &mut Context<Self>) -> bool {
        let paths = match IgnoredProfilePaths::normalized(paths) {
            Ok(paths) => paths,
            Err(error) => {
                log::warn!("Ignored path validation failed: {error}");
                self.ignored_path_error = Some("No se ha podido ignorar la ruta.".into());
                cx.notify();
                return false;
            },
        };
        self.ignored_path_error = None;
        if self.actual_backend_config.is_some() {
            self.on_receive_backend_config = if self.temp_backend_config.is_some() {
                OnReceiveBackendConfig::RequestAgainThenClearTemp
            } else {
                OnReceiveBackendConfig::ClearTemp
            };
            let pending = self
                .temp_backend_config
                .get_or_insert_with(|| self.actual_backend_config.clone().unwrap());
            pending.ignored_profile_paths = paths.clone();
        }
        self.backend_handle.send(MessageToBackend::SetIgnoredProfilePaths { paths: paths.0 });
        self.update_backend_configuration(cx);
        cx.notify();
        true
    }

    pub fn update_backend_configuration(&mut self, cx: &mut Context<Self>) {
        if self.get_configuration_task.is_some() {
            return;
        }

        let (send, recv) = tokio::sync::oneshot::channel();
        let settings_entity = cx.entity();
        self.get_configuration_task = Some(cx.spawn(async move |_, cx| {
            let config: BackendConfig = recv.await.unwrap_or_default();
            // A settings window can be closed while the backend request is in flight. Updating
            // the captured weak entity is safe in that case; updating the originating window
            // context tries to reach a native window that no longer exists.
            _ = cx.update_entity(&settings_entity, move |settings, cx| {
                settings.actual_backend_config = Some(config);
                settings.get_configuration_task = None;

                match settings.on_receive_backend_config {
                    OnReceiveBackendConfig::DoNothing => {},
                    OnReceiveBackendConfig::ClearTemp => {
                        settings.temp_backend_config = None;
                        settings.on_receive_backend_config = OnReceiveBackendConfig::DoNothing;
                    },
                    OnReceiveBackendConfig::RequestAgainThenClearTemp => {
                        settings.update_backend_configuration(cx);
                        settings.on_receive_backend_config = OnReceiveBackendConfig::ClearTemp;
                    },
                }

                cx.notify();
            });
        }));

        self.backend_handle.send(MessageToBackend::GetBackendConfiguration { channel: send });
    }
}

impl Render for SettingsRoot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = design::page_header("LAUNCHER", window);
        let mut tabs = h_flex().w_full().flex_shrink_0().px(px(20.)).border_b_1().border_color(rgba(0xffffff0b));
        for index in 0..self.settings.pages.len() {
            if self.settings.searched_pages.as_ref().is_some_and(|pages| !pages.contains(&index)) {
                continue;
            }
            tabs = tabs.child(
                design::tab(
                    ("settings-tab", index),
                    (self.settings.pages[index].title)(),
                    self.settings.selected_page == Some(index),
                )
                .on_click(cx.listener(move |root, _, _, cx| {
                    root.settings.selected_page = Some(index);
                    root.settings.selected_group = None;
                    root.group_list_state.scroll_to_reveal_item(0);
                    cx.notify();
                })),
            );
        }
        v_flex()
            .size_full()
            .child(header)
            .child(tabs)
            .child(
                h_flex()
                    .w_full()
                    .px(px(24.))
                    .pt(px(18.))
                    .child(div().flex_1())
                    .child(design::text_field(&self.search_state).prefix(PandoraIcon::Search).w(px(260.))),
            )
            .child(div().flex_1().min_h_0().px(px(12.)).py(px(12.)).child(self.render_page(cx)))
    }
}

impl SettingsRoot {
    pub fn render_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(page_ix) = self.settings.selected_page else {
            return div().size_full().p_3().text_lg().child("No hay resultados.").into_any();
        };

        let page = self.settings.pages[page_ix].clone();

        let two_columns = page_ix == 0 && self.settings.last_query.is_none();
        let group_count = if two_columns {
            2
        } else {
            page.searched_groups.as_ref().map(|g| g.len()).unwrap_or(page.groups.len())
        };
        if self.group_list_state.item_count() != group_count {
            self.group_list_state.reset(group_count);
        }
        if self.settings.deferred_scroll_to_group {
            self.settings.deferred_scroll_to_group = false;
            if let Some(group) = self.settings.selected_group
                && group < group_count
            {
                self.group_list_state.scroll_to_reveal_item(group);
            }
        }
        let selected_group = self.settings.selected_group;

        let entity = cx.entity();
        let render_group = move |mut group_ix: usize, window: &mut Window, cx: &mut App| {
            cx.update_entity(&entity, |root, cx| {
                if let Some(searched) = &page.searched_groups {
                    group_ix = searched[group_ix];
                }
                let group = &page.groups[group_ix];

                let (items, mut item_elements) = if let Some(searched) = &group.searched_items {
                    (itertools::Either::Left(searched.iter().copied()), Vec::with_capacity(searched.len()))
                } else {
                    (itertools::Either::Right(0..group.items.len()), Vec::with_capacity(group.items.len()))
                };

                let mut has_backend_missing = false;

                for item_ix in items {
                    let item = &group.items[item_ix];
                    let wide =
                        matches!(&item.widget, SettingItemWidget::BackendWide(_) | SettingItemWidget::AnyWide(_));

                    let widget = match &item.widget {
                        SettingItemWidget::None => {
                            continue;
                        },
                        SettingItemWidget::Switch(get, set) => {
                            let value = (get)(InterfaceConfig::get(cx));
                            let set = *set;
                            Switch::new(("switch-element", page_ix * 10000 + group_ix * 100 + item_ix))
                                .checked(value)
                                .on_click(move |value, _, cx| {
                                    (set)(InterfaceConfig::get_mut(cx), *value);
                                    cx.refresh_windows();
                                })
                                .into_any_element()
                        },
                        SettingItemWidget::Integer { get, set, min, max } => {
                            let mut initialized = false;
                            let state = window.use_keyed_state(
                                ("integer-element", page_ix * 10000 + group_ix * 100 + item_ix),
                                cx,
                                |window, cx| {
                                    initialized = true;
                                    let mut state = InputState::new(window, cx);
                                    if let Some(min) = min {
                                        state = state.min(*min as f64);
                                    }
                                    if let Some(max) = max {
                                        state = state.max(*max as f64);
                                    }
                                    let value = (get)(cx);
                                    state.set_value(format!("{}", value), window, cx);
                                    state
                                },
                            );
                            if initialized {
                                let get = *get;
                                let set = *set;
                                let min = *min;
                                let max = *max;
                                cx.subscribe_in(&state, window, move |_, entity, event: &InputEvent, window, cx| {
                                    if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                                        return;
                                    }

                                    let value = entity.read(cx).value();

                                    if let Ok(val) = value.parse::<f64>() {
                                        let mut val = val.round() as i32;
                                        if let Some(min) = min {
                                            val = val.max(min);
                                        }
                                        if let Some(max) = max {
                                            val = val.min(max);
                                        }
                                        (set)(val, cx);
                                    }

                                    let new_value = format!("{}", (get)(cx));
                                    if new_value != value {
                                        entity.update(cx, |input, cx| {
                                            input.set_value(new_value, window, cx);
                                        });
                                    }
                                })
                                .detach();
                            }
                            gpui_component::input::NumberInput::new(&state).w(px(200.0)).into_any_element()
                        },
                        SettingItemWidget::Backend(func) => {
                            let Some(backend_config) = root.backend_config() else {
                                has_backend_missing = true;
                                continue;
                            };

                            (func)(backend_config, window, cx)
                        },
                        SettingItemWidget::BackendWide(func) => {
                            let Some(backend_config) = root.backend_config() else {
                                has_backend_missing = true;
                                continue;
                            };
                            (func)(backend_config, root, window, cx)
                        },
                        SettingItemWidget::Any(func) | SettingItemWidget::AnyWide(func) => (func)(window, cx),
                    };

                    if wide {
                        item_elements.push(v_flex().w_full().child(widget));
                        continue;
                    }
                    let item_element = h_flex()
                        .justify_between()
                        .items_center()
                        .py(px(8.))
                        .gap(px(20.))
                        .w_full()
                        .child(
                            v_flex()
                                .text_sm()
                                .flex_1()
                                .max_w_3_5()
                                .child((item.title)())
                                .child(div().text_color(cx.theme().muted_foreground).child((item.description)())),
                        )
                        .child(h_flex().max_w_2_5().child(widget));

                    item_elements.push(item_element);
                }

                let items = v_flex()
                    .w_full()
                    .px(px(18.))
                    .py(px(16.))
                    .gap(px(10.))
                    .bg(rgb(crate::theme::PANEL))
                    .rounded(px(0.))
                    .line_height(rems(1.0))
                    .when(has_backend_missing, |this| this.child(h_flex().gap_1().child(Spinner::new())))
                    .children(item_elements);

                let mut panel =
                    v_flex().w_full().bg(rgb(crate::theme::PANEL)).border_1().border_color(rgba(0xffffff0b));
                if let Some(title) = group.title {
                    panel = panel.child(
                        div()
                            .px(px(18.))
                            .pt(px(16.))
                            .font_family(crate::theme::DISPLAY)
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(crate::theme::INK))
                            .when(selected_group == Some(group_ix), Styled::underline)
                            .child((title)()),
                    );
                }
                v_flex().w_full().px_3().py_1p5().child(panel.child(items)).into_any_element()
            })
        };
        list(self.group_list_state.clone(), move |row, window, cx| {
            if two_columns {
                let left = render_group(row, window, cx);
                let right = render_group(row + 2, window, cx);
                h_flex()
                    .w_full()
                    .items_start()
                    .gap(px(12.))
                    .child(div().flex_1().min_w_0().child(left))
                    .child(div().flex_1().min_w_0().child(right))
                    .into_any_element()
            } else {
                render_group(row, window, cx)
            }
        })
        .size_full()
        .into_any_element()
    }

    pub fn on_search(&mut self, entity: Entity<InputState>, event: &InputEvent, cx: &mut Context<Self>) {
        if let InputEvent::Change = event {
            self.settings.search(&*entity.read(cx).value());
        }
    }
}

#[derive(Default, Clone)]
enum SettingItemWidget {
    #[default]
    None,
    Switch(fn(&InterfaceConfig) -> bool, fn(&mut InterfaceConfig, bool)),
    Integer {
        get: fn(&App) -> i32,
        set: fn(i32, &mut App),
        min: Option<i32>,
        max: Option<i32>,
    },
    Backend(Rc<dyn Fn(&BackendConfig, &mut Window, &mut Context<SettingsRoot>) -> AnyElement>),
    BackendWide(Rc<dyn Fn(&BackendConfig, &SettingsRoot, &mut Window, &mut Context<SettingsRoot>) -> AnyElement>),
    Any(Rc<dyn Fn(&mut Window, &mut Context<SettingsRoot>) -> AnyElement>),
    AnyWide(Rc<dyn Fn(&mut Window, &mut Context<SettingsRoot>) -> AnyElement>),
}

struct SettingItem {
    title: fn() -> &'static str,
    description: fn() -> &'static str,
    widget: SettingItemWidget,
    casefolded_title: Option<String>,
    casefolded_description: Option<String>,
}

impl Clone for SettingItem {
    fn clone(&self) -> Self {
        Self {
            title: self.title.clone(),
            description: self.description.clone(),
            widget: self.widget.clone(),
            casefolded_title: None,
            casefolded_description: None,
        }
    }
}

impl Default for SettingItem {
    fn default() -> Self {
        Self {
            title: || "",
            description: || "",
            widget: SettingItemWidget::default(),
            casefolded_title: None,
            casefolded_description: None,
        }
    }
}

impl SettingItem {
    pub fn matches_search(&self, query: &str) -> bool {
        if let Some(casefolded_title) = &self.casefolded_title
            && casefolded_title.contains(query)
        {
            return true;
        }
        if let Some(casefolded_description) = &self.casefolded_description
            && casefolded_description.contains(query)
        {
            return true;
        }
        false
    }
}

#[derive(Clone)]
struct SettingGroup {
    title: Option<fn() -> &'static str>,
    items: Rc<[SettingItem]>,
    searched_items: Option<Rc<[usize]>>,
}

impl SettingGroup {
    pub fn reset_search(&mut self) {
        self.searched_items = None;
    }

    pub fn refine_search(&mut self, query: &str) -> bool {
        debug_assert!(!query.is_empty());

        let Some(previous) = &self.searched_items else {
            return self.search(query);
        };

        let mut searched = previous.to_vec();
        searched.retain(|index| self.items[*index].matches_search(query));
        let any_matches = !searched.is_empty();
        self.searched_items = Some(searched.into());
        any_matches
    }

    pub fn search(&mut self, query: &str) -> bool {
        if query.is_empty() {
            self.searched_items = None;
            return true;
        }

        let mut searched = Vec::new();
        for (index, group) in self.items.iter().enumerate() {
            if group.matches_search(query) {
                searched.push(index);
            }
        }
        let any_matches = !searched.is_empty();
        if searched.len() == self.items.len() {
            self.searched_items = None;
        } else {
            self.searched_items = Some(searched.into());
        }
        any_matches
    }
}

#[derive(Clone)]
struct SettingPage {
    title: fn() -> &'static str,
    groups: Rc<[SettingGroup]>,
    searched_groups: Option<Rc<[usize]>>,
}

impl SettingPage {
    pub fn reset_search(&mut self) {
        let groups = Rc::make_mut(&mut self.groups);
        for group in groups {
            group.reset_search();
        }
        self.searched_groups = None;
    }

    pub fn refine_search(&mut self, query: &str) -> bool {
        debug_assert!(!query.is_empty());

        let Some(previous) = &self.searched_groups else {
            return self.search(query);
        };

        let groups = Rc::make_mut(&mut self.groups);

        let mut searched = previous.to_vec();
        searched.retain(|index| groups[*index].refine_search(query));
        let any_matches = !searched.is_empty();
        self.searched_groups = Some(searched.into());
        any_matches
    }

    pub fn search(&mut self, query: &str) -> bool {
        debug_assert!(!query.is_empty());

        let groups = Rc::make_mut(&mut self.groups);

        let mut searched = Vec::new();
        for (index, group) in groups.iter_mut().enumerate() {
            if group.search(query) {
                searched.push(index);
            }
        }
        let any_matches = !searched.is_empty();
        if searched.len() == self.groups.len() {
            self.searched_groups = None;
        } else {
            self.searched_groups = Some(searched.into());
        }
        any_matches
    }
}

struct Settings {
    pages: Box<[SettingPage]>,
    selected_page: Option<usize>,
    selected_group: Option<usize>,
    deferred_scroll_to_group: bool,
    searched_pages: Option<Vec<usize>>,
    last_language_id: Option<u8>,
    last_query: Option<String>,
}

impl Settings {
    pub fn search(&mut self, query: &str) {
        self.selected_group = None;
        self.deferred_scroll_to_group = false;

        if query.is_empty() {
            for page in &mut self.pages {
                page.reset_search();
            }
            self.last_query = None;
            self.searched_pages = None;
            if self.selected_page.is_none() {
                self.selected_page = Some(0);
            }
            return;
        }

        let lang_id = t::get_current_lang_id();

        if self.last_language_id != Some(lang_id) {
            self.last_language_id = Some(lang_id);

            for page in &mut self.pages {
                let groups = Rc::make_mut(&mut page.groups);
                for group in groups {
                    let items = Rc::make_mut(&mut group.items);
                    for item in items {
                        item.casefolded_title = Some(casefold::simple_fold((item.title)().to_string()));
                        item.casefolded_description = Some(casefold::simple_fold((item.description)().to_string()));
                    }
                }
            }
        }

        let query = casefold::simple_fold(query.to_string());

        let searched = if let Some(searched) = &mut self.searched_pages
            && let Some(last_query) = &self.last_query
            && query.contains(last_query)
        {
            searched.retain(|index| self.pages[*index].refine_search(&*query));
            searched
        } else {
            let searched = self.searched_pages.get_or_insert_default();
            searched.clear();

            for (index, page) in self.pages.iter_mut().enumerate() {
                if page.search(&*query) {
                    searched.push(index);
                }
            }
            searched
        };

        // Update selected page
        if searched.is_empty() {
            self.selected_page = None;
        } else if let Some(old_selected) = self.selected_page {
            if !searched.contains(&old_selected) {
                self.selected_page = Some(searched.first().copied().unwrap());
            }
        } else {
            self.selected_page = Some(searched.first().copied().unwrap());
        }

        // Set last query
        self.last_query = Some(query);
    }
}

pub fn open_settings_window(window: &Window, _data: &DataEntities, cx: &mut App) {
    // Native callbacks receive a shared window; switch on the next frame with its mutable context.
    let handle = window.window_handle();
    cx.defer(move |cx| {
        _ = handle.update(cx, |_, window, cx| {
            crate::root::switch_page(crate::ui::PageType::LauncherSettings, &[], window, cx)
        });
    });
}

pub(crate) fn create(data: &DataEntities, window: &mut Window, cx: &mut App) -> Entity<SettingsRoot> {
    cx.new(|cx| {
        let search_state = cx.new(|cx| InputState::new(window, cx).placeholder("Buscar en ajustes..."));
        let ignored_path_input = cx.new(|cx| InputState::new(window, cx).placeholder("/mods/.connector"));
        let ignored_filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filtrar"));
        cx.subscribe(&ignored_filter, |_: &mut SettingsRoot, _, _: &InputEvent, cx| cx.notify())
            .detach();

        cx.subscribe(&search_state, SettingsRoot::on_search).detach();
        cx.subscribe_in(
            &ignored_path_input,
            window,
            |root: &mut SettingsRoot, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    root.add_ignored_path(window, cx);
                }
            },
        )
        .detach();

        let mut root = SettingsRoot {
            settings: create_settings(data, window, cx),
            search_state,
            ignored_path_input,
            ignored_filter,
            ignored_path_error: None,
            group_list_state: ListState::new(0, ListAlignment::Top, px(100.)),
            backend_handle: data.backend_handle.clone(),
            get_configuration_task: None,
            actual_backend_config: None,
            temp_backend_config: None,
            on_receive_backend_config: OnReceiveBackendConfig::DoNothing,
        };

        root.update_backend_configuration(cx);

        root
    })
}

fn create_settings(data: &DataEntities, window: &mut Window, cx: &mut App) -> Settings {
    let mut general = general::create_page(window, cx);
    let mut appearance = appearance::create_page(data, window, cx);
    let mut language = general.groups[0].items[0].clone();
    language.title = || "";
    language.description = || "";
    if let SettingItemWidget::Any(widget) = language.widget {
        language.widget = SettingItemWidget::AnyWide(widget);
    }
    let mut first_group = general.groups[0].clone();
    let mut items = first_group.items.to_vec();
    items.push(SettingItem {
        widget: SettingItemWidget::AnyWide(Rc::new(|_, _| {
            design::DesignButton::new("open-synchronization")
                .label("Sincronización")
                .on_click(|_, w, c| {
                    crate::root::switch_page(
                        crate::ui::PageType::Syncing,
                        &[crate::ui::PageType::LauncherSettings],
                        w,
                        c,
                    )
                })
                .into_any_element()
        })),
        ..Default::default()
    });
    first_group.items = items.into();
    let groups = Rc::make_mut(&mut general.groups);
    groups[0] = first_group;

    let mut general_groups = general.groups.to_vec();
    let mut theme_group = appearance.groups[2].clone();
    let mut theme_items = vec![appearance.groups[0].items[0].clone()];
    theme_items.extend(theme_group.items.iter().cloned());
    theme_group.items = theme_items.into();
    general_groups.push(theme_group);
    general_groups.push(appearance.groups[3].clone());
    general.groups = general_groups.into();
    let mut appearance_groups = appearance.groups[..2].to_vec();
    appearance_groups.push(SettingGroup {
        title: Some(|| "Idioma"),
        items: vec![language].into(),
        searched_items: None,
    });
    appearance.groups = appearance_groups.into();
    let mut pages = vec![
        general,
        appearance,
        network::create_page(),
        distribution::create_page(),
        ignored_paths::create_page(),
    ];
    #[cfg(windows)]
    pages.push(windows_security::create_page());

    pages.push(SettingPage {
        title: || "Acerca de",
        groups: vec![SettingGroup {
            title: Some(|| "Acerca de"),
            items: vec![SettingItem {
                title: || "Versión",
                widget: SettingItemWidget::Any(Rc::new(|_, _| {
                    let version = option_env!("PANDORA_RELEASE_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"));
                    let revision = option_env!("GIT_REVISION");
                    div()
                        .text_size(px(13.))
                        .child(revision.map(|sha| format!("{version} ({sha})")).unwrap_or_else(|| version.into()))
                        .into_any_element()
                })),
                ..Default::default()
            }]
            .into(),
            searched_items: None,
        }]
        .into(),
        searched_groups: None,
    });
    let capture_page = std::env::var("WBOMBO_UI_CAPTURE_PAGE").unwrap_or_default();
    let selected_page = match capture_page.as_str() {
        "appearance" | "launcher-appearance" => 1,
        "launcher-network" => 2,
        "launcher-distribution" => 3,
        "launcher-ignored" => 4,
        "launcher-windows" => 5.min(pages.len() - 1),
        "launcher-about" => pages.len() - 1,
        _ => 0,
    };
    Settings {
        pages: pages.into_boxed_slice(),
        selected_page: Some(selected_page),
        selected_group: None,
        deferred_scroll_to_group: false,
        searched_pages: None,
        last_language_id: None,
        last_query: None,
    }
}
