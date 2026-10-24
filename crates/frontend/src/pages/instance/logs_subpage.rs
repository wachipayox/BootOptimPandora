use std::{path::Path, sync::Arc};

use bridge::{
    handle::BackendHandle,
    instance::InstanceID,
    message::{LogFileInfo, LogFiles, MessageToBackend},
};
use gpui::{prelude::*, *};
use gpui_component::{
    IndexPath,
    button::Button,
    h_flex,
    scroll::ScrollableElement,
    select::{SelectEvent, SelectState},
    spinner::Spinner,
    v_flex,
};

use crate::{
    component::design::{self, DesignButton},
    component::{
        named_dropdown::{DropdownName, NamedDropdown, NamedDropdownItem},
        readonly_text_field::{ReadonlyTextField, ReadonlyTextFieldWithControls},
    },
    entity::instance::InstanceEntry,
    root, theme,
};

pub struct InstanceLogsSubpage {
    instance: InstanceID,
    backend_handle: BackendHandle,
    log_content: Option<Entity<ReadonlyTextFieldWithControls>>,
    loaded: bool,
    files: Vec<LogFileInfo>,
    filter: LogFilter,
    available_logs: Option<Entity<SelectState<NamedDropdown<Arc<Path>>>>>,
    clean_old_logs_text: Option<SharedString>,
    last_selected_path: Option<Arc<Path>>,
    _read_log_task: Option<Task<()>>,
    _get_log_files_task: Task<()>,
    _dropdown_change_subscrption: Option<Subscription>,
}

impl InstanceLogsSubpage {
    pub fn new(
        instance: &Entity<InstanceEntry>,
        backend_handle: BackendHandle,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> Self {
        let instance = instance.read(cx);
        let instance_id = instance.id;

        let mut this = Self {
            instance: instance_id,
            backend_handle,
            log_content: None,
            loaded: false,
            files: Vec::new(),
            filter: LogFilter::All,
            available_logs: None,
            clean_old_logs_text: None,
            last_selected_path: None,
            _read_log_task: None,
            _get_log_files_task: Task::ready(()),
            _dropdown_change_subscrption: None,
        };

        this.get_log_files(window, cx);

        this
    }
}

impl InstanceLogsSubpage {
    pub fn get_log_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.loaded = false;
        self.files.clear();
        self.log_content = None;
        self.available_logs = None;
        self.clean_old_logs_text = None;
        self.last_selected_path = None;
        self._read_log_task = None;
        self._dropdown_change_subscrption = None;

        let (send, recv) = tokio::sync::oneshot::channel();
        self._get_log_files_task = cx.spawn_in(window, async move |page, cx| {
            let result: LogFiles = recv.await.unwrap_or_default();
            let _ = page.update_in(cx, move |page, window, cx| {
                page.loaded = true;
                page.files = result.files;
                if !page.files.is_empty() {
                    let items = page
                        .files
                        .iter()
                        .filter_map(|file| {
                            let path = file.path.clone();
                            Some(NamedDropdownItem {
                                name: DropdownName::new(Arc::from(path.file_name()?.to_string_lossy())),
                                item: path,
                            })
                        })
                        .collect();

                    let dropdown = NamedDropdown::create(items, window, cx);

                    let _dropdown_change_subscrption = cx.subscribe_in(
                        &dropdown,
                        window,
                        move |page, entity, _: &SelectEvent<NamedDropdown<Arc<Path>>>, window, cx| {
                            let selected = entity.read(cx).selected_value().cloned();

                            if selected == page.last_selected_path {
                                return;
                            }
                            page.last_selected_path = selected.clone();

                            if let Some(selected) = selected {
                                let (send, mut recv) = tokio::sync::mpsc::channel::<Arc<str>>(256);

                                let text_field = cx.new(move |_| ReadonlyTextField::default());

                                let text_field2 = text_field.clone();
                                page._read_log_task = Some(cx.spawn(async move |_, cx| {
                                    while let Some(message) = recv.recv().await {
                                        let _ = cx.update_entity(&text_field2, |text_field, _| {
                                            text_field.add(message);
                                        });
                                    }
                                    let _ = cx.update_entity(&text_field2, |text_field, _| {
                                        text_field.shrink_to_fit();
                                    });
                                }));

                                page.backend_handle.send(MessageToBackend::ReadLog {
                                    path: selected.clone(),
                                    send,
                                });

                                let backend_handle = page.backend_handle.clone();
                                page.log_content = Some(cx.new(move |cx| {
                                    ReadonlyTextFieldWithControls::new(
                                        text_field,
                                        Box::new(move |div| {
                                            let backend_handle = backend_handle.clone();
                                            let selected = selected.clone();
                                            div.child(Button::new("upload").label("Subir y copiar enlace").on_click(
                                                move |_, window, cx| {
                                                    root::upload_log_file(
                                                        selected.clone(),
                                                        &backend_handle,
                                                        window,
                                                        cx,
                                                    );
                                                },
                                            ))
                                        }),
                                        window,
                                        cx,
                                    )
                                }));
                            } else {
                                page._read_log_task = None;
                                page.log_content = None;
                            }

                            cx.notify();
                        },
                    );

                    page._dropdown_change_subscrption = Some(_dropdown_change_subscrption);
                    dropdown.update(cx, |state, c| state.set_selected_index(Some(IndexPath::new(0)), window, c));
                    page.available_logs = Some(dropdown);

                    if result.total_gzipped_size > 0 {
                        let bytes = result.total_gzipped_size;
                        let size = if bytes >= 1_000_000 {
                            format!("{} MB", bytes / 1_000_000)
                        } else if bytes >= 1_000 {
                            format!("{} kB", bytes / 1_000)
                        } else {
                            format!("{bytes} B")
                        };
                        let string = format!("Limpiar registros antiguos ({size})");
                        page.clean_old_logs_text = Some(string.into());
                    }
                }
                cx.notify();
            });
        });

