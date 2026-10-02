use std::collections::BTreeSet;

use crate::entity::instance::InstanceEntry;
use bridge::{
    handle::BackendHandle,
    instance::InstanceID,
    message::MessageToBackend,
    profile_overwrites::{OverwriteChange, ProfileOverwritesReport, ProfileTextFile},
};
use gpui::{prelude::*, *};
use gpui_component::{
    button::Button,
    h_flex,
    input::{Textarea, TextareaState},
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
    notice: Option<String>,
    busy: bool,
    _load: Task<()>,
    _read: Task<()>,
    _save: Task<()>,
}

impl InstanceOverwritesSubpage {
    pub fn new(
        instance: &Entity<InstanceEntry>,
        backend: BackendHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut page = Self {
            instance: instance.read(cx).id,
            backend,
            report: None,
            expanded: BTreeSet::new(),
            selected: None,
            text: None,
            editor: cx.new(|cx| TextareaState::new(window, cx).auto_grow(10, 24)),
            notice: None,
            busy: false,
            _load: Task::ready(()),
            _read: Task::ready(()),
            _save: Task::ready(()),
        };
        page.reload(window, cx);
        page
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
        let header = h_flex()
            .justify_between()
            .items_center()
            .child(
                v_flex()
                    .child(div().text_lg().child("Archivos y sobrescrituras"))
                    .child(div().text_sm().child("Explora el modpack y compara sus archivos con sus madres.")),
            )
            .child(
                Button::new("refresh-overwrites")
                    .label("Actualizar lista")
                    .on_click(cx.listener(|page, _, window, cx| page.reload(window, cx))),
            );
        let content = match &self.report {
            None => div().child("Comparando archivos heredados…").into_any_element(),
            Some(Err(error)) => div().child(format!("No se pudo leer el modpack: {error}")).into_any_element(),
            Some(Ok(report)) => {
                let mut nodes = BTreeSet::new();
                let mut folders = BTreeSet::new();
                for path in report
                    .files
                    .iter()
                    .map(|f| &f.path)
                    .chain(
                        report
                            .local_changes
                            .iter()
                            .filter(|c| !report.files.iter().any(|f| f.comparison_path == c.path))
                            .map(|f| &f.path),
                    )
                    .chain(
                        report
                            .global_changes
                            .iter()
                            .filter(|c| !report.files.iter().any(|f| f.comparison_path == c.path))
                            .map(|f| &f.path),
                    )
                {
                    let parts: Vec<_> = path.split('/').collect();
                    for i in 1..parts.len() {
                        let folder = parts[..i].join("/");
                        nodes.insert(folder.clone());
                        folders.insert(folder);
                    }
                    nodes.insert(path.clone());
                }
                let mut tree = v_flex().gap_1();
                for path in nodes {
                    let parent = path.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
                    if !parent.is_empty() && !self.expanded.contains(parent) {
                        continue;
                    }
                    let folder = folders.contains(&path);
                    let depth = path.matches('/').count();
                    let name = path.rsplit('/').next().unwrap_or(&path);
                    let prefix = if folder {
                        if self.expanded.contains(&path) { "▾" } else { "▸" }
                    } else {
                        "·"
                    };
                    let comparison = report
                        .files
                        .iter()
                        .find(|f| f.path == path)
                        .map(|f| f.comparison_path.as_str())
                        .unwrap_or(path.as_str());
                    let changed =
                        report.local_changes.iter().chain(&report.global_changes).any(|c| c.path == comparison);
                    let label = format!("{prefix} {name}{}", if changed { "  ●" } else { "" });
                    let click_path = path.clone();
                    tree = tree.child(div().pl(px((depth * 14) as f32)).child(
                        Button::new(SharedString::from(format!("file-{path}"))).label(label).on_click(cx.listener(
                            move |page, _, window, cx| {
                                if folder {
                                    if !page.expanded.insert(click_path.clone()) {
                                        page.expanded.remove(&click_path);
                                    }
                                    cx.notify();
                                } else {
                                    page.select(click_path.clone(), window, cx);
                                }
                            },
                        )),
                    ));
                }
                if report.files.is_empty() {
                    tree = tree.child(div().child("El modpack está vacío."));
                }
                let mut details = v_flex().gap_3().flex_1();
                if let Some(path) = &self.selected {
                    details = details.child(div().text_lg().child(path.clone()));
                    let selected_file = report.files.iter().find(|file| &file.path == path);
                    let comparison = selected_file.map(|file| file.comparison_path.as_str()).unwrap_or(path.as_str());
                    if let Some(file) = selected_file {
                        details = details.child(div().text_sm().child(format!("{} bytes", file.size)));
                    }
                    if let Some(change) = report.local_changes.iter().find(|c| c.path == comparison) {
                        details = details.child(div().child(format!("Madre local: {}", change_name(change.change))));
                    }
                    if let Some(change) = report.global_changes.iter().find(|c| c.path == comparison) {
                        details = details.child(div().child(format!("Perfil global: {}", change_name(change.change))));
                    }
                    if path.to_ascii_lowercase().starts_with("mods/")
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
                                .on_click(cx.listener(|page, _, window, cx| page.toggle_mod(window, cx))),
                        );
                    }
                    match &self.text {
                        Some(Ok(file)) => {
                            details = details
                                .child(div().text_sm().child("Editor de texto · cambios locales de esta instancia"))
                                .child(Textarea::new(&self.editor))
                                .child(
                                    Button::new("save-overwrite-text")
                                        .label("Guardar archivo")
                                        .on_click(cx.listener(|page, _, window, cx| page.save(window, cx))),
                                );
                            if self.editor.read(cx).value().as_str() != file.contents {
                                details = details.child(div().text_sm().child("Hay cambios sin guardar."));
                            }
                        },
                        Some(Err(error)) => {
                            details = details.child(div().child(format!("Vista previa no disponible: {error}")))
                        },
                        None if report.files.iter().any(|f| &f.path == path && f.editable) => {
                            details = details.child(div().child("Abriendo archivo…"));
                        },
                        None => {
                            details = details.child(
                                div().text_sm().child("Archivo binario o eliminado; edición de texto no disponible."),
                            )
                        },
                    }
                } else {
                    details = details.child(
                        div().child("Selecciona un archivo para ver sus diferencias y editar texto compatible."),
                    );
                }
                if let Some(notice) = &self.notice {
                    details = details.child(div().child(notice.clone()));
                }
                let mut summary = v_flex().gap_1();
                if let Some(parent) = &report.local_parent {
                    summary = summary.child(
                        div()
                            .text_sm()
                            .child(format!("Madre local: {parent} · {} diferencias", report.local_changes.len())),
                    );
                }
                if let Some(profile) = &report.global_profile {
                    summary = summary.child(
                        div()
                            .text_sm()
                            .child(format!("Perfil global: {profile} · {} diferencias", report.global_changes.len())),
                    );
                }
                if let Some(error) = &report.local_error {
                    summary = summary.child(div().child(error.clone()));
                }
                if let Some(error) = &report.global_error {
                    summary = summary.child(div().child(error.clone()));
                }
                if report.skipped_entries > 0 {
                    summary = summary.child(
                        div()
                            .text_sm()
                            .child(format!("{} enlaces o entradas especiales omitidos.", report.skipped_entries)),
                    );
                }
                v_flex()
                    .gap_3()
                    .flex_1()
                    .child(summary)
                    .child(
                        h_flex()
                            .gap_4()
                            .flex_1()
                            .child(v_flex().w_72().overflow_y_scrollbar().child(tree))
                            .child(v_flex().flex_1().overflow_y_scrollbar().child(details)),
                    )
                    .into_any_element()
            },
        };
        v_flex().size_full().gap_4().p_4().child(header).child(content)
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
