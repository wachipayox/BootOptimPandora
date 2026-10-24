use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use bridge::{
    handle::BackendHandle,
    install::{ContentDownload, ContentInstall, ContentInstallFile, InstallTarget},
    instance::{ContentFolder, InstanceContentSummary, InstanceID},
};
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _, WindowExt,
    button::{Button, ButtonVariants},
    h_flex,
    input::{InputEvent, InputState, SelectAll},
    list::{List, ListState},
    menu::{DropdownMenu, PopupMenuItem},
    notification::{Notification, NotificationType},
    v_flex,
};
use parking_lot::Mutex;
use rustc_hash::FxHashSet;
use schema::{
    content::{ContentInstallReason, ContentSource},
    curseforge::CurseforgeClassId,
    loader::Loader,
    modrinth::ModrinthProjectType,
};
use ustr::Ustr;

use crate::{
    component::{
        content_list::ContentListDelegate,
        design::{self, DesignButton},
    },
    entity::{
        DataEntities,
        instance::{ContentStates, InstanceEntry},
    },
    interface_config::{InstanceContentSortKey, InterfaceConfig, PreferredAddContentSource},
    root, theme,
    ui::PageType,
};

pub struct InstanceContentSubpage {
    content_type: ContentType,
    instance: InstanceID,
    instance_loader: Loader,
    instance_version: Ustr,
    instance_name: SharedString,
    backend_handle: BackendHandle,
    content_states: ContentStates,
    content_list: Entity<ListState<ContentListDelegate>>,
    updating: Arc<Mutex<FxHashSet<u64>>>,
    content: Entity<Option<Arc<[InstanceContentSummary]>>>,
    search: Entity<InputState>,

    needs_update_check: bool,
    update_count: usize,

    _add_from_file_task: Option<Task<()>>,
}

#[derive(Clone, Copy)]
pub enum ContentType {
    Mods,
    ResourcePacks,
    Shaders,
}

impl ContentType {
    fn content_folder(self) -> ContentFolder {
        match self {
            ContentType::Mods => ContentFolder::Mods,
            ContentType::ResourcePacks => ContentFolder::ResourcePacks,
            ContentType::Shaders => ContentFolder::Shaders,
        }
    }

    fn install_select(self) -> &'static str {
        match self {
            ContentType::Mods => t::instance::content::install::select_mods(),
            ContentType::ResourcePacks => t::instance::content::install::select_resourcepacks(),
            ContentType::Shaders => t::instance::content::install::select_shaders(),
        }
    }

    fn modrinth_project_type(self) -> ModrinthProjectType {
        match self {
            ContentType::Mods => ModrinthProjectType::Mod,
            ContentType::ResourcePacks => ModrinthProjectType::Resourcepack,
            ContentType::Shaders => ModrinthProjectType::Shader,
        }
    }

    fn curseforge_class_id(self) -> CurseforgeClassId {
        match self {
            ContentType::Mods => CurseforgeClassId::Mod,
            ContentType::ResourcePacks => CurseforgeClassId::Resourcepack,
            ContentType::Shaders => CurseforgeClassId::Shader,
        }
    }

    fn valid_sort_modes(self) -> &'static [InstanceContentSortKey] {
        match self {
            ContentType::Mods => &[
                InstanceContentSortKey::Name,
                InstanceContentSortKey::ModId,
                InstanceContentSortKey::Filename,
                InstanceContentSortKey::ModifiedTime,
                InstanceContentSortKey::FileSize,
            ],
            ContentType::ResourcePacks | ContentType::Shaders => &[
                InstanceContentSortKey::Filename,
                InstanceContentSortKey::ModifiedTime,
                InstanceContentSortKey::FileSize,
            ],
        }
    }

    fn sort_key(self, config: &InterfaceConfig) -> InstanceContentSortKey {
        match self {
            ContentType::Mods => config.instance_mods_sort_key,
            ContentType::ResourcePacks => config.instance_resourcepacks_sort_key,
            ContentType::Shaders => config.instance_shaders_sort_key,
        }
    }

    fn sort_enabled_first(self, config: &InterfaceConfig) -> bool {
        match self {
            ContentType::Mods => config.instance_mods_sort_enabled_first,
            ContentType::ResourcePacks => config.instance_resourcepacks_sort_enabled_first,
            ContentType::Shaders => config.instance_shaders_sort_enabled_first,
        }
    }

    fn set_sort_key(self, config: &mut InterfaceConfig, value: InstanceContentSortKey) {
        match self {
            ContentType::Mods => config.instance_mods_sort_key = value,
            ContentType::ResourcePacks => config.instance_resourcepacks_sort_key = value,
            ContentType::Shaders => config.instance_shaders_sort_key = value,
        }
    }

    fn set_sort_enabled_first(self, config: &mut InterfaceConfig, value: bool) {
        match self {
            ContentType::Mods => config.instance_mods_sort_enabled_first = value,
            ContentType::ResourcePacks => config.instance_resourcepacks_sort_enabled_first = value,
            ContentType::Shaders => config.instance_shaders_sort_enabled_first = value,
        }
    }
}

