use std::{path::Path, sync::Arc};

use bridge::{
    handle::BackendHandle,
    instance::InstanceID,
    message::{LogFileInfo, LogFiles, MessageToBackend},
};
use gpui::{prelude::*, *};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex,
    scroll::ScrollableElement,
    spinner::Spinner,
    v_flex,
};

use crate::{
    component::{
        design::{self, DesignButton},
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
    clean_old_logs_text: Option<SharedString>,
    last_selected_path: Option<Arc<Path>>,
    _read_log_task: Option<Task<()>>,
    _get_log_files_task: Task<()>,
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
            clean_old_logs_text: None,
            last_selected_path: None,
            _read_log_task: None,
            _get_log_files_task: Task::ready(()),
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
        self.clean_old_logs_text = None;
        self.last_selected_path = None;
        self._read_log_task = None;
        let (send, recv) = tokio::sync::oneshot::channel();
        self._get_log_files_task = cx.spawn_in(window, async move |this, cx| {
            let result: LogFiles = recv.await.unwrap_or_default();
            let _ = this.update_in(cx, |this, w, c| {
                this.loaded = true;
                this.files = result.files;
                let bytes = result.total_gzipped_size;
                if bytes > 0 {
                    let size = if bytes >= 1_000_000 {
                        format!("{} MB", bytes / 1_000_000)
                    } else if bytes >= 1000 {
                        format!("{} kB", bytes / 1000)
                    } else {
                        format!("{bytes} B")
                    };
                    this.clean_old_logs_text = Some(format!("Limpiar registros antiguos ({size})").into());
                }
                this.select_first(w, c);
                c.notify();
            });
        });
        self.backend_handle.send(MessageToBackend::GetLogFiles {
            instance: self.instance,
            channel: send,
        });
    }
    fn select_first(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.files.iter().find(|f| self.filter.matches(f)).map(|f| f.path.clone());
        if let Some(path) = selected {
            self.select_file(path, window, cx);
        } else {
            self.last_selected_path = None;
            self.log_content = None;
            self._read_log_task = None;
        }
    }
    fn select_file(&mut self, path: Arc<Path>, window: &mut Window, cx: &mut Context<Self>) {
        if self.last_selected_path.as_ref() == Some(&path) {
            return;
        }
        self.last_selected_path = Some(path.clone());
        let (send, mut receive) = tokio::sync::mpsc::channel::<Arc<str>>(256);
        let text = cx.new(|_| ReadonlyTextField::default().with_font_family("Consolas".into()));
        let reader = text.clone();
        self._read_log_task = Some(cx.spawn(async move |_, cx| {
            while let Some(line) = receive.recv().await {
                let _ = cx.update_entity(&reader, |text, c| {
                    text.add(line);
                    c.notify();
                });
            }
            let _ = cx.update_entity(&reader, |text, c| {
                text.shrink_to_fit();
                c.notify();
            });
        }));
        self.backend_handle.send(MessageToBackend::ReadLog {
            path: path.clone(),
            send,
        });
        let backend = self.backend_handle.clone();
        self.log_content = Some(cx.new(move |c| {
            ReadonlyTextFieldWithControls::new_designed(
                text,
                Box::new(move |div| {
                    let backend = backend.clone();
                    let path = path.clone();
                    div.child(
                        Button::new("upload")
                            .ghost()
                            .h(px(36.))
                            .text_color(rgb(theme::INK_MUTED))
                            .label("Subir y copiar enlace")
                            .on_click(move |_, w, c| root::upload_log_file(path.clone(), &backend, w, c)),
                    )
                }),
                window,
                c,
            )
        }));
        cx.notify();
    }
}

#[derive(Clone, Copy, PartialEq)]
enum LogFilter {
    All,
    Logs,
    Crashes,
}
impl LogFilter {
    fn matches(self, file: &LogFileInfo) -> bool {
        match self {
            Self::All => true,
            Self::Logs => !file.crash,
            Self::Crashes => file.crash,
        }
    }
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
                cx.listener(move |this, _, w, c| {
                    this.filter = value;
                    this.select_first(w, c);
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
                        this.select_file(path.clone(), w, c);
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
