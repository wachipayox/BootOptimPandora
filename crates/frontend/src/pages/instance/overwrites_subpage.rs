use std::collections::{BTreeMap, BTreeSet};

use crate::entity::instance::InstanceEntry;
use crate::icon::PandoraIcon;
use bridge::{
    handle::BackendHandle,
    instance::InstanceID,
    message::MessageToBackend,
    profile_overwrites::{OverwriteChange, ProfileOverwritesReport, ProfileTextFile},
};
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _, Disableable, Icon, Sizable, StyledExt,
    button::Button,
    h_flex,
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
    scroll::ScrollableElement,
    v_flex,
};

pub struct InstanceOverwritesSubpage {
    instance: InstanceID,
    backend: BackendHandle,
    report: Option<Result<ProfileOverwritesReport, String>>,
    expanded: BTreeSet<String>,
    selected: Option<String>,
    text: Option<Result<ProfileTextFile, String>>,
    editor: Entity<TextareaState>,
    search: Entity<InputState>,
    only_changes: bool,
    notice: Option<String>,
    busy: bool,
    _load: Task<()>,
    _read: Task<()>,
    _save: Task<()>,
}

struct TreeRow {
    path: String,
    folder: bool,
    connector: String,
}

fn tree_rows(
    report: &ProfileOverwritesReport,
    expanded: &BTreeSet<String>,
    query: &str,
    only_changes: bool,
) -> Vec<TreeRow> {
    let query = query.trim().to_lowercase();
    let changed: BTreeSet<&str> = report
        .local_changes
        .iter()
        .chain(&report.global_changes)
        .map(|change| change.path.as_str())
        .collect();
    let represented: BTreeSet<&str> = report.files.iter().map(|file| file.comparison_path.as_str()).collect();
    let mut leaves = BTreeSet::new();
    for file in &report.files {
        if only_changes && !changed.contains(file.comparison_path.as_str()) {
            continue;
        }
        if !query.is_empty() && !file.path.to_lowercase().contains(&query) {
            continue;
        }
        leaves.insert(file.path.clone());
    }
    for change in report.local_changes.iter().chain(&report.global_changes) {
        if represented.contains(change.path.as_str()) {
            continue;
        }
        if !query.is_empty() && !change.path.to_lowercase().contains(&query) {
            continue;
        }
        leaves.insert(change.path.clone());
    }
    let mut folders = BTreeSet::new();
    let mut nodes = leaves.clone();
    for path in &leaves {
        let mut offset = 0;
        while let Some(index) = path[offset..].find('/') {
            offset += index;
            let folder = path[..offset].to_string();
            folders.insert(folder.clone());
            nodes.insert(folder);
            offset += 1;
        }
    }
    let mut children: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for path in nodes {
        let parent = path.rsplit_once('/').map(|(parent, _)| parent).unwrap_or("");
        children.entry(parent.to_string()).or_default().push(path);
    }
    for (parent, siblings) in &mut children {
        siblings.sort_by(|a, b| {
            let priority = |path: &str| {
                if !parent.is_empty() {
                    return 100;
                }
                match path {
                    "mods" => 0,
                    "config" => 1,
                    "resourcepacks" => 2,
                    "shaderpacks" => 3,
                    "defaultconfigs" => 4,
                    "kubejs" => 5,
                    _ => 100,
                }
            };
            priority(a)
                .cmp(&priority(b))
                .then_with(|| folders.contains(b).cmp(&folders.contains(a)))
                .then_with(|| a.to_lowercase().cmp(&b.to_lowercase()))
        });
    }
    fn walk(
        parent: &str,
        rails: &str,
        children: &BTreeMap<String, Vec<String>>,
        folders: &BTreeSet<String>,
        expanded: &BTreeSet<String>,
        searching: bool,
        output: &mut Vec<TreeRow>,
    ) {
        let Some(siblings) = children.get(parent) else {
            return;
        };
        for (index, path) in siblings.iter().enumerate() {
            let last = index + 1 == siblings.len();
            let connector = if parent.is_empty() {
                String::new()
            } else {
                format!("{rails}{} ", if last { "└─" } else { "├─" })
            };
            let folder = folders.contains(path);
            output.push(TreeRow {
                path: path.clone(),
                folder,
                connector,
            });
            if folder && (searching || expanded.contains(path)) {
                let next_rails = if parent.is_empty() {
                    String::new()
                } else {
                    format!("{rails}{}", if last { "   " } else { "│  " })
                };
                walk(path, &next_rails, children, folders, expanded, searching, output);
            }
        }
    }
    let mut output = Vec::new();
    walk("", "", &children, &folders, expanded, !query.is_empty(), &mut output);
    output
}

