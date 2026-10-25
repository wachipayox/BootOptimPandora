use crate::{component::design, theme};
use bridge::{
    handle::BackendHandle,
    message::{MessageToBackend, SkinImportPlan},
};
use gpui::{prelude::*, *};
use gpui_component::{Disableable, WindowExt, h_flex, spinner::Spinner, v_flex};

struct SkinImportDialog {
    backend: BackendHandle,
    plan: Option<SkinImportPlan>,
    failed: bool,
    busy: bool,
    task: Option<Task<()>>,
}
pub fn open(backend: BackendHandle, window: &mut Window, cx: &mut App) {
    let entity = cx.new(|cx| {
        let (send, receive) = tokio::sync::oneshot::channel();
        backend.send(MessageToBackend::DiscoverLauncherSkins { result: send });
        let task = cx.spawn(async move |entity, cx| {
            let result = receive.await;
            _ = entity.update(cx, |dialog: &mut SkinImportDialog, cx| {
                match result {
                    Ok(Ok(plan)) => dialog.plan = Some(plan),
                    other => {
                        log::warn!("Skin discovery failed: {other:?}");
                        dialog.failed = true;
                    },
                }
                dialog.busy = false;
                cx.notify();
            });
        });
        SkinImportDialog {
            backend,
            plan: None,
            failed: false,
            busy: true,
            task: Some(task),
        }
    });
    window.open_dialog(cx, move |dialog, _, _| {
        design::dialog(dialog)
            .title(design::dialog_title("Importar de launcher"))
            .child(entity.clone())
    });
}
impl Render for SkinImportDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut body = v_flex().w_full().gap(px(12.)).text_size(px(13.)).child(
            div()
                .text_color(rgb(theme::INK_MUTED))
                .child("Aspectos encontrados en otros launchers de este equipo."),
        );
        let mut count = 0;
        if let Some(plan) = &self.plan {
            count = plan.candidates.len();
            for source in &plan.sources {
                body = body.child(
                    v_flex()
                        .gap(px(4.))
                        .py(px(8.))
                        .border_b_1()
                        .border_color(rgba(0xffffff0b))
                        .child(source.label.clone())
                        .child(div().text_size(px(12.)).text_color(rgb(theme::INK_MUTED)).child(format!(
                            "{} aspectos · {} repetidos no se importan",
                            source.found, source.repeated
                        ))),
                );
            }
            body = body.child(
                h_flex().justify_between().child(format!("{count} aspectos nuevos")).child(
                    div()
                        .text_color(rgb(theme::INK_MUTED))
                        .child(format!("{} repetidos no se importan", plan.repeated)),
                ),
            );
        }
        if self.failed {
            body = body.child(div().text_color(rgb(theme::INK_MUTED)).child("No se han podido importar los aspectos."));
        }
        if self.busy {
            body = body.child(Spinner::new());
        }
        body.child(
            h_flex()
                .w_full()
                .justify_end()
                .gap(px(8.))
                .pt(px(12.))
                .child(
                    design::action_button("cancel-skin-import", "Cancelar", design::ButtonAction::Secondary)
                        .on_click(|_, w, c| w.close_dialog(c)),
                )
                .child(
                    design::action_button(
                        "confirm-skin-import",
                        format!("Importar {count}"),
                        design::ButtonAction::Primary,
                    )
                    .disabled(self.busy || count == 0)
                    .on_click(cx.listener(|dialog, _, window, cx| {
                        let Some(plan) = dialog.plan.take() else {
                            return;
                        };
                        dialog.busy = true;
                        let (send, receive) = tokio::sync::oneshot::channel();
                        dialog.backend.send(MessageToBackend::ImportLauncherSkins { plan, result: send });
                        dialog.task = Some(window.spawn(cx, async move |cx| {
                            let result = receive.await;
                            _ = cx.update(|window, cx| match result {
                                Ok(Ok(_)) => window.close_dialog(cx),
                                other => {
                                    log::warn!("Skin import failed: {other:?}");
                                    window.push_notification(
                                        gpui_component::notification::Notification::new()
                                            .with_type(gpui_component::notification::NotificationType::Error)
                                            .title("No se han podido importar los aspectos."),
                                        cx,
                                    );
                                    window.close_dialog(cx);
                                },
                            });
                        }));
                        cx.notify();
                    })),
                ),
        )
    }
}
