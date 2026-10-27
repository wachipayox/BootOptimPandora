use super::overwrites_diff::{DiffRow, DisplayRow, unified_projection};
use std::collections::{BTreeMap, BTreeSet};

use crate::icon::PandoraIcon;
use crate::{
    component::design::{self, DesignButton},
    entity::instance::InstanceEntry,
    theme,
};
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
    ActiveTheme as _, Disableable, Icon, StyledExt, WindowExt,
    button::{Button, DropdownButton},
    h_flex,
    input::{InputEvent, InputState, Textarea, TextareaState},
    menu::{ContextMenuExt, PopupMenuItem},
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
    display_rows: Vec<DisplayRow>,
    diff_hunks: Vec<(usize, usize, bool, bool)>,
    diff_width: f32,
    diff_scroll: UniformListScrollHandle,
    editing: bool,
    _ancestor_read: Task<()>,
    editor: Entity<TextareaState>,
    search: Entity<InputState>,
    change_filter: usize,
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
        .h(px(20.))
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
    change_filter: usize,
) -> Vec<TreeRow> {
    let query = query.trim().to_lowercase();
    let changed: BTreeSet<&str> = report
        .local_changes
        .iter()
        .chain(&report.global_changes)
        .filter(|change| filter_change(change.change, change_filter))
        .map(|change| change.path.as_str())
        .collect();
    let represented: BTreeSet<&str> = report.files.iter().map(|file| file.comparison_path.as_str()).collect();
    let mut leaves = BTreeSet::new();
    for file in &report.files {
        if change_filter != 0 && !changed.contains(file.comparison_path.as_str()) {
            continue;
        }
        if !query.is_empty() && !file.path.to_lowercase().contains(&query) {
            continue;
        }
        leaves.insert(file.path.clone());
    }
    for change in report.local_changes.iter().chain(&report.global_changes) {
        if !filter_change(change.change, change_filter) {
            continue;
        }
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
                    "config" => 0,
                    "mods" => 1,
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
            display_rows: Vec::new(),
            diff_hunks: Vec::new(),
            diff_width: 0.,
            diff_scroll: UniformListScrollHandle::new(),
            editing: false,
            _ancestor_read: Task::ready(()),
            editor: cx.new(|cx| TextareaState::new(window, cx).auto_grow(10, 24)),
            search,
            change_filter: 1,
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
                } else if std::env::var("WBOMBO_UI_CAPTURE_PAGE")
                    .is_ok_and(|route| route.starts_with("instance-overwrites:agente-274-"))
                {
                    let path = "config/agent274-instance.properties";
                    if page
                        .report
                        .as_ref()
                        .and_then(|r| r.as_ref().ok())
                        .is_some_and(|report| report.files.iter().any(|file| file.path == path))
                    {
                        page.expanded.insert("config".into());
                        page.select(path.into(), window, cx);
                    } else {
                        log::info!("[WBOMBO-UI] instance-overwrites-ready pid={}", std::process::id());
                    }
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
        self.display_rows.clear();
        self.diff_hunks.clear();
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
        self.display_rows.clear();
        self.diff_hunks.clear();
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
            let (rows, display_rows, width, hunks) = cx
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
                    let (display_rows, hunks) = unified_projection(&rows, &hunks);
                    (rows, display_rows, width as f32 * 7.2 + 68., hunks)
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
                page.display_rows = display_rows;
                page.diff_hunks = hunks;
                page.diff_width = width;
                if std::env::var("WBOMBO_UI_CAPTURE_PAGE")
                    .is_ok_and(|route| route.starts_with("instance-overwrites:agente-274-"))
                    && std::env::var("WBOMBO_UI_CAPTURE_STATE").as_deref() == Ok("editing")
                    && matches!(page.text, Some(Ok(_)))
                {
                    page.editing = true;
                }
                log::info!("[WBOMBO-UI] instance-overwrites-ready pid={}", std::process::id());
                cx.notify();
            });
        });
        cx.notify();
    }

    fn ancestor_menu(&self, cx: &mut Context<Self>) -> DropdownButton {
        let names = self
            .ancestor
            .as_ref()
            .and_then(|r| r.as_ref().ok())
            .map(|r| r.ancestors.clone())
            .unwrap_or_default();
        let label = names
            .get(self.ancestor_level)
            .map(|name| format!("Comparar con: {name}"))
            .unwrap_or_else(|| "Comparar con:".into());
        let entity = cx.entity().downgrade();
        let selector = DropdownButton::new("comparison-ancestor")
            .button(Button::new("ancestor-label").label(label))
            .disabled(names.is_empty())
            .dropdown_menu(move |mut menu, window, _| {
                for (level, name) in names.iter().enumerate() {
                    let Some(entity) = entity.upgrade() else {
                        break;
                    };
                    menu = menu.item(PopupMenuItem::new(name.clone()).on_click(window.listener_for(
                        &entity,
                        move |page: &mut Self, _, window, cx| {
                            page.ancestor_level = level;
                            page.load_ancestor(window, cx);
                        },
                    )));
                }
                menu
            });
        selector
    }
    fn comparison_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let border = cx.theme().border;
        let secondary = cx.theme().secondary;
        let mut view = v_flex().flex_1().min_h_0().gap_2();
        view = view.child(
            h_flex().gap_2().items_center().child(
                div()
                    .flex_1()
                    .text_sm()
                    .text_color(muted)
                    .child("Verde: cambios de esta instancia · Rojo: contenido de la madre"),
            ),
        );
        match &self.ancestor {
            None => view = view.child(gpui_component::skeleton::Skeleton::new().w_full().h(px(44.))),
            Some(Err(error)) => {
                view = view.child(
                    div()
                        .p_4()
                        .text_color(muted)
                        .child(overwrite_error("No se ha podido leer el antepasado.", &error)),
                )
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
                let list = uniform_list(
                    "config-unified-diff",
                    self.display_rows.len() + 1,
                    cx.processor(move |page, range: std::ops::Range<usize>, _, _| {
                        range
                            .map(|index| {
                                let Some(display) = page.display_rows.get(index) else {
                                    return div().w_full().h(px(20.));
                                };
                                let row = &page.diff_rows[display.pair];
                                div().w_full().h(px(20.)).font_family("Consolas").text_size(px(12.)).child(diff_cell(
                                    if display.local { &row.left } else { &row.right },
                                    row.changed,
                                    display.local,
                                    muted,
                                    border,
                                ))
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .w_full()
                .h_full()
                .min_w(px(self.diff_width))
                .track_scroll(&self.diff_scroll);
                let panels = div().size_full().min_w_0().overflow_x_scrollbar().child(list);
                let total = (self.display_rows.len() + 1) as f32;
                let mut overview = div()
                    .relative()
                    .w(px(16.))
                    .flex_none()
                    .h_full()
                    .border_x_1()
                    .border_color(border)
                    .bg(secondary.opacity(0.6));
                for &(start, count, local, parent) in &self.diff_hunks {
                    overview = overview.child(
                        h_flex()
                            .id(("diff-change-marker", start))
                            .absolute()
                            .left_0()
                            .w_full()
                            .top(relative(start as f32 / total))
                            .h(relative(count as f32 / total))
                            .min_h(px(4.))
                            .cursor_pointer()
                            .child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .top_0()
                                    .bottom_0()
                                    .w(px(7.))
                                    .when(local, |this| this.bg(rgb(0x3fb950))),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .right_0()
                                    .top_0()
                                    .bottom_0()
                                    .w(px(7.))
                                    .when(parent, |this| this.bg(rgb(0xff6b63))),
                            )
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
                            v_flex().w(px(28.)).h_full().flex_none().pt(px(0.)).child(
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
        self.notice = None;
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
                        Err(error) => {
                            page.notice = Some(overwrite_error("No se ha podido guardar el archivo.", &error))
                        },
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
        self.notice = None;
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
                    Err(error) => {
                        page.notice = Some(overwrite_error("No se ha podido cambiar el estado del mod.", &error))
                    },
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
                    Err(error) => page.notice = Some(overwrite_error("No se ha podido ignorar la ruta.", &error)),
                }
                cx.notify();
            });
        });
        cx.notify();
    }

    fn confirm_restore(&mut self, path: String, source: RestoreSource, window: &mut Window, cx: &mut Context<Self>) {
        let label = match source {
            RestoreSource::LocalParent => "Restaurar desde madre local",
            RestoreSource::PinnedGlobal => "Restaurar desde revisión global fijada",
        };
        let entity = cx.entity();
        let display_path = path.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let entity = entity.clone();
            let path = path.clone();
            design::dialog(dialog)
                .title(design::dialog_title("Restaurar archivo"))
                .width(px(500.0))
                .child(
                    v_flex()
                        .gap_2()
                        .child(display_path.clone())
                        .child(label)
                        .child("Se sustituirá el archivo local si existe. La madre no cambiará."),
                )
                .footer(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .gap_2()
                        .child(
                            DesignButton::new("cancel-restore")
                                .label("Cancelar")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            design::action_button("confirm-restore", "Restaurar", design::ButtonAction::Primary)
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    _ = entity
                                        .update(cx, |page, cx| page.restore_file(path.clone(), source, window, cx));
                                }),
                        ),
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
        self.notice = None;
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
                    Err(error) => page.notice = Some(overwrite_error("No se ha podido restaurar el archivo.", &error)),
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
        let mut filters = h_flex().gap(px(2.));
        for (index, label) in ["Todos", "Cambios", "Añadidos", "Eliminados"].into_iter().enumerate() {
            filters =
                filters.child(design::chip(("overwrite-filter", index), label, self.change_filter == index).on_click(
                    cx.listener(move |page, _, _, cx| {
                        page.change_filter = index;
                        cx.notify();
                    }),
                ));
        }
        let header = h_flex()
            .w_full()
            .gap(px(12.))
            .child(
                design::text_field(&self.search)
                    .prefix(Icon::new(PandoraIcon::Search).size(px(14.)))
                    .flex_1()
                    .min_w(px(160.))
                    .max_w(px(340.)),
            )
            .child(self.ancestor_menu(cx))
            .child(filters)
            .when(!self.diff_hunks.is_empty(), |d| {
                d.child(DesignButton::new("first-diff-change").label("Ir al primer cambio").on_click(cx.listener(
                    |page, _, _, cx| {
                        if let Some(&(start, _, _, _)) = page.diff_hunks.first() {
                            page.diff_scroll.scroll_to_item(start, ScrollStrategy::Center);
                            cx.notify();
                        }
                    },
                )))
            });
        let header = v_flex().gap(px(10.)).child(header).child(
            DesignButton::new("refresh-overwrites")
                .label("Volver a comparar")
                .self_start()
                .on_click(cx.listener(|page, _, window, cx| page.reload(window, cx))),
        );

        let content = match &self.report {
            None => v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap(px(12.))
                .child(gpui_component::spinner::Spinner::new())
                .child(div().text_color(rgb(crate::theme::INK_MUTED)).child("Cargando…"))
                .into_any_element(),
            Some(Err(error)) => v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_color(cx.theme().danger)
                        .child(overwrite_error("No se ha podido leer el perfil.", &error)),
                )
                .into_any_element(),
            Some(Ok(report)) => {
                let query = self.search.read(cx).value();
                let rows = tree_rows(report, &self.expanded, &query, self.change_filter);
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
                                .text_xs()
                                .border_1()
                                .border_color(
                                    rgb(match change.change {
                                        OverwriteChange::Added | OverwriteChange::AddedDisabled => 0x55bf7a,
                                        OverwriteChange::Removed => 0xe56b65,
                                        OverwriteChange::Disabled | OverwriteChange::ModifiedAndDisabled => {
                                            theme::INK_MUTED
                                        },
                                        _ => theme::ACCENT,
                                    })
                                    .opacity(0.35),
                                )
                                .text_color(rgb(match change.change {
                                    OverwriteChange::Added | OverwriteChange::AddedDisabled => 0x55bf7a,
                                    OverwriteChange::Removed => 0xe56b65,
                                    OverwriteChange::Disabled | OverwriteChange::ModifiedAndDisabled => {
                                        theme::INK_MUTED
                                    },
                                    _ => theme::ACCENT,
                                }))
                                .child(change_badge(change.change)),
                        );
                    }
                    if selected {
                        item = item.bg(rgba(0x5bcef51a));
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
                    tree = tree.child(div().w_full().p_4().text_center().text_sm().text_color(muted).child(
                        if self.change_filter != 0 {
                            "No hay diferencias con este filtro."
                        } else {
                            "No hay archivos con este filtro."
                        },
                    ));
                }
                let explorer = v_flex()
                    .w(px(340.))
                    .h_full()
                    .flex_shrink_0()
                    .bg(rgb(theme::PANEL_SUNKEN))
                    .child(
                        h_flex()
                            .justify_between()
                            .px(px(14.))
                            .py(px(12.))
                            .child("Explorador")
                            .child(div().text_size(px(11.)).text_color(muted).child("Clic derecho: opciones")),
                    )
                    .child(v_flex().flex_1().min_h_0().overflow_y_scrollbar().p_2().child(tree));

                let mut details = v_flex()
                    .gap_4()
                    .flex_1()
                    .h_full()
                    .min_w_0()
                    .border_1()
                    .border_color(border)
                    .bg(rgb(theme::PANEL_SUNKEN))
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
                            .child(div().text_xs().text_color(muted).child(
                                selected_file.map(|file| format_size(file.size)).unwrap_or_else(|| "Eliminado".into()),
                            )),
                    );
                    let is_text = matches!(
                        path.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str(),
                        "toml" | "properties" | "txt" | "cfg" | "ini" | "json" | "mcmeta" | "yaml" | "yml"
                    );
                    let restore_source = if report
                        .local_changes
                        .iter()
                        .any(|change| change.path == comparison && can_restore(change.change))
                    {
                        Some(RestoreSource::LocalParent)
                    } else if report
                        .global_changes
                        .iter()
                        .any(|change| change.path == comparison && can_restore(change.change))
                    {
                        Some(RestoreSource::PinnedGlobal)
                    } else {
                        None
                    };
                    let mut file_actions = h_flex().w_full().items_center().gap(px(8.));
                    if let Some(source) = restore_source {
                        let restore_path = path.clone();
                        file_actions = file_actions.child(
                            DesignButton::new("restore-selected-parent")
                                .label("Restaurar desde la madre")
                                .disabled(self.busy)
                                .on_click(cx.listener(move |page, _, window, cx| {
                                    page.confirm_restore(restore_path.clone(), source, window, cx)
                                })),
                        );
                    }
                    if is_text && !self.editing {
                        details = details.child(self.comparison_view(cx));
                    } else {
                        match &self.text {
                            Some(Ok(file)) => {
                                let dirty = self.editor.read(cx).value().as_str() != file.contents;
                                details = details
                                    .child(v_flex().child(div().font_semibold().child("Contenido")).child(
                                        div().text_xs().text_color(muted).child(if dirty {
                                            "Cambios sin guardar"
                                        } else {
                                            "Sin cambios"
                                        }),
                                    ))
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .overflow_y_scrollbar()
                                            .child(Textarea::new(&self.editor).bg(rgb(theme::PANEL_SUNKEN))),
                                    );
                                file_actions = file_actions
                                    .child(div().flex_1())
                                    .child(
                                        DesignButton::new("back-to-comparison")
                                            .label("Comparar")
                                            .disabled(dirty || self.busy)
                                            .on_click(cx.listener(|page, _, window, cx| {
                                                page.editing = false;
                                                page.load_ancestor(window, cx);
                                            })),
                                    )
                                    .child(
                                        DesignButton::new("discard-overwrite-text")
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
                                        design::action_button(
                                            "save-overwrite-text",
                                            "Guardar cambios",
                                            design::ButtonAction::Primary,
                                        )
                                        .disabled(!dirty || self.busy)
                                        .on_click(cx.listener(|page, _, window, cx| page.save(window, cx))),
                                    );
                            },
                            Some(Err(error)) => {
                                details = details.child(
                                    div()
                                        .p_4()
                                        .bg(secondary)
                                        .child(overwrite_error("No se ha podido previsualizar el archivo.", &error)),
                                );
                            },
                            None if selected_file.is_some_and(|file| file.editable) => {
                                details = details.child(gpui_component::skeleton::Skeleton::new().w_full().h(px(44.)));
                            },
                            None => {
                                details = details.child(
                                    div()
                                        .p_4()
                                        .bg(secondary)
                                        .child("Archivo binario o eliminado. La edición de texto no está disponible."),
                                );
                            },
                        }
                    }
                    if is_text && !self.editing {
                        file_actions = file_actions.child(
                            DesignButton::new("edit-comparison-file")
                                .label("Editar archivo")
                                .disabled(!matches!(self.text, Some(Ok(_))) || self.busy)
                                .on_click(cx.listener(|page, _, _, cx| {
                                    page.editing = true;
                                    cx.notify();
                                })),
                        );
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
                        file_actions = file_actions.child(
                            DesignButton::new("toggle-profile-mod")
                                .label(label)
                                .disabled(self.busy)
                                .on_click(cx.listener(|page, _, window, cx| page.toggle_mod(window, cx))),
                        );
                    }
                    details = details.child(file_actions);
                }
                if let Some(notice) = &self.notice {
                    details = details.child(div().text_sm().child(notice.clone()));
                }
                let mut section = v_flex()
                    .gap_3()
                    .flex_1()
                    .min_h_0()
                    .child(h_flex().gap_3().flex_1().min_h_0().child(explorer).child(details));
                if let Some(error) = &report.local_error {
                    section = section.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().danger)
                            .child(overwrite_error("No se ha podido leer el antepasado.", error)),
                    );
                }
                if let Some(error) = &report.global_error {
                    section = section.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().danger)
                            .child(overwrite_error("No se ha podido leer el antepasado.", error)),
                    );
                }
                section.into_any_element()
            },
        };
        v_flex()
            .size_full()
            .gap(px(16.))
            .px(px(24.))
            .py(px(20.))
            .text_size(px(13.))
            .child(header)
            .child(content)
            .when(self.busy, |d| d.child(gpui_component::skeleton::Skeleton::new().w_full().h(px(8.))))
    }
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

fn change_badge(change: OverwriteChange) -> &'static str {
    match change {
        OverwriteChange::Added | OverwriteChange::AddedDisabled => "Añadido",
        OverwriteChange::Removed => "Eliminado",
        OverwriteChange::Disabled | OverwriteChange::ModifiedAndDisabled => "Desactivado",
        OverwriteChange::Enabled | OverwriteChange::ModifiedAndEnabled => "Activado",
        OverwriteChange::Modified => "Modificado",
    }
}

fn filter_change(change: OverwriteChange, filter: usize) -> bool {
    match filter {
        2 => matches!(change, OverwriteChange::Added | OverwriteChange::AddedDisabled),
        3 => change == OverwriteChange::Removed,
        _ => true,
    }
}
fn overwrite_error(message: &str, detail: &str) -> String {
    let detail = detail
        .split_whitespace()
        .map(|part| if part.contains("://") { "[URL omitted]" } else { part })
        .collect::<Vec<_>>()
        .join(" ");
    log::warn!("Instance overwrite operation failed: {detail}");
    message.to_owned()
}
