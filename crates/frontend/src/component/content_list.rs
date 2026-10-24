use std::{
    hash::{DefaultHasher, Hash, Hasher},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use bridge::{
    handle::BackendHandle,
    instance::{
        ContentFolder, ContentSummary, ContentType, InstanceContentID, InstanceContentSummary, InstanceID,
        InstanceProfileInfo, UNKNOWN_CONTENT_SUMMARY,
    },
    message::MessageToBackend,
    modal_action::ModalAction,
};
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Disableable, IndexPath, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    list::{ListDelegate, ListItem, ListState},
    menu::{DropdownMenu, PopupMenuItem},
    spinner::Spinner,
    switch::Switch,
    v_flex,
};
use parking_lot::Mutex;
use rustc_hash::FxHashSet;
use schema::{content::ContentSource, loader::Loader, text_component::FlatTextComponent};
use strum::IntoEnumIterator;
use ustr::Ustr;

use crate::{
    component::{
        design::{DesignButton, YesNo},
        error_alert::ErrorAlert,
    },
    entity::DataEntities,
    icon::PandoraIcon,
    interface_config::{InstanceContentSortKey, InterfaceConfig},
    png_render_cache, theme,
};

#[derive(Clone)]
struct ContentEntryChild {
    summary: Arc<ContentSummary>,
    parent_filename_hash: u64,
    parent: InstanceContentID,
    path: Arc<str>,
    filesize: u64,
    lowercase_search_keys: Arc<[Arc<str>]>,
    disabled_default: bool,
    enabled: bool,
    parent_enabled: bool,
    disabled_third_party_downloads: bool,
    is_missing: bool,
}

impl ContentEntryChild {
    pub fn compare_by(&self, other: &ContentEntryChild, key: InstanceContentSortKey) -> std::cmp::Ordering {
        match key {
            InstanceContentSortKey::Name => {
                let name_a = self.summary.name.as_deref().or(self.summary.id.as_deref()).unwrap_or(&*self.path);
                let name_b = other.summary.name.as_deref().or(other.summary.id.as_deref()).unwrap_or(&*other.path);
                lexical_sort::natural_lexical_cmp(name_a, name_b)
            },
            InstanceContentSortKey::ModId => {
                let name_a = self.summary.id.as_deref().or(self.summary.name.as_deref()).unwrap_or(&*self.path);
                let name_b = other.summary.id.as_deref().or(other.summary.name.as_deref()).unwrap_or(&*other.path);
                lexical_sort::natural_lexical_cmp(name_a, name_b)
            },
            InstanceContentSortKey::Filename => {
                let name_a = &*self.path;
                let name_b = &*other.path;
                lexical_sort::natural_lexical_cmp(name_a, name_b)
            },
            InstanceContentSortKey::ModifiedTime => std::cmp::Ordering::Equal,
            InstanceContentSortKey::FileSize => self.filesize.cmp(&other.filesize).reverse(),
        }
    }
}

enum SummaryOrChild {
    Summary(InstanceContentSummary),
    Child(ContentEntryChild),
}

pub struct ContentListDelegate {
    id: InstanceID,
    folder: ContentFolder,
    profile_info: InstanceProfileInfo,
    for_loader: Loader,
    for_version: Ustr,
    backend_handle: BackendHandle,
    data: DataEntities,
    sort_key: InstanceContentSortKey,
    enabled_first: bool,
    loading: bool,
    content: Vec<InstanceContentSummary>,
    searched: Option<Vec<SummaryOrChild>>,
    children: Vec<Vec<ContentEntryChild>>,
    expanded: Arc<AtomicUsize>,
    confirming_delete: Arc<Mutex<FxHashSet<u64>>>,
    updating: Arc<Mutex<FxHashSet<u64>>>,
    last_query: SharedString,
    selected: FxHashSet<u64>,
    selected_range: FxHashSet<u64>,
    last_clicked_non_range: Option<u64>,
}

