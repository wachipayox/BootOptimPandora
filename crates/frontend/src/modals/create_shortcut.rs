use std::{
    io::Cursor,
    path::{Path, PathBuf},
    sync::Arc,
};

use bridge::{
    handle::BackendHandle,
    message::{MessageToBackend, ShortcutError, ShortcutIconChoice},
};
use gpui::{prelude::*, *};
use gpui_component::{Disableable, Icon, WindowExt, dialog::Dialog, h_flex, v_flex};

use crate::{
    component::{
        design::{self, DesignButton},
        path_label::PathLabel,
    },
    entity::instance::InstanceEntry,
    icon::PandoraIcon,
    png_render_cache, theme,
};

struct ShortcutDialog {
    instance: Entity<InstanceEntry>,
    backend: BackendHandle,
    choice: usize,
    custom: Option<PathBuf>,
    busy: bool,
    error: Option<ShortcutError>,
    task: Task<()>,
}

impl ShortcutDialog {
    fn render(&mut self, dialog: Dialog, _window: &mut Window, cx: &mut Context<Self>) -> Dialog {
        let instance = self.instance.read(cx).clone();
        let mut options = v_flex().w_full().gap(px(8.));
        for (index, label) in ["Icono de la instancia", "Icono del launcher", "Imagen propia…"].into_iter().enumerate()
        {
            let preview = match index {
                0 => {
                    if let Some(bytes) = instance.icon.clone() {
                        png_render_cache::render_with_transform(
                            bytes,
                            png_render_cache::ImageTransformation::Resize { width: 32, height: 32 },
                            cx,
                        )
                        .size(px(32.))
                        .into_any_element()
                    } else {
                        Icon::default()
                            .path(
                                instance
                                    .configuration
                                    .instance_fallback_icon
                                    .map(|p| p.as_str())
                                    .unwrap_or("icons/instance-default.svg"),
                            )
                            .size(px(32.))
                            .into_any_element()
                    }
                },
                1 => img(Arc::new(Image::from_bytes(
                    ImageFormat::Png,
                    include_bytes!("../../../../package/windows_icons/icon_256x256.png").to_vec(),
                )))
                .size(px(32.))
                .into_any_element(),
                _ => Icon::new(PandoraIcon::File).size(px(32.)).into_any_element(),
            };
            let description =
                v_flex().flex_1().min_w_0().items_start().gap(px(3.)).child(label).when(index == 2, |d| {
                    d.when_some(self.custom.clone(), |d, path| {
                        d.child(
                            div()
                                .w_full()
                                .text_size(px(11.))
                                .text_color(rgb(theme::INK_MUTED))
                                .child(PathLabel::new(path, false)),
                        )
                    })
                });
            options = options.child(
                DesignButton::new(("shortcut-choice", index))
                    .aria_label(label)
                    .w_full()
                    .h(px(60.))
                    .justify_start()
                    .toggled(self.choice == index)
                    .disabled(self.busy)
                    .child(h_flex().w_full().gap(px(12.)).items_center().child(preview).child(description))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if index == 2 {
                            this.choose_custom(window, cx);
                        } else {
                            this.choice = index;
                            this.error = None;
                            cx.notify();
                        }
                    })),
            );
        }
        let weak = cx.entity().downgrade();
        let busy = self.busy;
        design::dialog(dialog)
            .w(px(520.))
            .title(design::dialog_title("Crear acceso directo"))
            .overlay_closable(false)
            .on_cancel(move |_, _, _| !busy)
            .on_ok(move |_, window, cx| {
                _ = weak.update(cx, |this, cx| this.create(window, cx));
                false
            })
            .child(options)
            .when_some(self.error, |dialog, error| {
                dialog.child(div().text_size(px(12.)).text_color(rgb(theme::DANGER)).child(match error {
                    ShortcutError::Image => "No se ha podido abrir la imagen.",
                    ShortcutError::Create => "No se ha podido crear el acceso directo.",
                }))
            })
            .footer(
                h_flex()
                    .w_full()
                    .gap(px(8.))
                    .justify_end()
                    .child(
                        DesignButton::new("shortcut-cancel")
                            .label("Cancelar")
                            .disabled(self.busy)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        design::action_button("shortcut-create", "Crear acceso directo", design::ButtonAction::Primary)
                            .disabled(self.busy)
                            .on_click(cx.listener(|this, _, window, cx| this.create(window, cx))),
                    ),
            )
    }

    fn choose_custom(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Imagen propia…".into()),
        });
        self.busy = true;
        let weak = cx.entity().downgrade();
        self.task = window.spawn(cx, async move |cx| {
            let response = receiver.await;
            _ = cx.update(|_, cx| {
                _ = weak.update(cx, |this, cx| {
                    this.busy = false;
                    match response {
                        Ok(Ok(Some(paths))) => {
                            if let Some(path) = paths.into_iter().next() {
                                this.custom = Some(path);
                                this.choice = 2;
                                this.error = None;
                            }
                        },
                        Ok(Ok(None)) => {},
                        _ => this.error = Some(ShortcutError::Image),
                    }
                    cx.notify();
                });
            });
        });
        cx.notify();
    }

    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let instance = self.instance.read(cx).clone();
        let icon = match self.choice {
            0 => match instance_icon_png(&instance, cx) {
                Ok(bytes) => ShortcutIconChoice::Instance(bytes),
                Err(error) => {
                    self.error = Some(error);
                    cx.notify();
                    return;
                },
            },
            1 => ShortcutIconChoice::Launcher,
            _ => match self.custom.clone() {
                Some(path) => ShortcutIconChoice::Custom(path),
                None => {
                    self.error = Some(ShortcutError::Image);
                    cx.notify();
                    return;
                },
            },
        };
        let user_dirs = directories::UserDirs::new();
        let directory = user_dirs.as_ref().and_then(directories::UserDirs::desktop_dir).unwrap_or(Path::new("."));
        #[cfg(target_os = "windows")]
        let suffix = "lnk";
        #[cfg(target_os = "linux")]
        let suffix = "desktop";
        #[cfg(target_os = "macos")]
        let suffix = "app";
        let receiver = cx.prompt_for_new_path(directory, Some(&format!("{}.{suffix}", instance.name)));
        let backend = self.backend.clone();
        let id = instance.id;
        let weak = cx.entity().downgrade();
        self.busy = true;
        self.error = None;
        self.task = window.spawn(cx, async move |cx| {
            let response = receiver.await;
            let result = match response {
                Ok(Ok(Some(path))) => {
                    let (channel, receive) = tokio::sync::oneshot::channel();
                    backend.send(MessageToBackend::CreateInstanceShortcut {
                        id,
                        path,
                        icon,
                        channel,
                    });
                    Some(receive.await.unwrap_or(Err(ShortcutError::Create)))
                },
                Ok(Ok(None)) => None,
                _ => Some(Err(ShortcutError::Create)),
            };
            _ = cx.update(|window, cx| {
                _ = weak.update(cx, |this, cx| {
                    this.busy = false;
                    match result {
                        Some(Ok(())) => window.close_dialog(cx),
                        Some(Err(error)) => this.error = Some(error),
                        None => {},
                    }
                    cx.notify();
                });
            });
        });
        cx.notify();
    }
}

