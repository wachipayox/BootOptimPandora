use std::collections::{BTreeMap, BTreeSet};

use crate::entity::instance::InstanceEntry;
use crate::icon::PandoraIcon;
use bridge::{
    handle::BackendHandle,
    instance::InstanceID,
    message::MessageToBackend,
    profile_overwrites::{
        OverwriteChange, ProfileAncestorText, ProfileOverwritesReport, ProfileTextFile, RestoreSource,
    },
};
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _, Disableable, Icon, Sizable, StyledExt, WindowExt,
    button::{Button, DropdownButton},
    h_flex,
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
    menu::{ContextMenuExt, PopupMenuItem},
    resizable::{h_resizable, resizable_panel},
    scroll::{ScrollableElement, Scrollbar},
    v_flex,
};

pub struct InstanceOverwritesSubpage {
    instance: InstanceID,
    backend: BackendHandle,
    report: Option<Result<ProfileOverwritesReport, String>>,
    expanded: BTreeSet<String>,
    selected: Option<String>,
    text: Option<Result<ProfileTextFile, String>>,
    ancestor: Option<Result<ProfileAncestorText, String>>,
    ancestor_level: usize,
    ancestor_cache: BTreeMap<(String, usize), ProfileAncestorText>,
    diff_rows: Vec<DiffRow>,
    diff_hunks: Vec<(usize, usize, bool, bool)>,
    diff_width: f32,
    diff_scroll: UniformListScrollHandle,
    editing: bool,
    _ancestor_read: Task<()>,
    editor: Entity<TextareaState>,
    search: Entity<InputState>,
    only_changes: bool,
    notice: Option<String>,
    busy: bool,
    _load: Task<()>,
    _policy_check: Task<()>,
    _read: Task<()>,
    _save: Task<()>,
}

struct TreeRow {
    path: String,
    folder: bool,
    connector: String,
}

struct DiffRow {
    left: Option<(usize, String)>,
    right: Option<(usize, String)>,
    changed: bool,
}

// Bounded LCS for ordinary configs; large files use a bounded alignment window.
// Work runs on the background executor, and rows are virtualized in the viewer.
fn diff_lines(left: &str, right: &str) -> Vec<DiffRow> {
    let a: Vec<_> = left.split_inclusive('\n').collect();
    let b: Vec<_> = right.split_inclusive('\n').collect();
    let width = b.len() + 1;
    let mut table = if (a.len() + 1).saturating_mul(width) <= 2_000_000 {
        vec![0u32; (a.len() + 1) * width]
    } else {
        Vec::new()
    };
    if !table.is_empty() {
        for i in (0..a.len()).rev() {
            for j in (0..b.len()).rev() {
                table[i * width + j] = if a[i] == b[j] {
                    1 + table[(i + 1) * width + j + 1]
                } else {
                    table[(i + 1) * width + j].max(table[i * width + j + 1])
                };
            }
        }
    }
    let mut rows = Vec::new();
    let (mut i, mut j) = (0, 0);
    let mut removed = Vec::new();
    let mut added = Vec::new();
    let flush = |rows: &mut Vec<DiffRow>, removed: &mut Vec<(usize, String)>, added: &mut Vec<(usize, String)>| {
        let mut l = removed.drain(..);
        let mut r = added.drain(..);
        loop {
            let left = l.next();
            let right = r.next();
            if left.is_none() && right.is_none() {
                break;
            }
            rows.push(DiffRow {
                left,
                right,
                changed: true,
            });
        }
    };
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            flush(&mut rows, &mut removed, &mut added);
            rows.push(DiffRow {
                left: Some((i + 1, a[i].into())),
                right: Some((j + 1, b[j].into())),
                changed: false,
            });
            i += 1;
            j += 1;
        } else {
            let take_left = j == b.len()
                || (i < a.len()
                    && if !table.is_empty() {
                        table[(i + 1) * width + j] >= table[i * width + j + 1]
                    } else {
                        let next_a = a.iter().skip(i + 1).take(64).position(|line| j < b.len() && *line == b[j]);
                        let next_b = b.iter().skip(j + 1).take(64).position(|line| *line == a[i]);
                        next_a.unwrap_or(65) <= next_b.unwrap_or(65)
                    });
            if take_left {
                removed.push((i + 1, a[i].into()));
                i += 1;
            } else {
                added.push((j + 1, b[j].into()));
                j += 1;
            }
        }
    }
    flush(&mut rows, &mut removed, &mut added);
    rows
}

