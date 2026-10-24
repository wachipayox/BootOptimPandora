use std::{collections::HashSet, sync::Arc};

use bridge::{
    handle::BackendHandle,
    message::{MessageToBackend, SyncState},
    safe_path::SafePath,
};
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _, Disableable,
    button::{Button, ButtonVariants},
    h_flex,
    input::InputState,
    spinner::Spinner,
    v_flex,
};
use once_cell::sync::Lazy;
use rustc_hash::FxHashSet;

use crate::{component::design, entity::DataEntities, icon::PandoraIcon, pages::page::Page, theme};

pub struct SyncingPage {
    backend_handle: BackendHandle,
    sync_state: Option<SyncState>,
    pending: FxHashSet<Arc<str>>,
    loading: FxHashSet<Arc<str>>,
    custom_input_state: Entity<InputState>,
    _get_sync_state_task: Task<()>,
}

impl SyncingPage {
    pub fn new(data: &DataEntities, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut page = Self {
            backend_handle: data.backend_handle.clone(),
            sync_state: None,
            pending: FxHashSet::default(),
            loading: FxHashSet::default(),
            custom_input_state: cx.new(|cx| InputState::new(window, cx)),
            _get_sync_state_task: Task::ready(()),
        };

        page.update_sync_state(cx);

        page
    }
}

impl SyncingPage {
    pub fn update_sync_state(&mut self, cx: &mut Context<Self>) {
        let (send, recv) = tokio::sync::oneshot::channel();
        self._get_sync_state_task = cx.spawn(async move |page, cx| {
            let Ok(result): Result<SyncState, _> = recv.await else {
                return;
            };
            let _ = page.update(cx, move |page, cx| {
                page.loading.retain(|loading| !page.pending.contains(loading));
                page.pending = FxHashSet::default();
                page.sync_state = Some(result);
                cx.notify();

                if !page.loading.is_empty() {
                    page.pending = page.loading.clone();
                    page.update_sync_state(cx);
                }
            });
        });

        self.backend_handle.send(MessageToBackend::GetSyncState { channel: send });
    }

    pub fn create_entry(
        &self,
        sync_state: &SyncState,
        name: Arc<str>,
        is_file: bool,
        label: SharedString,
        warning: Hsla,
        info: Hsla,
        cx: &mut Context<Self>,
    ) -> Div {
        let synced_count;
        let cannot_sync_count;
        let enabled;
        if let Some(sync_target_state) = sync_state.targets.get(&name)
            && sync_target_state.is_file == is_file
        {
            synced_count = sync_target_state.sync_count;
            cannot_sync_count = sync_target_state.cannot_sync_count;
            enabled = sync_target_state.enabled;
        } else {
            synced_count = 0;
            cannot_sync_count = 0;
            enabled = false;
        }
        let disabled = !enabled && cannot_sync_count > 0;
        let is_loading = self.loading.contains(&name);

        let backend_handle = self.backend_handle.clone();
        let target_name = name.clone();
        let switch = design::YesNo::new(name.clone()).disabled(disabled).checked(enabled).on_click(cx.listener(
            move |page, value, _, cx| {
                backend_handle.send(MessageToBackend::SetSyncing {
                    target: target_name.clone(),
                    is_file,
                    value: *value,
                });
                page.loading.insert(target_name.clone());
                if page.pending.is_empty() {
                    page.pending.insert(target_name.clone());
                    page.update_sync_state(cx);
                }
            },
        ));
        let mut title = v_flex().flex_1().min_w_0().child(label);
        if (enabled || synced_count > 0) && !is_file {
            title = title.child(
                div()
                    .text_size(px(10.))
                    .text_color(info)
                    .child(format!("{synced_count} de {} instancias sincronizadas", sync_state.total_count)),
            );
        }
        let mut base = h_flex().w_full().min_h(px(49.)).gap(px(8.)).items_center().child(title);
        if cannot_sync_count > 0 {
            let names = sync_state
                .targets
                .get(&name)
                .map(|s| s.cannot_sync_instances.join("\n"))
                .unwrap_or_default();
            let tooltip = format!(
                "La ruta ya existe en {cannot_sync_count} instancias. Haz una copia antes de sincronizarla.\n{names}"
            );
            base = base.child(
                Button::new(SharedString::from(format!("sync-warning-{name}")))
                    .ghost()
                    .rounded(px(0.))
                    .compact()
                    .icon(PandoraIcon::TriangleAlert)
                    .text_color(warning)
                    .tooltip(tooltip)
                    .label(format!(
                        "No se puede sincronizar en {cannot_sync_count} de {} instancias",
                        sync_state.total_count
                    )),
            );
        }
        if is_loading {
            base = base.child(Spinner::new());
        }
        base = base.child(switch);

        base
    }
}

impl Page for SyncingPage {
    fn controls(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }

    fn scrollable(&self, _cx: &App) -> bool {
        true
    }
}