impl ContentListDelegate {
    pub fn new(
        id: InstanceID,
        data: &DataEntities,
        folder: ContentFolder,
        for_loader: Loader,
        for_version: Ustr,
        sort_key: InstanceContentSortKey,
        enabled_first: bool,
        updating: Arc<Mutex<FxHashSet<u64>>>,
    ) -> Self {
        Self {
            id,
            folder,
            profile_info: InstanceProfileInfo::default(),
            for_loader,
            for_version,
            backend_handle: data.backend_handle.clone(),
            data: data.clone(),
            sort_key,
            enabled_first,
            loading: true,
            content: Vec::new(),
            searched: None,
            children: Vec::new(),
            expanded: Arc::new(AtomicUsize::new(0)),
            confirming_delete: Default::default(),
            updating,
            last_query: SharedString::new_static(""),
            selected: FxHashSet::default(),
            selected_range: FxHashSet::default(),
            last_clicked_non_range: None,
        }
    }

    pub fn set_profile_info(&mut self, info: InstanceProfileInfo) {
        self.profile_info = info;
    }
    pub fn set_sort_options(&mut self, sort_key: InstanceContentSortKey, enabled_first: bool) {
        self.sort_key = sort_key;
        self.enabled_first = enabled_first;
    }

    pub fn render_summary(
        &self,
        summary: &InstanceContentSummary,
        selected: bool,
        expand_index: Option<usize>,
        cx: &mut Context<ListState<Self>>,
    ) -> ListItem {
        let icon = if let Some(png_icon) = summary.content_summary.png_icon.as_ref() {
            png_render_cache::render(png_icon.clone(), cx)
        } else {
            gpui::img(ImageSource::Resource(Resource::Embedded("images/default_mod.png".into())))
        };

        let id = self.id;
        let content_id = summary.id;
        let element_id = summary.filename_hash;
        let name = summary.content_summary.name.as_deref().unwrap_or(&summary.filename).to_owned();
        let relative = self
            .data
            .instances
            .read(cx)
            .entries
            .get(&id)
            .and_then(|instance| summary.path.strip_prefix(&instance.read(cx).dot_minecraft_folder).ok())
            .map(|p| p.to_string_lossy().replace('\\', "/"));
        let relative = relative.as_deref().map(|p| p.strip_suffix(".disabled").unwrap_or(p)).unwrap_or("");
        let inherited = self.profile_info.inherited_paths.contains(relative);
        let source = if self.profile_info.global_paths.contains(relative) {
            "Perfil global"
        } else {
            match summary.content_source {
                ContentSource::ModrinthProject { .. } | ContentSource::ModrinthUnknown => "Modrinth",
                ContentSource::CurseforgeProject { .. } => "CurseForge",
                _ => "local",
            }
        };
        let location = if inherited { "heredado de la instancia" } else { "local" };
        let author = if summary.content_summary.authors.is_empty() {
            location.to_owned()
        } else {
            format!("{} · {location}", summary.content_summary.authors)
        };
        let can_update = summary.update.can_update(self.for_loader, self.for_version.as_str());
        let is_loading = self.updating.lock().contains(&element_id);
        let open_version = {
            let summary = summary.clone();
            let data = self.data.clone();
            let updating = self.updating.clone();
            let list = cx.entity();
            move |_: &ClickEvent, w: &mut Window, c: &mut App| {
                c.stop_propagation();
                crate::modals::change_version::open(id, &summary, &data, updating.clone(), list.clone(), w, c);
            }
        };
        let update = {
            let backend = self.backend_handle.clone();
            let updating = self.updating.clone();
            let open = open_version.clone();
            cx.listener(move |list, click: &ClickEvent, w, c| {
                c.stop_propagation();
                if !click.modifiers().shift {
                    open(click, w, c);
                    return;
                }
                let delegate = list.delegate_mut();
                if delegate.is_selected(element_id) {
                    for item in &delegate.content {
                        if delegate.is_selected(item.filename_hash)
                            && item.update.can_update(delegate.for_loader, delegate.for_version.as_str())
                        {
                            crate::root::update_single_mod(id, item.id, item.filename_hash, &updating, &backend, w, c);
                        }
                    }
                    delegate.selected.clear();
                    delegate.selected_range.clear();
                    delegate.last_clicked_non_range = None;
                } else {
                    crate::root::update_single_mod(id, content_id, element_id, &updating, &backend, w, c);
                }
                c.notify();
            })
        };
        let mut version = h_flex()
            .w(px(140.))
            .justify_center()
            .gap(px(6.))
            .text_size(px(13.))
            .text_color(rgb(theme::INK_MUTED))
            .child(div().line_clamp(1).child(summary.content_summary.version_str.to_string()));
        if can_update {
            version = version.child(
                DesignButton::new(("update", element_id))
                    .aria_label("Actualizar")
                    .w(px(18.))
                    .h(px(20.))
                    .p_0()
                    .child(
                        gpui_component::Icon::new(if is_loading {
                            PandoraIcon::Loader
                        } else {
                            PandoraIcon::ArrowUp
                        })
                        .size(px(12.))
                        .text_color(rgb(theme::ACCENT_TEXT)),
                    )
                    .on_click(update),
            );
        }
        let toggle = {
            let backend = self.backend_handle.clone();
            YesNo::new(("toggle", element_id))
                .checked(summary.enabled)
                .disabled(!summary.can_toggle)
                .on_click(cx.listener(move |list, checked: &bool, w, c| {
                    c.stop_propagation();
                    w.prevent_default();
                    let delegate = list.delegate();
                    let ids = if delegate.is_selected(element_id) {
                        delegate
                            .content
                            .iter()
                            .filter(|item| delegate.is_selected(item.filename_hash))
                            .map(|item| item.id)
                            .collect()
                    } else {
                        vec![content_id]
                    };
                    backend.send(MessageToBackend::SetContentEnabled {
                        id,
                        content_ids: ids,
                        enabled: *checked,
                    });
                }))
        };
        let page_url = match &summary.content_source {
            ContentSource::ModrinthProject { project_id } => Some(format!("https://modrinth.com/project/{project_id}")),
            ContentSource::CurseforgeProject { project_id } => {
                Some(format!("https://www.curseforge.com/projects/{project_id}"))
            },
            _ => None,
        };
        let manual = matches!(summary.content_source, ContentSource::Manual);
        let can_unpack = summary.content_summary.extra.modpack_files().is_some();
        let backend = self.backend_handle.clone();
        let title = summary.filename.clone();
        let confirmed = self.confirming_delete.lock().contains(&element_id);
        let delete = std::rc::Rc::new(cx.listener(move |list, click: &ClickEvent, _, c| {
            c.stop_propagation();
            let delegate = list.delegate_mut();
            if confirmed || (InterfaceConfig::get(c).quick_delete_mods && click.modifiers().shift) {
                let ids = if delegate.is_selected(element_id) {
                    delegate
                        .content
                        .iter()
                        .filter(|item| delegate.is_selected(item.filename_hash))
                        .map(|item| item.id)
                        .collect()
                } else {
                    vec![content_id]
                };
                delegate.backend_handle.send(MessageToBackend::DeleteContent { id, content_ids: ids });
            } else {
                let mut confirmation = delegate.confirming_delete.lock();
                confirmation.clear();
                if delegate.is_selected(element_id) {
                    confirmation.extend(&delegate.selected);
                    confirmation.extend(&delegate.selected_range);
                } else {
                    confirmation.insert(element_id);
                }
            }
            c.notify();
        }));
        let more = Button::new(("content-more", element_id))
            .ghost()
            .icon(PandoraIcon::Ellipsis)
            .w(px(30.))
            .h(px(26.))
            .border_1()
            .border_color(rgba(0xffffff0b))
            .on_click(|_, _, c| c.stop_propagation())
            .dropdown_menu(move |mut menu, _, _| {
                let open = open_version.clone();
                menu = menu.item(PopupMenuItem::new("Cambiar versión").disabled(manual).on_click(open));
                let url = page_url.clone();
                menu =
                    menu.item(PopupMenuItem::new("Abrir página").disabled(url.is_none()).on_click(move |_, _, c| {
                        if let Some(url) = &url {
                            c.open_url(url);
                        }
                    }));
                if can_unpack {
                    let backend = backend.clone();
                    let title = title.clone();
                    menu = menu.item(PopupMenuItem::new("Descomprimir").on_click(move |_, w, c| {
                        crate::modals::unzip_modpack::open_unzip_modpack(id, content_id, &title, backend.clone(), w, c)
                    }));
                }
                let delete = delete.clone();
                menu.separator()
                    .item(
                        PopupMenuItem::new("Eliminar")
                            .icon(if confirmed {
                                PandoraIcon::Check
                            } else {
                                PandoraIcon::Trash2
                            })
                            .on_click(move |e, w, c| delete(e, w, c)),
                    )
                    .min_w(px(190.))
            });
        let mut name_cell = h_flex().flex_1().min_w_0().gap(px(12.));
        if let Some(index) = expand_index {
            let expanded = self.expanded.clone();
            let open = expanded.load(Ordering::Relaxed) == index;
            name_cell = name_cell.child(
                DesignButton::new(("expand", element_id))
                    .aria_label(name.clone())
                    .w(px(18.))
                    .h(px(26.))
                    .p_0()
                    .child(if open {
                        PandoraIcon::ArrowDown
                    } else {
                        PandoraIcon::ArrowRight
                    })
                    .on_click(cx.listener(move |_, _, _, c| {
                        expanded.store(
                            if expanded.load(Ordering::Relaxed) == index {
                                0
                            } else {
                                index
                            },
                            Ordering::Relaxed,
                        );
                        c.stop_propagation();
                        c.notify();
                    })),
            );
        }
        name_cell = name_cell.child(icon.size(px(32.)).flex_shrink_0().grayscale(!summary.enabled)).child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(1.))
                .child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .line_clamp(1)
                        .text_color(rgb(theme::INK))
                        .child(name),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .line_height(px(14.))
                        .line_clamp(1)
                        .text_color(rgb(theme::INK_MUTED))
                        .child(author),
                ),
        );
        let item_content = h_flex()
            .w_full()
            .h(px(53.))
            .px(px(14.))
            .gap(px(12.))
            .border_b_1()
            .border_color(rgba(0xffffff0b))
            .when(selected, |d| d.bg(rgb(theme::ACCENT_SOFT)))
            .child(name_cell)
            .child(version)
            .child(
                h_flex()
                    .w(px(140.))
                    .justify_center()
                    .text_size(px(12.))
                    .text_color(rgb(theme::INK_MUTED))
                    .child(source),
            )
            .child(h_flex().w(px(84.)).justify_center().child(toggle))
            .child(more);

        ListItem::new(("item", element_id)).p_0().child(item_content).on_click(cx.listener(
            move |this, click: &ClickEvent, _, cx| {
                cx.stop_propagation();
                if click.standard_click() {
                    let delegate = this.delegate_mut();
                    delegate.confirming_delete.lock().clear();
                    if click.modifiers().shift
                        && let Some(from) = delegate.last_clicked_non_range
                    {
                        delegate.selected_range.clear();

                        if let Some(searched) = &delegate.searched {
                            let from_index = searched.iter().position(|element| match element {
                                SummaryOrChild::Summary(summary) => summary.filename_hash == from,
                                SummaryOrChild::Child(_) => false,
                            });

                            let Some(from_index) = from_index else {
                                return;
                            };

                            let to_index = searched.iter().position(|element| match element {
                                SummaryOrChild::Summary(summary) => summary.filename_hash == element_id,
                                SummaryOrChild::Child(_) => false,
                            });

                            let Some(to_index) = to_index else {
                                return;
                            };

                            let min_index = from_index.min(to_index);
                            let max_index = from_index.max(to_index);

                            for add in searched[min_index..=max_index].iter() {
                                match add {
                                    SummaryOrChild::Summary(summary) => {
                                        delegate.selected_range.insert(summary.filename_hash);
                                    },
                                    SummaryOrChild::Child(_) => {},
                                }
                            }
                        } else {
                            let from_index = delegate.content.iter().position(|element| element.filename_hash == from);

                            let Some(from_index) = from_index else {
                                return;
                            };

                            let to_index =
                                delegate.content.iter().position(|element| element.filename_hash == element_id);

                            let Some(to_index) = to_index else {
                                return;
                            };

                            let min_index = from_index.min(to_index);
                            let max_index = from_index.max(to_index);

                            for add in delegate.content[min_index..=max_index].iter() {
                                delegate.selected_range.insert(add.filename_hash);
                            }
                        }
                    } else if click.modifiers().secondary() || click.modifiers().shift {
                        // Cmd+Click (macos), Ctrl+Click (win/linux)

                        delegate.selected.extend(&delegate.selected_range);
                        delegate.selected_range.clear();

                        if delegate.selected.contains(&element_id) {
                            delegate.selected.remove(&element_id);
                        } else {
                            delegate.selected.insert(element_id);
                        }

                        delegate.last_clicked_non_range = Some(element_id);
                    } else {
                        delegate.selected_range.clear();
                        delegate.selected.clear();
                        delegate.selected.insert(element_id);
                        delegate.last_clicked_non_range = Some(element_id);
                    }
                }
            },
        ))
    }

    fn render_child_entry(&self, child: &ContentEntryChild, cx: &mut App) -> ListItem {
        let summary = &child.summary;
        let icon = if let Some(png_icon) = summary.png_icon.as_ref() {
            png_render_cache::render(png_icon.clone(), cx)
        } else {
            gpui::img(ImageSource::Resource(Resource::Embedded("images/default_mod.png".into())))
        };

        let mut hasher = DefaultHasher::new();
        child.parent_filename_hash.hash(&mut hasher);
        child.path.hash(&mut hasher);
        let element_id = hasher.finish();

        let enabled = child.enabled;
        let visually_enabled = enabled && child.parent_enabled && !child.disabled_third_party_downloads;

        let (desc1, desc2) = create_descriptions(
            summary.name.clone(),
            summary.version_str.clone(),
            summary.authors.clone(),
            summary.rich_description.clone(),
            !visually_enabled,
            child.path.clone(),
            cx.theme().muted_foreground,
        );

        let mut item_content = h_flex()
            .gap_1()
            .pl_4()
            .child(
                Switch::new(("toggle", element_id))
                    .checked(enabled && !child.disabled_third_party_downloads)
                    .when_else(
                        child.is_missing || child.disabled_third_party_downloads,
                        |this| this.disabled(true),
                        |this| {
                            this.on_click({
                                let id = self.id;
                                let content_id = child.parent;
                                let child_id = child.summary.id.clone();
                                let child_name = child.summary.name.clone();
                                let path = child.path.clone();
                                let disabled_default = child.disabled_default;
                                let backend_handle = self.backend_handle.clone();
                                move |checked, window, cx| {
                                    cx.stop_propagation();
                                    window.prevent_default();

                                    backend_handle.send(MessageToBackend::SetContentChildEnabled {
                                        id,
                                        content_id,
                                        child_id: child_id.clone(),
                                        child_name: child_name.clone(),
                                        child_filename: path.clone(),
                                        disabled_default,
                                        enabled: *checked,
                                    });
                                }
                            })
                        },
                    )
                    .px_2(),
            )
            .child(icon.size_16().min_w_16().min_h_16().grayscale(!visually_enabled))
            .line_height(rems(1.2))
            .child(desc1.pl_2().when(!visually_enabled, |this| this.line_through()))
            .when_some(desc2, |div, desc2| div.child(desc2.when(!visually_enabled, |this| this.line_through())));

        if child.disabled_third_party_downloads {
            item_content = item_content.child(
                ErrorAlert::new(
                    t::instance::content::blocked().into(),
                    t::instance::content::install::no_third_party_downloads().into(),
                )
                .w(Length::Auto),
            );
        } else if child.is_missing {
            item_content = item_content.child(
                Button::new("download").label(t::instance::content::download()).success().on_click({
                    let backend_handle = self.backend_handle.clone();
                    let id = self.id;
                    let content_id = child.parent;
                    move |_, window, cx| {
                        let modal_action = ModalAction::default();

                        backend_handle.send(MessageToBackend::DownloadContentChildren {
                            id,
                            content_id,
                            modal_action: modal_action.clone(),
                        });

                        crate::modals::generic::show_modal(
                            window,
                            cx,
                            t::instance::content::downloading_children().into(),
                            t::instance::content::error_downloading_children().into(),
                            modal_action,
                        );
                    }
                }),
            );
        }

        ListItem::new(("item", element_id)).p_1().child(item_content)
    }

    pub fn set_content(&mut self, new_content: &[InstanceContentSummary]) {
        let last_mods_len = self.content.len();

        struct Item {
            modification: InstanceContentSummary,
            children: Vec<ContentEntryChild>,
        }

        let mut items = Vec::with_capacity(new_content.len());

        for modification in new_content.iter() {
            let mut inner_children = Vec::new();

            let extra = &modification.content_summary.extra;
            let files = if let ContentType::ModrinthModpack { files, .. } = extra {
                Some(files)
            } else if let ContentType::CurseforgeModpack {
                unknown_files, files, ..
            } = &extra
            {
                for unknown_file in unknown_files.iter() {
                    let filename: Arc<str> = t::instance::content::file_id(unknown_file.file_id).into();

                    let lowercase_filename: Arc<str> = filename.to_ascii_lowercase().into();
                    let lowercase_search_keys = Arc::new([lowercase_filename]);

                    inner_children.push(ContentEntryChild {
                        summary: UNKNOWN_CONTENT_SUMMARY.clone(),
                        parent_filename_hash: modification.filename_hash,
                        parent: modification.id,
                        lowercase_search_keys,
                        path: filename,
                        filesize: 0,
                        disabled_default: false,
                        enabled: true,
                        parent_enabled: modification.enabled,
                        disabled_third_party_downloads: false,
                        is_missing: true,
                    });
                }

                Some(files)
            } else {
                None
            };

            if let Some(files) = files {
                for file in files.iter() {
                    if let Some(path) = file.path()
                        && !path.starts_with("mods")
                        && !path.starts_with("resourcepacks")
                    {
                        continue;
                    }

                    let summary = file.summary.clone();

                    let mut id = None;
                    let mut name = None;

                    if let Some(content_summary) = &summary {
                        id = content_summary.id.as_ref().map(|s| &**s);
                        name = content_summary.name.as_ref().map(|s| &**s);
                    }

                    let enabled =
                        modification
                            .disabled_children
                            .is_enabled(file.default_disabled, id, name, file.path.as_str());

                    let is_missing = summary.is_none();
                    let summary = summary.unwrap_or(UNKNOWN_CONTENT_SUMMARY.clone());

                    let lowercase_filename: Arc<str> = file.path.as_str().to_ascii_lowercase().into();

                    let lowercase_search_keys = summary
                        .id
                        .clone()
                        .into_iter()
                        .chain(summary.name.clone().into_iter())
                        .chain(std::iter::once(lowercase_filename))
                        .collect();

                    let filesize = match &file.source {
                        bridge::instance::ModpackFileSource::DownloadUrl { size, .. } => *size as u64,
                        bridge::instance::ModpackFileSource::DownloadCurseforge { .. } => 0,
                        bridge::instance::ModpackFileSource::Builtin { bytes } => bytes.len() as u64,
                    };

                    inner_children.push(ContentEntryChild {
                        summary,
                        parent_filename_hash: modification.filename_hash,
                        parent: modification.id,
                        lowercase_search_keys,
                        path: file.path.as_str().into(),
                        filesize,
                        disabled_default: file.default_disabled,
                        enabled,
                        parent_enabled: modification.enabled,
                        disabled_third_party_downloads: file.disabled_third_party_downloads,
                        is_missing,
                    });
                }
            }

            items.push(Item {
                modification: modification.clone(),
                children: inner_children,
            });
        }

        let enabled_first = self.enabled_first;
        let sort_key = self.sort_key;

        items.sort_by(|a, b| {
            let ordering = a.children.len().cmp(&b.children.len()).reverse();
            if ordering != std::cmp::Ordering::Equal {
                return ordering;
            }

            if enabled_first && a.modification.enabled != b.modification.enabled {
                return b.modification.enabled.cmp(&a.modification.enabled);
            }

            let ordering = sort_key.compare(&a.modification, &b.modification);
            if ordering != std::cmp::Ordering::Equal {
                return ordering;
            }

            for key in InstanceContentSortKey::iter() {
                if key == sort_key {
                    continue;
                }

                let ordering = key.compare(&a.modification, &b.modification);
                if ordering != std::cmp::Ordering::Equal {
                    return ordering;
                }
            }

            return std::cmp::Ordering::Equal;
        });

        for item in &mut items {
            item.children.sort_by(|a, b| {
                if enabled_first && a.enabled != b.enabled {
                    return b.enabled.cmp(&a.enabled);
                }

                let ordering = a.compare_by(b, sort_key);
                if ordering != std::cmp::Ordering::Equal {
                    return ordering;
                }

                for key in InstanceContentSortKey::iter() {
                    if key == sort_key {
                        continue;
                    }

                    let ordering = a.compare_by(b, key);
                    if ordering != std::cmp::Ordering::Equal {
                        return ordering;
                    }
                }

                return std::cmp::Ordering::Equal;
            });
        }

        let mods: Vec<_> = items.iter().map(|i| i.modification.clone()).collect();
        let children: Vec<_> = items.into_iter().map(|i| i.children).collect();

        let mut updating = self.updating.lock();
        if !updating.is_empty() {
            let ids: FxHashSet<u64> = mods.iter().map(|summary| summary.filename_hash).collect();
            updating.retain(|id| ids.contains(&id));
        }
        drop(updating);

        self.loading = false;
        self.content = mods.clone();
        self.children = children;
        self.searched = None;
        self.confirming_delete.lock().clear();
        if last_mods_len != self.content.len() {
            self.expanded.store(0, Ordering::Release);
        }
        let _ = self.actual_perform_search(&self.last_query.clone());
    }

    fn actual_perform_search(&mut self, query: &str) {
        let query = query.trim_ascii();

        self.last_clicked_non_range = None;

        if query.is_empty() {
            self.last_query = SharedString::new_static("");
            self.searched = None;
            return;
        }

        self.last_query = SharedString::new(query);

        let query = query.to_lowercase();

        let mut searched = Vec::new();

        for (m, children) in self.content.iter().zip(self.children.iter()) {
            let mut parent_added = false;

            if m.lowercase_search_keys.iter().any(|f| f.contains(&query)) {
                parent_added = true;
                searched.push(SummaryOrChild::Summary(m.clone()));
            }

            for child in children {
                if child.lowercase_search_keys.iter().any(|f| f.contains(&query)) {
                    if !parent_added {
                        parent_added = true;
                        searched.push(SummaryOrChild::Summary(m.clone()));
                    }

                    searched.push(SummaryOrChild::Child(child.clone()));
                }
            }
        }

        self.searched = Some(searched);
    }

    fn is_selected(&self, element_id: u64) -> bool {
        self.selected.contains(&element_id) || self.selected_range.contains(&element_id)
    }

    pub fn clear_selection(&mut self) {
        self.selected.clear();
        self.selected_range.clear();
        self.last_clicked_non_range = None;
        self.confirming_delete.lock().clear();
    }

    pub fn select_all(&mut self) {
        self.clear_selection();

        if let Some(searched) = &self.searched {
            for element in searched {
                if let SummaryOrChild::Summary(summary) = element {
                    self.selected.insert(summary.filename_hash);
                }
            }
        } else {
            for summary in &self.content {
                self.selected.insert(summary.filename_hash);
            }
        }
    }
}