        self.backend_handle.send(MessageToBackend::GetLogFiles {
            instance: self.instance,
            channel: send,
        });
    }
}

#[derive(Clone, Copy, PartialEq)]
enum LogFilter {
    All,
    Logs,
    Crashes,
}
impl Render for InstanceLogsSubpage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut filters = h_flex().gap(px(2.)).p(px(10.));
        for (ix, label, value) in [
            (0, "Todo", LogFilter::All),
            (1, "Logs", LogFilter::Logs),
            (2, "Crashes", LogFilter::Crashes),
        ] {
            filters = filters.child(design::chip(("log-filter", ix as usize), label, self.filter == value).on_click(
                cx.listener(move |this, _, _, c| {
                    this.filter = value;
                    c.notify();
                }),
            ));
        }
        let mut list = v_flex()
            .w(px(260.))
            .h_full()
            .flex_shrink_0()
            .bg(rgb(theme::PANEL))
            .border_1()
            .border_color(rgba(0xffffff0b))
            .child(
                h_flex()
                    .px(px(14.))
                    .h(px(40.))
                    .justify_between()
                    .bg(rgb(theme::PANEL_RAISED))
                    .child(div().font_family(theme::DISPLAY).child("Archivos"))
                    .child(div().text_size(px(12.)).text_color(rgb(theme::INK_MUTED)).child("Más nuevo primero")),
            )
            .child(filters);
        let visible = self
            .files
            .iter()
            .filter(|file| match self.filter {
                LogFilter::All => true,
                LogFilter::Logs => !file.crash,
                LogFilter::Crashes => file.crash,
            })
            .collect::<Vec<_>>();
        let mut rows = v_flex().flex_1().min_h_0().overflow_y_scrollbar();
        if visible.is_empty() && self.loaded {
            rows = rows.child(div().w_full().py(px(24.)).text_center().text_color(rgb(theme::INK_MUTED)).child(
                match self.filter {
                    LogFilter::All => "No hay registros.",
                    LogFilter::Logs => "No hay logs.",
                    LogFilter::Crashes => "No hay crashes.",
                },
            ));
        }
        for (ix, file) in visible.iter().enumerate() {
            let path = file.path.clone();
            let selected = self.last_selected_path.as_ref() == Some(&path);
            let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
            let date = chrono::DateTime::from_timestamp_millis(file.modified_unix_ms.min(i64::MAX as u64) as i64)
                .map(|d| d.with_timezone(&chrono::Local).format("%d/%m/%Y, %H:%M").to_string())
                .unwrap_or_default();
            let detail = if file.launcher {
                format!("Del launcher · {date}")
            } else {
                format!("{date} · {} kB", file.size / 1000)
            };
            rows = rows.child(
                DesignButton::new(("log-file", ix))
                    .w_full()
                    .h_auto()
                    .min_h(px(49.))
                    .px(px(14.))
                    .py(px(7.))
                    .when(selected, |d| d.bg(rgb(theme::ACCENT_SOFT)))
                    .child(
                        h_flex()
                            .w_full()
                            .gap(px(8.))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap(px(3.))
                                    .child(
                                        div()
                                            .w_full()
                                            .text_left()
                                            .text_size(px(13.))
                                            .line_clamp(1)
                                            .text_color(rgb(if file.crash { theme::DANGER } else { theme::INK }))
                                            .child(name),
                                    )
                                    .child(div().text_size(px(11.)).text_color(rgb(theme::INK_MUTED)).child(detail)),
                            )
                            .when(file.crash, |d| {
                                d.child(div().text_size(px(9.)).text_color(rgb(theme::DANGER)).child("CRASH"))
                            }),
                    )
                    .border_b_1()
                    .border_color(rgba(0xffffff0b))
                    .on_click(cx.listener(move |this, _, w, c| {
                        if let Some(state) = &this.available_logs {
                            state.update(c, |s, c| s.set_selected_value(&path, w, c));
                        }
                    })),
            );
        }
        if !self.loaded {
            rows = rows.child(h_flex().w_full().justify_center().py(px(24.)).child(Spinner::new()));
        }
        list = list.child(rows);
        let mut toolbar = h_flex().w_full().justify_end();
        if let Some(label) = self.clean_old_logs_text.clone() {
            toolbar = toolbar.child(DesignButton::new("clean-old-logs").label(label).on_click(cx.listener(
                |this, _, w, c| {
                    this.backend_handle.send(MessageToBackend::CleanupOldLogFiles {
                        instance: this.instance,
                    });
                    this.get_log_files(w, c);
                },
            )));
        }
        let reader = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(rgb(theme::PANEL_SUNKEN))
            .border_1()
            .border_color(rgba(0xffffff0b))
            .when_some(self.log_content.clone(), |d, c| d.child(c));
        v_flex()
            .size_full()
            .px(px(24.))
            .pb(px(24.))
            .gap(px(12.))
            .child(toolbar)
            .child(h_flex().flex_1().min_h_0().items_start().gap(px(16.)).child(list).child(reader))
    }
}