impl InstanceContentSubpage {
    pub fn new(
        instance: &Entity<InstanceEntry>,
        content_type: ContentType,
        data: &DataEntities,
        backend_handle: BackendHandle,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> Self {
        let instance = instance.read(cx);
        let instance_loader = instance.configuration.loader;
        let instance_version = instance.configuration.minecraft_version;
        let instance_id = instance.id;
        let instance_name = instance.name.clone();
        let content_states = instance.content_states.clone();

        let content_folder = content_type.content_folder();
        let content = instance.content[content_folder].clone();

        let config = InterfaceConfig::get(cx);
        let mut sort_key = content_type.sort_key(config);
        let enabled_first = content_type.sort_enabled_first(config);

        let valid_sort_modes = content_type.valid_sort_modes();
        if !valid_sort_modes.contains(&sort_key) {
            sort_key = valid_sort_modes[0];
        }

        let updating = Arc::new(Mutex::new(FxHashSet::default()));
        let mut content_list_delegate = ContentListDelegate::new(
            instance_id,
            data,
            content_type.content_folder(),
            instance_loader,
            instance_version,
            sort_key,
            enabled_first,
            updating.clone(),
        );

        let (needs_update_check, update_count) = if let Some(new_content) = content.read(cx) {
            content_list_delegate.set_content(new_content);
            calculate_update_count(instance_loader, instance_version, new_content)
        } else {
            (false, 0)
        };

        let content_list =
            cx.new(|cx| ListState::new(content_list_delegate, window, cx).selectable(false).searchable(false));

        cx.observe(&content, |page, content, cx| {
            if let Some(new_content) = content.read(cx) {
                let (needs_update_check, update_count) =
                    calculate_update_count(page.instance_loader, page.instance_version, new_content);
                page.needs_update_check = needs_update_check;
                page.update_count = update_count;
            }

            page.content_list.update(cx, |list, cx| {
                if let Some(new_content) = content.read(cx) {
                    list.delegate_mut().set_content(new_content);
                    log::info!("instance-content-ready pid={}", std::process::id());
                }
                cx.notify();
            });

            cx.notify();
        })
        .detach();

        let search = cx.new(|c| {
            InputState::new(window, c).placeholder(match content_type {
                ContentType::Mods => "Buscar mods...",
                ContentType::ResourcePacks => "Buscar paquetes de recursos...",
                ContentType::Shaders => "Buscar shaders...",
            })
        });
        cx.subscribe_in(&search, window, {
            let list = content_list.clone();
            move |_, field, event: &InputEvent, w, c| {
                if matches!(event, InputEvent::Change) {
                    let query = field.read(c).value();
                    list.update(c, |list, c| list.set_query(&query, w, c));
                }
            }
        })
        .detach();
        let (send, receive) = tokio::sync::oneshot::channel();
        backend_handle.send(bridge::message::MessageToBackend::GetInstanceProfileInfo {
            id: instance_id,
            channel: send,
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(info)) = receive.await {
                let _ = this.update(cx, |this, c| {
                    this.content_list.update(c, |list, c| {
                        list.delegate_mut().set_profile_info(info);
                        log::info!("instance-content-origin-ready pid={}", std::process::id());
                        c.notify();
                    });
                    c.notify();
                });
            }
        })
        .detach();

        Self {
            content_type,
            instance: instance_id,
            instance_loader,
            instance_version,
            instance_name,
            backend_handle,
            content_states,
            content_list,
            updating,
            content,
            search,
            needs_update_check,
            update_count,
            _add_from_file_task: None,
        }
    }