/// Rasterize embedded fallback vectors on the UI thread, like their normal GPUI rendering.
/// RenderImage is straight BGRA; only swap channels before encoding the PNG.
fn instance_icon_png(instance: &InstanceEntry, cx: &App) -> Result<Arc<[u8]>, ShortcutError> {
    if let Some(bytes) = &instance.icon {
        return Ok(bytes.iter().copied().collect::<Vec<_>>().into());
    }
    let path = instance
        .configuration
        .instance_fallback_icon
        .map(|p| p.as_str())
        .unwrap_or("icons/instance-default.svg");
    let asset = crate::Assets::get(path).ok_or(ShortcutError::Image)?;
    let svg = std::str::from_utf8(&asset.data)
        .map_err(|_| ShortcutError::Image)?
        .replace("currentColor", &format!("#{:06x}", theme::INK));
    let renderer = cx.svg_renderer();
    let parsed = renderer.parse_svg(svg.as_bytes()).map_err(|_| ShortcutError::Image)?;
    let image = renderer
        .render_parsed(&parsed, SvgSize::ExactSize(size(DevicePixels(256), DevicePixels(256))))
        .map_err(|_| ShortcutError::Image)?;
    let mut bytes = image.as_bytes(0).ok_or(ShortcutError::Image)?.to_vec();
    for pixel in bytes.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let image = image::RgbaImage::from_raw(256, 256, bytes).ok_or(ShortcutError::Image)?;
    let mut encoded = Cursor::new(Vec::new());
    image.write_to(&mut encoded, image::ImageFormat::Png).map_err(|_| ShortcutError::Image)?;
    Ok(encoded.into_inner().into())
}

pub fn open(instance: Entity<InstanceEntry>, backend: BackendHandle, window: &mut Window, cx: &mut App) {
    let state = cx.new(|cx| {
        cx.observe(&instance, |_, _, cx| cx.notify()).detach();
        ShortcutDialog {
            instance,
            backend,
            choice: 0,
            custom: None,
            busy: false,
            error: None,
            task: Task::ready(()),
        }
    });
    window.open_dialog(cx, move |dialog, window, cx| {
        cx.update_entity(&state, |this, cx| this.render(dialog, window, cx))
    });
}