impl InstanceOverwritesSubpage {
    pub fn new(
        instance: &Entity<InstanceEntry>,
        backend: BackendHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Buscar archivos o carpetas…"));
        cx.subscribe(&search, Self::on_search_changed).detach();
        let mut page = Self {
            instance: instance.read(cx).id,
            backend,
            report: None,
            expanded: BTreeSet::new(),
            selected: None,
            text: None,
            editor: cx.new(|cx| TextareaState::new(window, cx).auto_grow(10, 24)),
            search,
            only_changes: false,
            notice: None,
            busy: false,
            _load: Task::ready(()),
            _read: Task::ready(()),
            _save: Task::ready(()),
        };
        page.reload(window, cx);
        page
    }

    fn on_search_changed(&mut self, _: Entity<InputState>, event: &InputEvent, cx: &mut Context<Self>) {
        if let InputEvent::Change = event {
            cx.notify();
        }
    }

    fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.report = None;
        let (channel, receive) = tokio::sync::oneshot::channel();
        self.backend.send(MessageToBackend::GetProfileOverwrites {
            id: self.instance,
            channel,
        });
        self._load = cx.spawn_in(window, async move |page, cx| {
            let result = receive.await.unwrap_or_else(|_| Err("Launcher backend stopped".into()));
            _ = page.update_in(cx, |page, _, cx| {
                page.report = Some(result);
                cx.notify();
            });
        });
        cx.notify();
    }

    fn select(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = Some(path.clone());
        self.text = None;
        self.notice = None;
        let editable = self
            .report
            .as_ref()
            .and_then(|r| r.as_ref().ok())
            .is_some_and(|r| r.files.iter().any(|file| file.path == path && file.editable));
        if editable {
            let (channel, receive) = tokio::sync::oneshot::channel();
            self.backend.send(MessageToBackend::ReadProfileTextFile {
                id: self.instance,
                path: path.clone(),
                channel,
            });
            self._read = cx.spawn_in(window, async move |page, cx| {
                let result = receive.await.unwrap_or_else(|_| Err("Launcher backend stopped".into()));
                _ = page.update_in(cx, |page, window, cx| {
                    if page.selected.as_deref() != Some(&path) {
                        return;
                    }
                    if let Ok(file) = &result {
                        page.editor.update(cx, |editor, cx| editor.set_value(file.contents.clone(), window, cx));
                    }
                    page.text = Some(result);
                    cx.notify();
                });
            });
        }
        cx.notify();
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(Ok(file)) = &self.text else {
            return;
        };
        let contents = self.editor.read(cx).value().to_string();
        if contents == file.contents {
            return;
        }
        let path = file.path.clone();
        let (channel, receive) = tokio::sync::oneshot::channel();
        self.backend.send(MessageToBackend::SaveProfileTextFile {
            id: self.instance,
            path: path.clone(),
            contents,
            expected_sha256: file.sha256.clone(),
            channel,
        });
        self.notice = Some("Guardando…".into());
        self.busy = true;
        self._save = cx.spawn_in(window, async move |page, cx| {
            let result = receive.await.unwrap_or_else(|_| Err("Launcher backend stopped".into()));
            _ = page.update_in(cx, |page, window, cx| {
                page.busy = false;
                if page.selected.as_deref() == Some(&path) {
                    match result {
                        Ok(file) => {
                            page.editor.update(cx, |editor, cx| editor.set_value(file.contents.clone(), window, cx));
                            page.text = Some(Ok(file));
                            page.notice = Some("Archivo guardado.".into());
                            page.reload(window, cx);
                        },
                        Err(error) => page.notice = Some(format!("No se pudo guardar: {error}")),
                    }
                }
                cx.notify();
            });
        });
        cx.notify();
    }

    fn toggle_mod(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(path) = self.selected.clone() else {
            return;
        };
        let (channel, receive) = tokio::sync::oneshot::channel();
        self.backend.send(MessageToBackend::ToggleProfileMod {
            id: self.instance,
            path,
            channel,
        });
        self.busy = true;
        self.notice = Some("Cambiando estado del mod…".into());
        self._save = cx.spawn_in(window, async move |page, cx| {
            let result = receive.await.unwrap_or_else(|_| Err("Launcher backend stopped".into()));
            _ = page.update_in(cx, |page, window, cx| {
                page.busy = false;
                match result {
                    Ok(path) => {
                        page.selected = Some(path);
                        page.text = None;
                        page.notice = Some("Estado del mod cambiado.".into());
                        page.reload(window, cx);
                    },
                    Err(error) => page.notice = Some(format!("No se pudo cambiar el mod: {error}")),
                }
                cx.notify();
            });
        });
        cx.notify();
    }
}