    fn install_paths(&self, paths: &[PathBuf], window: &mut Window, cx: &mut App) {
        let content_folder = self.content_type.content_folder().folder_name();

        let content_install = ContentInstall {
            target: InstallTarget::Instance(self.instance),
            loader: self.instance_loader,
            minecraft_version: self.instance_version,
            files: paths
                .into_iter()
                .filter_map(|path| {
                    Some(ContentInstallFile {
                        replace_old: None,
                        path: bridge::install::ContentInstallPath::Raw(
                            Path::new(content_folder).join(path.file_name()?).into(),
                        ),
                        download: ContentDownload::File { path: path.clone() },
                        content_source: ContentSource::Manual,
                        reason: ContentInstallReason::Standalone,
                    })
                })
                .collect(),
        };
        crate::root::start_install(content_install, &self.backend_handle, window, cx);
    }
}

fn calculate_update_count(loader: Loader, version: Ustr, content: &Arc<[InstanceContentSummary]>) -> (bool, usize) {
    let mut update_count = 0;

    for content in content.iter() {
        let status = content.update.status_if_matches(loader, version.as_str());
        match status {
            bridge::instance::ContentUpdateStatus::Unknown => {
                return (true, 0);
            },
            bridge::instance::ContentUpdateStatus::Modrinth | bridge::instance::ContentUpdateStatus::Curseforge => {
                update_count += 1;
            },
            _ => {},
        }
    }

    (false, update_count)
}