impl ListDelegate for ContentListDelegate {
    type Item = ListItem;
    fn render_empty(&mut self, _: &mut Window, _: &mut Context<ListState<Self>>) -> impl IntoElement {
        let text = if !self.content.is_empty() {
            "No hay resultados.".to_owned()
        } else {
            format!(
                "No hay {} en esta instancia.",
                match self.folder {
                    ContentFolder::Mods => "mods",
                    ContentFolder::ResourcePacks => "paquetes de recursos",
                    ContentFolder::Shaders => "shaders",
                }
            )
        };
        div()
            .w_full()
            .py(px(32.))
            .text_center()
            .text_color(rgb(crate::theme::INK_MUTED))
            .child(text)
    }

    fn items_count(&self, _section: usize, _cx: &App) -> usize {
        if let Some(searched) = &self.searched {
            return searched.len();
        }

        let expanded = self.expanded.load(Ordering::Relaxed);
        if expanded > 0 {
            self.content.len() + self.children[expanded - 1].len()
        } else {
            self.content.len()
        }
    }

    fn loading(&self, _cx: &App) -> bool {
        self.loading
    }

    fn render_loading(&mut self, _window: &mut Window, cx: &mut Context<ListState<Self>>) -> impl IntoElement {
        v_flex()
            .w_full()
            .h_1_2()
            .items_center()
            .justify_center()
            .child(Spinner::new().color(cx.theme().muted_foreground).with_size(px(36.0)))
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let mut index = ix.row;

        if let Some(searched) = &self.searched {
            let item = searched.get(index)?;
            match item {
                SummaryOrChild::Summary(instance_mod_summary) => {
                    let selected = self.is_selected(instance_mod_summary.filename_hash);
                    return Some(self.render_summary(instance_mod_summary, selected, None, cx));
                },
                SummaryOrChild::Child(mod_entry_child) => {
                    return Some(self.render_child_entry(mod_entry_child, cx));
                },
            }
        }

        let expanded = self.expanded.load(Ordering::Relaxed);

        if expanded > 0 && index >= expanded {
            if let Some(child) = self.children[expanded - 1].get(index - expanded) {
                return Some(self.render_child_entry(child, cx));
            }
            index -= self.children[expanded - 1].len();
        }

        let summary = self.content.get(index)?;
        let selected = self.is_selected(summary.filename_hash);

        let expand_index = if self.children[index].is_empty() {
            None
        } else {
            Some(index + 1)
        };
        Some(self.render_summary(summary, selected, expand_index, cx))
    }