fn ancestor_label(level: usize) -> String {
    match level {
        0 => "Madre".into(),
        1 => "Abuela".into(),
        _ => format!("Antepasado {}", level + 1),
    }
}

fn diff_cell(line: &Option<(usize, String)>, changed: bool, left: bool, muted: Hsla, border: Hsla) -> Div {
    let background = if changed && line.is_some() {
        if left {
            rgba(0x2ea04325).into()
        } else {
            rgba(0xf8514925).into()
        }
    } else {
        transparent_black()
    };
    let (number, text) = line
        .as_ref()
        .map(|(n, t)| (n.to_string(), t.trim_end_matches(['\r', '\n']).replace('\t', "    ")))
        .unwrap_or_default();
    h_flex()
        .w_full()
        .min_w_0()
        .h(px(24.))
        .bg(background)
        .border_r_1()
        .border_color(border)
        .child(div().w(px(58.)).flex_shrink_0().pr_3().text_right().text_color(muted).child(number))
        .child(div().flex_1().min_w_0().whitespace_nowrap().overflow_hidden().child(text))
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
            ancestor: None,
            ancestor_level: 0,
            ancestor_cache: BTreeMap::new(),
            diff_rows: Vec::new(),
            diff_hunks: Vec::new(),
            diff_width: 0.,
            diff_scroll: UniformListScrollHandle::new(),
            editing: false,
            _ancestor_read: Task::ready(()),
            editor: cx.new(|cx| TextareaState::new(window, cx).auto_grow(10, 24)),
            search,
            only_changes: false,
            notice: None,
            busy: false,
            _load: Task::ready(()),
            _policy_check: Task::ready(()),
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
        self.ancestor_cache.clear();
        let (channel, receive) = tokio::sync::oneshot::channel();
        self.backend.send(MessageToBackend::GetProfileOverwrites {
            id: self.instance,
            channel,
        });
        self._load = cx.spawn_in(window, async move |page, cx| {
            let result = receive.await.unwrap_or_else(|_| Err("Launcher backend stopped".into()));
            _ = page.update_in(cx, |page, window, cx| {
                page.report = Some(result);
                if let Some(path) = page.selected.clone() {
                    page.select(path, window, cx);
                }
                cx.notify();
            });
        });
        cx.notify();
    }

    pub(crate) fn on_reenter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (channel, receive) = tokio::sync::oneshot::channel();
        self.backend.send(MessageToBackend::GetBackendConfiguration { channel });
        self._policy_check = cx.spawn_in(window, async move |page, cx| {
            if let Ok(config) = receive.await {
                _ = page.update_in(cx, |page, window, cx| {
                    let new_paths = config.ignored_profile_paths.0;
                    let Some(Ok(report)) = &page.report else {
                        return;
                    };
                    if report.ignored_paths == new_paths {
                        return;
                    }
                    if report.ignored_paths.iter().all(|path| new_paths.contains(path)) {
                        page.apply_ignore_policy(new_paths, cx);
                    } else {
                        page.reload(window, cx);
                    }
                });
            }
        });
    }

    fn apply_ignore_policy(&mut self, paths: Vec<String>, cx: &mut Context<Self>) {
        let Some(Ok(report)) = &mut self.report else {
            return;
        };
        let ignored = schema::ignored_profile_paths::IgnoredProfilePaths(paths.clone());
        report.ignored_paths = paths;
        report.files.retain(|file| !ignored.contains(&file.path));
        report.local_changes.retain(|change| !ignored.contains(&change.path));
        report.global_changes.retain(|change| !ignored.contains(&change.path));
        if self.selected.as_ref().is_some_and(|path| ignored.contains(path)) {
            self.selected = None;
            self.text = None;
        }
        cx.notify();
    }

    fn select(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = Some(path.clone());
        self.text = None;
        self.notice = None;
        self.editing = false;
        self.ancestor_level = 0;
        self.ancestor = None;
        self.diff_rows.clear();
        let editable = matches!(
            path.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str(),
            "toml" | "properties" | "txt" | "cfg" | "ini" | "json" | "mcmeta" | "yaml" | "yml"
        );
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
                    page.load_ancestor(window, cx);
                    cx.notify();
                });
            });
        }
        cx.notify();
    }

    fn load_ancestor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.selected.clone() else {
            return;
        };
        self.ancestor = None;
        self.diff_rows.clear();
        let level = self.ancestor_level;
        let cached = self.ancestor_cache.get(&(path.clone(), level)).cloned();
        let local = self
            .text
            .as_ref()
            .and_then(|r| r.as_ref().ok())
            .map(|f| f.contents.clone())
            .unwrap_or_default();
        let (channel, receive) = tokio::sync::oneshot::channel();
        if cached.is_none() {
            self.backend.send(MessageToBackend::ReadProfileAncestorText {
                id: self.instance,
                path: path.clone(),
                level,
                channel,
            });
        }
        self._ancestor_read = cx.spawn_in(window, async move |page, cx| {
            let result = if let Some(cached) = cached {
                Ok(cached)
            } else {
                receive.await.unwrap_or_else(|_| Err("Launcher backend stopped".into()))
            };
            let right = result.as_ref().ok().and_then(|r| r.contents.clone()).unwrap_or_default();
            let (rows, width, hunks) = cx
                .background_executor()
                .spawn(async move {
                    let rows = diff_lines(&local, &right);
                    let width = rows
                        .iter()
                        .flat_map(|r| [&r.left, &r.right])
                        .filter_map(|line| line.as_ref())
                        .map(|(_, text)| text.chars().map(|c| if c == '\t' { 4 } else { 1 }).sum::<usize>())
                        .max()
                        .unwrap_or(0);
                    let mut hunks = Vec::new();
                    let mut start = 0;
                    while start < rows.len() {
                        if !rows[start].changed {
                            start += 1;
                            continue;
                        }
                        let mut end = start;
                        let (mut local, mut parent) = (false, false);
                        while end < rows.len() && rows[end].changed {
                            local |= rows[end].left.is_some();
                            parent |= rows[end].right.is_some();
                            end += 1;
                        }
                        hunks.push((start, end - start, local, parent));
                        start = end;
                    }
                    (rows, (width as f32 * 8.5 + 68.) * 2., hunks)
                })
                .await;
            _ = page.update_in(cx, |page, _, cx| {
                if page.selected.as_deref() != Some(&path) || page.ancestor_level != level {
                    return;
                }
                if let Ok(value) = &result {
                    if page.ancestor_cache.len() >= 32 {
                        page.ancestor_cache.clear();
                    }
                    page.ancestor_cache.insert((path, level), value.clone());
                }
                page.ancestor = Some(result);
                page.diff_rows = rows;
                page.diff_hunks = hunks;
                page.diff_width = width;
                cx.notify();
            });
        });
        cx.notify();
    }

    fn comparison_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let border = cx.theme().border;
        let secondary = cx.theme().secondary;
        let mut view = v_flex().flex_1().min_h_0().gap_2();
        let names = self
            .ancestor
            .as_ref()
            .and_then(|r| r.as_ref().ok())
            .map(|r| r.ancestors.clone())
            .unwrap_or_default();
        let label = names
            .get(self.ancestor_level)
            .map(|name| format!("{} · {name}", ancestor_label(self.ancestor_level)))
            .unwrap_or_else(|| "Elegir antepasado".into());
        let label = if label.chars().count() > 45 {
            format!("{}…", label.chars().take(44).collect::<String>())
        } else {
            label
        };
        let entity = cx.entity().downgrade();
        view = view.child(
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .text_sm()
                        .text_color(muted)
                        .child("Verde: cambios de esta instancia · Rojo: contenido de la madre"),
                )
                .child(
                    Button::new("edit-comparison-file")
                        .label("Editar archivo")
                        .disabled(!matches!(self.text, Some(Ok(_))) || self.busy)
                        .on_click(cx.listener(|page, _, _, cx| {
                            page.editing = true;
                            cx.notify();
                        })),
                ),
        );
        let ancestor_selector = DropdownButton::new("comparison-ancestor")
            .small()
            .button(Button::new("ancestor-label").label(label))
            .dropdown_menu(move |mut menu, window, _| {
                for (level, name) in names.iter().enumerate() {
                    let Some(entity) = entity.upgrade() else {
                        break;
                    };
                    menu = menu.item(PopupMenuItem::new(format!("{} · {name}", ancestor_label(level))).on_click(
                        window.listener_for(&entity, move |page: &mut Self, _, window, cx| {
                            page.ancestor_level = level;
                            page.load_ancestor(window, cx);
                        }),
                    ));
                }
                menu
            });
        match &self.ancestor {
            None => view = view.child(div().p_4().text_color(muted).child("Cargando comparación…")),
            Some(Err(error)) => {
                view =
                    view.child(div().p_4().text_color(muted).child(format!("No se pudo leer el antepasado: {error}")))
            },
            Some(Ok(result)) if result.ancestors.is_empty() => {
                view = view.child(
                    div()
                        .p_4()
                        .text_color(muted)
                        .child("Esta instancia no tiene una madre disponible para comparar."),
                )
            },
            Some(Ok(result)) => {
                if result.contents.is_none() {
                    view =
                        view.child(div().text_sm().text_color(muted).child("El archivo no existe en este antepasado."));
                }
                if !matches!(self.text, Some(Ok(_))) {
                    view = view.child(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .child("El archivo no está disponible en esta instancia."),
                    );
                }
                let mut panels = h_resizable("config-comparison-split");
                let mut selector = Some(ancestor_selector);
                for left in [true, false] {
                    let mut header = h_flex()
                        .h(px(44.))
                        .flex_shrink_0()
                        .px_3()
                        .items_center()
                        .bg(secondary)
                        .border_b_1()
                        .border_color(border)
                        .overflow_hidden();
                    header = if left {
                        header.child(div().font_semibold().child("Esta instancia"))
                    } else {
                        header.child(selector.take().unwrap())
                    };
                    let list = uniform_list(
                        if left { "config-diff-left" } else { "config-diff-right" },
                        self.diff_rows.len() + 1,
                        cx.processor(move |page, range: std::ops::Range<usize>, _, _| {
                            range
                                .map(|index| {
                                    // Extra empty row keeps the final real line above the overlay scrollbar.
                                    let Some(row) = page.diff_rows.get(index) else {
                                        return div().w_full().h(px(24.));
                                    };
                                    div().w_full().h(px(24.)).font_family("Consolas").text_sm().child(diff_cell(
                                        if left { &row.left } else { &row.right },
                                        row.changed,
                                        left,
                                        muted,
                                        border,
                                    ))
                                })
                                .collect::<Vec<_>>()
                        }),
                    )
                    .w_full()
                    .h_full()
                    .min_w(px(self.diff_width / 2.))
                    .track_scroll(&self.diff_scroll);
                    panels = panels.child(
                        resizable_panel().size_range(px(180.)..px(10000.)).child(
                            v_flex()
                                .size_full()
                                .min_w_0()
                                .min_h_0()
                                .child(header)
                                .child(div().flex_1().min_h_0().w_full().overflow_x_scrollbar().child(list)),
                        ),
                    );
                }
                let total = (self.diff_rows.len() + 1) as f32;
                let mut overview = div().relative().w(px(12.)).h_full().bg(secondary.opacity(0.25));
                for &(start, count, local, parent) in &self.diff_hunks {
                    overview = overview.child(
                        h_flex()
                            .id(("diff-change-marker", start))
                            .absolute()
                            .left_0()
                            .w_full()
                            .top(relative(start as f32 / total))
                            .h(relative(count as f32 / total))
                            .min_h(px(3.))
                            .cursor_pointer()
                            .child(div().w_1_2().h_full().when(local, |this| this.bg(rgb(0x2ea043))))
                            .child(div().w_1_2().h_full().when(parent, |this| this.bg(rgb(0xf85149))))
                            .on_click(cx.listener(move |page, _, _, cx| {
                                page.diff_scroll.scroll_to_item(start, ScrollStrategy::Center);
                                cx.notify();
                            })),
                    );
                }
                view = view.child(
                    h_flex()
                        .flex_1()
                        .min_h_0()
                        .border_1()
                        .border_color(border)
                        .child(div().flex_1().min_w_0().h_full().child(panels))
                        .child(
                            v_flex().h_full().flex_shrink_0().pt(px(44.)).child(
                                h_flex()
                                    .flex_1()
                                    .min_h_0()
                                    .child(overview)
                                    .child(div().w_3().h_full().child(Scrollbar::vertical(&self.diff_scroll))),
                            ),
                        ),
                );
            },
        }
        view.into_any_element()
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
    fn ignore_path(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let (channel, receive) = tokio::sync::oneshot::channel();
        self.backend.send(MessageToBackend::AddIgnoredProfilePath {
            path: path.clone(),
            channel,
        });
        self.busy = true;
        self._save = cx.spawn_in(window, async move |page, cx| {
            let result = receive.await.unwrap_or_else(|_| Err("Launcher backend stopped".into()));
            _ = page.update_in(cx, |page, _, cx| {
                page.busy = false;
                match result {
                    Ok(()) => {
                        page.notice = Some(format!("/{path} está en la lista global de ignorados."));
                        let mut paths = page
                            .report
                            .as_ref()
                            .and_then(|report| report.as_ref().ok())
                            .map(|report| report.ignored_paths.clone())
                            .unwrap_or_default();
                        paths.push(path.clone());
                        if let Ok(paths) = schema::ignored_profile_paths::IgnoredProfilePaths::normalized(paths) {
                            page.apply_ignore_policy(paths.0, cx);
                        }
                    },
                    Err(error) => page.notice = Some(format!("No se pudo ignorar la ruta: {error}")),
                }
                cx.notify();
            });
        });
        cx.notify();
    }

    fn confirm_restore(&mut self, path: String, source: RestoreSource, window: &mut Window, cx: &mut Context<Self>) {
        let label = match source {
            RestoreSource::LocalParent => "madre local",
            RestoreSource::PinnedGlobal => "revisión global fijada",
        };
        let entity = cx.entity();
        let display_path = path.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let entity = entity.clone();
            let path = path.clone();
            dialog
                .title("Restaurar archivo")
                .width(px(500.0))
                .child(
                    v_flex()
                        .gap_2()
                        .child(format!("¿Restaurar {display_path} desde la {label}?"))
                        .child("Se sustituirá el archivo local si existe. La madre no cambiará."),
                )
                .footer(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("cancel-restore")
                                .label("Cancelar")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(Button::new("confirm-restore").label("Restaurar").on_click(move |_, window, cx| {
                            window.close_dialog(cx);
                            _ = entity.update(cx, |page, cx| page.restore_file(path.clone(), source, window, cx));
                        })),
                )
        });
    }

    fn restore_file(&mut self, path: String, source: RestoreSource, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let (channel, receive) = tokio::sync::oneshot::channel();
        self.backend.send(MessageToBackend::RestoreProfileFile {
            id: self.instance,
            path,
            source,
            channel,
        });
        self.busy = true;
        self.notice = Some("Restaurando archivo…".into());
        self._save = cx.spawn_in(window, async move |page, cx| {
            let result = receive.await.unwrap_or_else(|_| Err("Launcher backend stopped".into()));
            _ = page.update_in(cx, |page, window, cx| {
                page.busy = false;
                match result {
                    Ok(()) => {
                        page.selected = None;
                        page.text = None;
                        page.notice = Some("Archivo restaurado desde su madre.".into());
                        page.reload(window, cx);
                    },
                    Err(error) => page.notice = Some(format!("No se pudo restaurar: {error}")),
                }
                cx.notify();
            });
        });
        cx.notify();
    }
}