impl InstanceContentSubpage {
    fn set_sort(&mut self, key: InstanceContentSortKey, enabled_first: bool, cx: &mut Context<Self>) {
        let config = InterfaceConfig::get_mut(cx);
        self.content_type.set_sort_key(config, key);
        self.content_type.set_sort_enabled_first(config, enabled_first);
        let content = self.content.read(cx).clone();
        self.content_list.update(cx, |list, c| {
            list.delegate_mut().set_sort_options(key, enabled_first);
            if let Some(content) = &content {
                list.delegate_mut().set_content(content);
            }
            c.notify();
        });
        cx.notify();
    }
    fn update_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(content) = self.content.read(cx).clone() {
            for summary in content.iter() {
                if summary.update.can_update(self.instance_loader, self.instance_version.as_str()) {
                    root::update_single_mod(
                        self.instance,
                        summary.id,
                        summary.filename_hash,
                        &self.updating,
                        &self.backend_handle,
                        window,
                        cx,
                    );
                }
            }
            self.content_list.update(cx, |_, c| c.notify());
        }
    }
}
impl Render for InstanceContentSubpage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.content_states.observe(self.content_type.content_folder());
        let config = InterfaceConfig::get(cx);
        let key = self.content_type.sort_key(config);
        let enabled = self.content_type.sort_enabled_first(config);
        let mut tabs = h_flex();
        for (ix, label, value) in [
            (0, "Mods", super::instance_page::InstanceSubpageType::Mods),
            (1, "Paquetes de recursos", super::instance_page::InstanceSubpageType::ResourcePacks),
            (2, "Shaders", super::instance_page::InstanceSubpageType::Shaders),
        ] {
            let active = matches!(
                (self.content_type, value),
                (ContentType::Mods, super::instance_page::InstanceSubpageType::Mods)
                    | (ContentType::ResourcePacks, super::instance_page::InstanceSubpageType::ResourcePacks)
                    | (ContentType::Shaders, super::instance_page::InstanceSubpageType::Shaders)
            );
            tabs = tabs.child(design::chip(("content-kind", ix as usize), label, active).on_click(move |_, w, c| {
                InterfaceConfig::get_mut(c).instance_subpage = value;
                w.refresh();
            }));
        }
        let weak = cx.entity().downgrade();
        let valid = self.content_type.valid_sort_modes();
        let sort = Button::new("content-sort")
            .ghost()
            .w(px(180.))
            .h(px(36.))
            .border_1()
            .border_color(rgba(0xffffff33))
            .icon(crate::icon::PandoraIcon::SortAscending)
            .label(format!(
                "{} ▾",
                if enabled {
                    "Activados primero".to_owned()
                } else {
                    key.name().as_str().to_owned()
                }
            ))
            .dropdown_menu(move |mut menu, _, _| {
                let page = weak.clone();
                menu = menu
                    .item(PopupMenuItem::new("Activados primero").checked(enabled).on_click(move |_, _, c| {
                        let _ = page.update(c, |this, c| this.set_sort(key, !enabled, c));
                    }))
                    .separator();
                for &value in valid {
                    let page = weak.clone();
                    menu =
                        menu.item(PopupMenuItem::new(value.name().as_str().to_owned()).checked(key == value).on_click(
                            move |_, _, c| {
                                let _ = page.update(c, |this, c| this.set_sort(value, enabled, c));
                            },
                        ));
                }
                menu.min_w(px(200.))
            });
        let bar = h_flex()
            .w_full()
            .gap(px(12.))
            .child(tabs)
            .child(div().flex_1())
            .child(sort)
            .child(design::text_field(&self.search).w(px(260.)).h(px(36.)));
        let mut actions = h_flex().w_full().gap(px(8.));
        if let Some(content) = self.content.read(cx) {
            let noun = match self.content_type {
                ContentType::Mods => "mods",
                ContentType::ResourcePacks => "paquetes de recursos",
                ContentType::Shaders => "shaders",
            };
            let mut counts =
                format!("{} {noun} · {} activados", content.len(), content.iter().filter(|s| s.enabled).count());
            if !self.needs_update_check && self.update_count > 0 {
                counts.push_str(&format!(" · {} con actualización", self.update_count));
            }
            actions = actions.child(div().text_size(px(12.)).text_color(rgb(theme::INK_MUTED)).child(counts));
        }
        actions = actions
            .child(div().flex_1())
            .child(
                DesignButton::new("add-modrinth")
                    .label("Añadir de Modrinth")
                    .on_click(cx.listener(Self::add_from_modrinth)),
            )
            .child(
                DesignButton::new("add-curseforge")
                    .label("Añadir de CurseForge")
                    .on_click(cx.listener(Self::add_from_curseforge)),
            )
            .child(
                DesignButton::new("add-file")
                    .label("Añadir archivo")
                    .on_click(cx.listener(Self::add_from_file)),
            );
        if !self.needs_update_check && self.update_count > 0 {
            actions = actions.child(
                design::action_button(
                    "update-all",
                    format!("Actualizar todo ({})", self.update_count),
                    design::ButtonAction::Primary,
                )
                .on_click(cx.listener(|this, _, w, c| this.update_all(w, c))),
            );
        }
        let headings = h_flex()
            .h(px(36.))
            .px(px(14.))
            .gap(px(12.))
            .bg(rgb(theme::PANEL_RAISED))
            .font_family(theme::DISPLAY)
            .text_size(px(11.))
            .text_color(rgb(theme::INK_MUTED))
            .child(div().flex_1().pl(px(44.)).child("NOMBRE"))
            .child(div().w(px(140.)).text_center().child("VERSIÓN"))
            .child(div().w(px(140.)).text_center().child("ORIGEN"))
            .child(div().w(px(84.)).text_center().child("ACTIVO"))
            .child(div().w(px(30.)));
        let list = div()
            .id("content-list-area")
            .flex_1()
            .min_h_0()
            .border_1()
            .border_color(rgba(0xffffff0b))
            .bg(rgb(theme::PANEL))
            .drag_over(|style, _: &ExternalPaths, _, c| style.bg(c.theme().accent))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, w, c| this.install_paths(paths.paths(), w, c)))
            .child(
                v_flex()
                    .size_full()
                    .child(headings)
                    .child(div().flex_1().min_h_0().child(List::new(&self.content_list))),
            )
            .on_click({
                let list = self.content_list.clone();
                move |_, _, c| {
                    list.update(c, |list, c| {
                        list.delegate_mut().clear_selection();
                        c.notify();
                    });
                }
            })
            .key_context("Input")
            .on_action({
                let list = self.content_list.clone();
                move |_: &SelectAll, _, c| {
                    list.update(c, |list, c| {
                        list.delegate_mut().select_all();
                        c.notify();
                    });
                }
            });
        let backend = self.backend_handle.clone();
        let id = self.instance;
        let footer = h_flex()
            .h(px(34.))
            .w_full()
            .justify_between()
            .child(
                DesignButton::new("check-updates")
                    .label("Comprobar actualizaciones")
                    .on_click(move |_, w, c| root::start_update_check(id, &backend, w, c)),
            )
            .child(
                design::action_button(
                    "explore-content",
                    format!("Explorar contenido para {}", self.instance_name),
                    design::ButtonAction::Secondary,
                )
                .on_click(cx.listener(Self::add_from_modrinth)),
            );
        v_flex()
            .size_full()
            .px(px(24.))
            .pt(px(20.))
            .pb(px(24.))
            .gap(px(16.))
            .text_size(px(13.))
            .text_color(rgb(theme::INK))
            .child(bar)
            .child(actions)
            .child(list)
            .child(footer)
    }
}