impl Render for InstanceOverwritesSubpage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let border = cx.theme().border;
        let secondary = cx.theme().secondary;
        let accent = cx.theme().accent;
        let header = h_flex()
            .justify_between()
            .items_center()
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(Icon::new(PandoraIcon::GitBranch).size_5())
                            .child(div().text_lg().font_semibold().child("Archivos del perfil")),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .child("Explora la rama, revisa lo heredado y edita tus cambios locales."),
                    ),
            )
            .child(
                Button::new("refresh-overwrites")
                    .label("Volver a comparar")
                    .on_click(cx.listener(|page, _, window, cx| page.reload(window, cx))),
            );

        let content = match &self.report {
            None => v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .child(div().text_color(muted).child("Comparando archivos del modpack…"))
                .into_any_element(),
            Some(Err(error)) => v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .child(div().text_color(cx.theme().danger).child(format!("No se pudo leer el modpack: {error}")))
                .into_any_element(),
            Some(Ok(report)) => {
                let query = self.search.read(cx).value();
                let changed: BTreeSet<&str> = report
                    .local_changes
                    .iter()
                    .chain(&report.global_changes)
                    .map(|change| change.path.as_str())
                    .collect();
                let rows = tree_rows(report, &self.expanded, &query, self.only_changes);
                let empty = rows.is_empty();
                let file_index: BTreeMap<_, _> = report.files.iter().map(|file| (file.path.as_str(), file)).collect();
                let mut tree = v_flex();
                for row in rows {
                    let path = row.path.clone();
                    let name = path.rsplit('/').next().unwrap_or(&path).to_string();
                    let comparison = file_index
                        .get(path.as_str())
                        .map(|file| file.comparison_path.as_str())
                        .unwrap_or(path.as_str());
                    let change = report
                        .local_changes
                        .iter()
                        .find(|change| change.path == comparison)
                        .or_else(|| report.global_changes.iter().find(|change| change.path == comparison));
                    let selected = self.selected.as_deref() == Some(&path);
                    let folder = row.folder;
                    let expanded = self.expanded.contains(&path) || !query.trim().is_empty();
                    let icon = if folder {
                        if expanded {
                            PandoraIcon::FolderOpen
                        } else {
                            PandoraIcon::FolderClosed
                        }
                    } else {
                        PandoraIcon::File
                    };
                    let click_path = path.clone();
                    let mut item = h_flex()
                        .id(SharedString::from(format!("overwrite-tree-{path}")))
                        .h_8()
                        .w_full()
                        .px_2()
                        .gap_2()
                        .items_center()
                        .rounded_md()
                        .cursor_pointer()
                        .hover(|style| style.bg(secondary))
                        .child(div().font_family("Consolas").text_sm().text_color(muted).child(row.connector))
                        .child(Icon::new(icon).size_4().text_color(if folder { accent } else { muted }))
                        .child(div().flex_1().min_w_0().truncate().text_sm().child(name));
                    if let Some(change) = change {
                        item = item.child(
                            div()
                                .px_2()
                                .py_px()
                                .rounded_md()
                                .text_xs()
                                .text_color(cx.theme().warning)
                                .bg(cx.theme().warning.opacity(0.12))
                                .child(change_badge(change.change)),
                        );
                    }
                    if selected {
                        item = item.bg(accent.opacity(0.35));
                    }
                    tree = tree.child(item.on_click(cx.listener(move |page, _, window, cx| {
                        if folder {
                            if !page.expanded.insert(click_path.clone()) {
                                page.expanded.remove(&click_path);
                            }
                            cx.notify();
                        } else {
                            page.select(click_path.clone(), window, cx);
                        }
                    })));
                }
                if empty {
                    tree = tree.child(div().p_4().text_sm().text_color(muted).child(if self.only_changes {
                        "No hay diferencias con este filtro."
                    } else {
                        "No hay archivos con este filtro."
                    }));
                }
                let total = report.files.len();
                let explorer = v_flex()
                    .w(px(390.0))
                    .h_full()
                    .flex_shrink_0()
                    .rounded_lg()
                    .border_1()
                    .border_color(border)
                    .bg(secondary.opacity(0.3))
                    .child(
                        v_flex()
                            .gap_3()
                            .p_3()
                            .border_b_1()
                            .border_color(border)
                            .child(
                                h_flex()
                                    .justify_between()
                                    .items_center()
                                    .child(div().font_semibold().child("Explorador"))
                                    .child(div().text_xs().text_color(muted).child(format!("{total} archivos"))),
                            )
                            .child(Input::new(&self.search).prefix(Icon::new(PandoraIcon::Search).small()))
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        div()
                                            .id("overwrite-all")
                                            .px_3()
                                            .py_1()
                                            .rounded_md()
                                            .cursor_pointer()
                                            .bg(if self.only_changes {
                                                secondary
                                            } else {
                                                accent.opacity(0.45)
                                            })
                                            .child("Todos")
                                            .on_click(cx.listener(|page, _, _, cx| {
                                                page.only_changes = false;
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id("overwrite-changed")
                                            .px_3()
                                            .py_1()
                                            .rounded_md()
                                            .cursor_pointer()
                                            .bg(if self.only_changes {
                                                accent.opacity(0.45)
                                            } else {
                                                secondary
                                            })
                                            .child(format!("Diferencias · {}", changed.len()))
                                            .on_click(cx.listener(|page, _, _, cx| {
                                                page.only_changes = true;
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    )
                    .child(v_flex().flex_1().overflow_y_scrollbar().p_2().child(tree));

                let mut details = v_flex()
                    .gap_4()
                    .flex_1()
                    .h_full()
                    .min_w_0()
                    .rounded_lg()
                    .border_1()
                    .border_color(border)
                    .bg(secondary.opacity(0.18))
                    .p_4();
                if let Some(path) = &self.selected {
                    let file_name = path.rsplit('/').next().unwrap_or(path);
                    let directory = path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or(".minecraft");
                    let selected_file = report.files.iter().find(|file| &file.path == path);
                    let comparison = selected_file.map(|file| file.comparison_path.as_str()).unwrap_or(path.as_str());
                    details = details.child(
                        h_flex()
                            .justify_between()
                            .items_start()
                            .child(
                                v_flex()
                                    .gap_1()
                                    .min_w_0()
                                    .child(div().text_lg().font_semibold().truncate().child(file_name.to_string()))
                                    .child(div().text_sm().text_color(muted).child(directory.to_string())),
                            )
                            .child(
                                div().text_xs().text_color(muted).child(
                                    selected_file
                                        .map(|file| format_size(file.size))
                                        .unwrap_or_else(|| "Archivo eliminado".into()),
                                ),
                            ),
                    );
                    let mut sources = h_flex().gap_3();
                    if let Some(parent) = &report.local_parent {
                        let change = report.local_changes.iter().find(|c| c.path == comparison).map(|c| c.change);
                        sources = sources.child(source_card("MADRE LOCAL", parent, change, border, secondary, muted));
                    }
                    if let Some(profile) = &report.global_profile {
                        let change = report.global_changes.iter().find(|c| c.path == comparison).map(|c| c.change);
                        let name = profile.split_once(" · ").map(|(name, _)| name).unwrap_or(profile);
                        sources = sources.child(source_card("PERFIL GLOBAL", name, change, border, secondary, muted));
                    }
                    if report.local_parent.is_some() || report.global_profile.is_some() {
                        details = details.child(sources);
                    }
                    match &self.text {
                        Some(Ok(file)) => {
                            let dirty = self.editor.read(cx).value().as_str() != file.contents;
                            details = details
                                .child(
                                    h_flex()
                                        .justify_between()
                                        .items_center()
                                        .child(v_flex().child(div().font_semibold().child("Contenido")).child(
                                            div().text_xs().text_color(muted).child(if dirty {
                                                "Cambios sin guardar"
                                            } else {
                                                "Edición local de esta rama"
                                            }),
                                        ))
                                        .child(
                                            h_flex()
                                                .gap_2()
                                                .child(
                                                    Button::new("discard-overwrite-text")
                                                        .label("Descartar")
                                                        .disabled(!dirty || self.busy)
                                                        .on_click(cx.listener(|page, _, window, cx| {
                                                            if let Some(Ok(file)) = &page.text {
                                                                let original = file.contents.clone();
                                                                page.editor.update(cx, |editor, cx| {
                                                                    editor.set_value(original, window, cx)
                                                                });
                                                            }
                                                        })),
                                                )
                                                .child(
                                                    Button::new("save-overwrite-text")
                                                        .label("Guardar cambios")
                                                        .disabled(!dirty || self.busy)
                                                        .on_click(
                                                            cx.listener(|page, _, window, cx| page.save(window, cx)),
                                                        ),
                                                ),
                                        ),
                                )
                                .child(v_flex().flex_1().overflow_y_scrollbar().child(Textarea::new(&self.editor)));
                        },
                        Some(Err(error)) => {
                            details = details.child(
                                div()
                                    .p_4()
                                    .rounded_md()
                                    .bg(secondary)
                                    .child(format!("No se puede previsualizar este archivo: {error}")),
                            );
                        },
                        None if selected_file.is_some_and(|file| file.editable) => {
                            details = details.child(div().text_color(muted).child("Abriendo archivo de texto…"));
                        },
                        None => {
                            details = details.child(
                                div()
                                    .p_4()
                                    .rounded_md()
                                    .bg(secondary)
                                    .child("Archivo binario o eliminado. La edición de texto no está disponible."),
                            );
                        },
                    }
                    if selected_file.is_some()
                        && path.to_ascii_lowercase().starts_with("mods/")
                        && (path.to_ascii_lowercase().ends_with(".jar")
                            || path.to_ascii_lowercase().ends_with(".jar.disabled"))
                    {
                        let label = if path.to_ascii_lowercase().ends_with(".disabled") {
                            "Activar mod"
                        } else {
                            "Desactivar mod"
                        };
                        details = details.child(
                            Button::new("toggle-profile-mod")
                                .label(label)
                                .disabled(self.busy)
                                .on_click(cx.listener(|page, _, window, cx| page.toggle_mod(window, cx))),
                        );
                    }
                } else {
                    details = details.child(
                        v_flex()
                            .flex_1()
                            .items_center()
                            .justify_center()
                            .gap_3()
                            .child(Icon::new(PandoraIcon::GitBranch).size_8().text_color(muted))
                            .child(div().font_semibold().child("Explora tu rama"))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(muted)
                                    .child("Elige un archivo del árbol para ver su origen y sus cambios."),
                            ),
                    );
                }
                if let Some(notice) = &self.notice {
                    details = details.child(div().text_sm().child(notice.clone()));
                }
                let mut summary = h_flex().gap_2().items_center();
                if let Some(parent) = &report.local_parent {
                    summary = summary.child(summary_chip(
                        "Madre local",
                        parent,
                        report.local_changes.len(),
                        border,
                        secondary,
                    ));
                }
                if let Some(profile) = &report.global_profile {
                    let name = profile.split_once(" · ").map(|(name, _)| name).unwrap_or(profile);
                    summary = summary.child(summary_chip(
                        "Perfil global",
                        name,
                        report.global_changes.len(),
                        border,
                        secondary,
                    ));
                }
                if report.skipped_entries > 0 {
                    summary = summary.child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("{} enlaces omitidos", report.skipped_entries)),
                    );
                }
                let mut section = v_flex()
                    .gap_3()
                    .flex_1()
                    .min_h_0()
                    .child(summary)
                    .child(h_flex().gap_3().flex_1().min_h_0().child(explorer).child(details));
                if let Some(error) = &report.local_error {
                    section = section.child(div().text_sm().text_color(cx.theme().danger).child(error.clone()));
                }
                if let Some(error) = &report.global_error {
                    section = section.child(div().text_sm().text_color(cx.theme().danger).child(error.clone()));
                }
                section.into_any_element()
            },
        };
        v_flex().size_full().gap_4().p_4().child(header).child(content)
    }
}

fn summary_chip(label: &str, name: &str, count: usize, border: Hsla, background: Hsla) -> AnyElement {
    h_flex()
        .gap_2()
        .items_center()
        .px_3()
        .py_2()
        .rounded_md()
        .border_1()
        .border_color(border)
        .bg(background)
        .child(div().text_xs().child(label.to_string()))
        .child(div().font_semibold().text_sm().child(name.to_string()))
        .child(div().text_xs().child(format!("{count} cambios")))
        .into_any_element()
}

fn source_card(
    label: &str,
    name: &str,
    change: Option<OverwriteChange>,
    border: Hsla,
    background: Hsla,
    muted: Hsla,
) -> AnyElement {
    v_flex()
        .flex_1()
        .gap_1()
        .p_3()
        .rounded_md()
        .border_1()
        .border_color(border)
        .bg(background)
        .child(div().text_xs().text_color(muted).child(label.to_string()))
        .child(div().text_sm().font_semibold().truncate().child(name.to_string()))
        .child(div().text_sm().child(change.map(change_name).unwrap_or("Sin cambios").to_string()))
        .into_any_element()
}

fn format_size(size: u64) -> String {
    if size < 1024 {
        format!("{size} B")
    } else if size < 1024 * 1024 {
        format!("{:.1} KiB", size as f64 / 1024.0)
    } else {
        format!("{:.1} MiB", size as f64 / (1024.0 * 1024.0))
    }
}

fn change_name(change: OverwriteChange) -> &'static str {
    match change {
        OverwriteChange::Added => "Añadido",
        OverwriteChange::AddedDisabled => "Añadido y desactivado",
        OverwriteChange::Modified => "Modificado",
        OverwriteChange::Removed => "Eliminado",
        OverwriteChange::Disabled => "Desactivado",
        OverwriteChange::Enabled => "Activado",
        OverwriteChange::ModifiedAndDisabled => "Modificado y desactivado",
        OverwriteChange::ModifiedAndEnabled => "Modificado y activado",
    }
}

fn change_badge(change: OverwriteChange) -> &'static str {
    match change {
        OverwriteChange::Added | OverwriteChange::AddedDisabled => "Nuevo",
        OverwriteChange::Removed => "Quitado",
        OverwriteChange::Disabled | OverwriteChange::ModifiedAndDisabled => "Off",
        OverwriteChange::Enabled | OverwriteChange::ModifiedAndEnabled => "On",
        OverwriteChange::Modified => "Editado",
    }
}