fn can_restore(change: OverwriteChange) -> bool {
    !matches!(change, OverwriteChange::Added | OverwriteChange::AddedDisabled)
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
                let page_entity = cx.entity();
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
                    let restore_local = !row.folder
                        && report
                            .local_changes
                            .iter()
                            .any(|change| change.path == comparison && can_restore(change.change));
                    let restore_global = !row.folder
                        && report
                            .global_changes
                            .iter()
                            .any(|change| change.path == comparison && can_restore(change.change));
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
                    let menu_path = path.clone();
                    let menu_entity = page_entity.clone();
                    tree = tree.child(
                        item.on_click(cx.listener(move |page, _, window, cx| {
                            if folder {
                                if !page.expanded.insert(click_path.clone()) {
                                    page.expanded.remove(&click_path);
                                }
                                cx.notify();
                            } else {
                                page.select(click_path.clone(), window, cx);
                            }
                        }))
                        .context_menu(move |menu, _, _| {
                            let ignored_path = menu_path.clone();
                            let ignored_entity = menu_entity.clone();
                            let mut menu = menu.item(
                                PopupMenuItem::new(if folder {
                                    "Ignorar carpeta y su contenido"
                                } else {
                                    "Añadir archivo a ignorados"
                                })
                                .on_click(move |_, window, cx| {
                                    _ = ignored_entity
                                        .update(cx, |page, cx| page.ignore_path(ignored_path.clone(), window, cx));
                                }),
                            );
                            if restore_local {
                                let restore_path = menu_path.clone();
                                let restore_entity = menu_entity.clone();
                                menu = menu.item(PopupMenuItem::new("Restaurar desde madre local").on_click(
                                    move |_, window, cx| {
                                        _ = restore_entity.update(cx, |page, cx| {
                                            page.confirm_restore(
                                                restore_path.clone(),
                                                RestoreSource::LocalParent,
                                                window,
                                                cx,
                                            )
                                        });
                                    },
                                ));
                            }
                            if restore_global {
                                let restore_path = menu_path.clone();
                                let restore_entity = menu_entity.clone();
                                menu =
                                    menu.item(PopupMenuItem::new("Restaurar desde revisión global fijada").on_click(
                                        move |_, window, cx| {
                                            _ = restore_entity.update(cx, |page, cx| {
                                                page.confirm_restore(
                                                    restore_path.clone(),
                                                    RestoreSource::PinnedGlobal,
                                                    window,
                                                    cx,
                                                )
                                            });
                                        },
                                    ));
                            }
                            menu
                        }),
                    );
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
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child("Clic derecho en un archivo o carpeta para ver sus opciones."),
                            )
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
                    let is_text = matches!(
                        path.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str(),
                        "toml" | "properties" | "txt" | "cfg" | "ini" | "json" | "mcmeta" | "yaml" | "yml"
                    );
                    if is_text && !self.editing {
                        details = details.child(self.comparison_view(cx));
                    } else {
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
                                                        Button::new("back-to-comparison")
                                                            .label("Comparar")
                                                            .disabled(dirty || self.busy)
                                                            .on_click(cx.listener(|page, _, window, cx| {
                                                                page.editing = false;
                                                                page.load_ancestor(window, cx);
                                                            })),
                                                    )
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
                                                                cx.listener(|page, _, window, cx| {
                                                                    page.save(window, cx)
                                                                }),
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
                                details =
                                    details.child(
                                        div().p_4().rounded_md().bg(secondary).child(
                                            "Archivo binario o eliminado. La edición de texto no está disponible.",
                                        ),
                                    );
                            },
                        }
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