impl Render for SyncingPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(sync_state) = &self.sync_state else {
            return v_flex().w_full().p(px(24.)).child(Spinner::new());
        };
        let sync_folder = sync_state.sync_folder.clone();
        let enabled = sync_state.targets.values().filter(|target| target.enabled).count();
        let count = NAMED_SYNC_TARGETS.len()
            + sync_state.targets.keys().filter(|name| !NAMED_SYNC_TARGETS.contains(&***name)).count();
        let groups: &[(&str, &[(&str, &str, bool)])] = &[
            (
                "Archivos",
                &[
                    ("options.txt", "options.txt", true),
                    ("servers.dat", "servers.dat", true),
                    ("command_history.txt", "command_history.txt", true),
                    ("hotbar.nbt", "hotbar.nbt", true),
                ],
            ),
            (
                "Carpetas",
                &[
                    ("saves", "Mundos (saves)", false),
                    ("config", "config", false),
                    ("screenshots", "Capturas", false),
                    ("resourcepacks", "Paquetes de recursos", false),
                    ("downloads", "Recursos de servidor (downloads)", false),
                    ("shaderpacks", "Shaders", false),
                ],
            ),
            (
                "Mods",
                &[
                    ("Distant_Horizons_server_data", "Distant Horizons", false),
                    ("xaero", "Xaero's Minimap", false),
                    ("journeymap", "JourneyMap", false),
                    ("flashback", "Flashback", false),
                    (".bobby", "Bobby", false),
                    (".voxy", "Voxy", false),
                    ("schematics", "Litematica", false),
                ],
            ),
        ];
        let mut columns = h_flex().w_full().items_start().gap(px(16.));
        for (heading, entries) in groups {
            let mut panel = v_flex()
                .flex_1()
                .min_w_0()
                .p(px(18.))
                .gap(px(4.))
                .bg(rgb(theme::PANEL))
                .border_1()
                .border_color(rgba(0xffffff0b))
                .child(
                    div()
                        .font_family(theme::DISPLAY)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(px(15.))
                        .mb(px(8.))
                        .child(*heading),
                );
            for (name, label, file) in *entries {
                panel = panel.child(self.create_entry(
                    sync_state,
                    (*name).into(),
                    *file,
                    (*label).into(),
                    cx.theme().warning,
                    rgb(theme::INK_MUTED).into(),
                    cx,
                ));
            }
            columns = columns.child(panel);
        }
        let mut custom = v_flex().w_full().gap(px(8.));
        for (name, state) in &sync_state.targets {
            if NAMED_SYNC_TARGETS.contains(&**name) || !state.enabled {
                continue;
            }
            custom = custom.child(self.create_entry(
                sync_state,
                name.clone(),
                state.is_file,
                name.to_string().into(),
                cx.theme().warning,
                rgb(theme::INK_MUTED).into(),
                cx,
            ));
        }
        let mut actions = h_flex()
            .w_full()
            .gap(px(8.))
            .child(design::text_field(&self.custom_input_state).w(px(260.)));
        for (index, label, file) in [(0usize, "Archivo propio…", true), (1, "Carpeta propia…", false)] {
            actions = actions.child(design::DesignButton::new(("custom-sync", index)).label(label).on_click(
                cx.listener(move |page, _, window, cx| {
                    let input = page.custom_input_state.read(cx).value();
                    let input = input.as_str().trim_ascii();
                    if SafePath::new(input).is_none() {
                        return;
                    }
                    let name: Arc<str> = input.into();
                    page.backend_handle.send(MessageToBackend::SetSyncing {
                        target: name.clone(),
                        is_file: file,
                        value: true,
                    });
                    page.loading.insert(name.clone());
                    if page.pending.is_empty() {
                        page.pending.insert(name);
                        page.update_sync_state(cx);
                    }
                    page.custom_input_state.update(cx, |state, cx| state.set_value("", window, cx));
                }),
            ));
        }
        v_flex().w_full().p(px(24.)).gap(px(20.)).text_size(px(13.))
            .child(h_flex().w_full().gap(px(12.)).justify_between()
                .child(div().text_size(px(12.)).text_color(rgb(theme::INK_MUTED)).child(format!("Comparte archivos y carpetas entre todos tus instancias. {enabled} de {count} sincronizados.")))
                .child(design::DesignButton::new("open-sync-folder").ghost().label("Abrir carpeta sincronizada")
                    .on_click(move |_,w,c|crate::open_folder(&sync_folder,w,c))))
            .child(columns).child(custom).child(actions)
            .child(div().text_size(px(11.)).text_color(rgb(theme::INK_DISABLED))
                .child("Si una instancia ya tiene esa carpeta, se avisa para que hagas copia antes. «Desactivar sincronización» en el propio instancia lo excluye."))
    }
}

static NAMED_SYNC_TARGETS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    HashSet::from([
        "options.txt",
        "servers.dat",
        "command_history.txt",
        "hotbar.nbt",
        "saves",
        "config",
        "screenshots",
        "resourcepacks",
        "downloads",
        "shaderpacks",
        "flashback",
        "Distant_Horizons_server_data",
        ".voxy",
        "xaero",
        "journeymap",
        ".bobby",
        "schematics",
    ])
});