impl InstanceContentSubpage {
    fn add_from_modrinth(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<'_, InstanceContentSubpage>) {
        let config = InterfaceConfig::get_mut(cx);
        config.modrinth_page_project_type = self.content_type.modrinth_project_type();
        config.preferred_add_content_source = PreferredAddContentSource::Modrinth;

        let path = &[
            PageType::Instances,
            PageType::InstancePage {
                name: self.instance_name.clone(),
            },
        ];
        let page = crate::ui::PageType::Modrinth {
            installing_for: Some(self.instance_name.clone()),
        };
        root::switch_page(page, path, window, cx);
    }

    fn add_from_curseforge(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<'_, InstanceContentSubpage>,
    ) {
        let config = InterfaceConfig::get_mut(cx);
        config.curseforge_page_class_id = self.content_type.curseforge_class_id();
        config.preferred_add_content_source = PreferredAddContentSource::CurseForge;

        let path = &[
            PageType::Instances,
            PageType::InstancePage {
                name: self.instance_name.clone(),
            },
        ];
        let page = crate::ui::PageType::Curseforge {
            installing_for: Some(self.instance_name.clone()),
        };
        root::switch_page(page, path, window, cx);
    }

    fn add_from_file(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<'_, InstanceContentSubpage>) {
        let config = InterfaceConfig::get_mut(cx);
        config.preferred_add_content_source = PreferredAddContentSource::File;

        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some(self.content_type.install_select().into()),
        });

        let entity = cx.entity();
        let add_from_file_task = window.spawn(cx, async move |cx| {
            let Ok(result) = receiver.await else {
                return;
            };
            _ = cx.update_window_entity(&entity, move |this, window, cx| match result {
                Ok(Some(paths)) => {
                    this.install_paths(&paths, window, cx);
                },
                Ok(None) => {},
                Err(error) => {
                    let error = format!("{}", error);
                    let notification =
                        Notification::new().autohide(false).with_type(NotificationType::Error).title(error);
                    window.push_notification(notification, cx);
                },
            });
        });
        self._add_from_file_task = Some(add_from_file_task);
    }
}