    fn set_selected_index(&mut self, _ix: Option<IndexPath>, _window: &mut Window, _cx: &mut Context<ListState<Self>>) {
    }

    fn perform_search(&mut self, query: &str, _window: &mut Window, _cx: &mut Context<ListState<Self>>) -> Task<()> {
        self.actual_perform_search(query);
        Task::ready(())
    }
}

fn create_descriptions(
    name: Option<Arc<str>>,
    version: Arc<str>,
    authors: Arc<str>,
    rich_description: Option<Arc<FlatTextComponent>>,
    grayscale: bool,
    filename: Arc<str>,
    secondary: Hsla,
) -> (Div, Option<Div>) {
    if name.is_none() && authors.is_empty() {
        if let Some(rich_description) = rich_description {
            let styled_text = super::create_styled_text(&*rich_description, grayscale);

            let description1 = v_flex()
                .min_w_2_5()
                .gap_2()
                .whitespace_nowrap()
                .overflow_x_hidden()
                .line_height(relative(1.0))
                .child(SharedString::from(filename))
                .child(div().line_clamp(2).child(styled_text));
            return (description1, None);
        }

        let description1 = v_flex()
            .min_w_1_5()
            .whitespace_nowrap()
            .overflow_x_hidden()
            .child(SharedString::from(filename))
            .child(div().text_color(secondary).child(SharedString::from(version)));
        return (description1, None);
    }

    let description1 = v_flex()
        .min_w_1_5()
        .whitespace_nowrap()
        .overflow_x_hidden()
        .child(SharedString::from(name.clone().unwrap_or(filename.clone())))
        .child(div().text_color(secondary).child(SharedString::from(version)));

    let mut description2 = v_flex().text_color(secondary).child(SharedString::from(authors));

    if name.is_some() {
        description2 = description2.child(SharedString::from(filename));
    }

    (description1, Some(description2))
}
