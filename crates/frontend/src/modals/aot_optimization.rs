//! "Optimizing launch": the JDK 25 AOT cache of an instance is being built after its game closed.
//! The progress is an estimate made by the backend (Java reports none).

use std::rc::Rc;

use bridge::{
    instance::{InstanceID, InstanceStatus},
    message::{AotOptimizationPhase, MessageToBackend},
};
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    h_flex,
    tooltip::Tooltip,
    v_flex,
};

use crate::{
    component::progress_bar::ProgressBar,
    entity::{DataEntities, instance::InstanceEntry},
    icon::PandoraIcon,
};

/// The instance's status button while it optimizes; opens the progress dialog.
pub fn status_button(id: impl Into<ElementId>, instance_id: InstanceID, data: &DataEntities) -> Button {
    let data = data.clone();
    Button::new(id)
        .info()
        .icon(PandoraIcon::Loader)
        .label(t::instance::aot::optimizing())
        .on_click(move |_, window, cx| open(instance_id, &data, window, cx))
}

/// A small "?" whose explanation opens in a box of fixed width, wrapped and split into paragraphs
/// at blank lines (a button's own tooltip stays on one line).
pub fn help_mark(id: impl Into<ElementId>, text: impl Into<SharedString>) -> impl IntoElement {
    let text: SharedString = text.into();
    div().id(id).child(Button::new("help").label("?").ghost().small()).tooltip(move |window, cx| {
        let text = text.clone();
        Tooltip::element(move |_, _| {
            v_flex()
                .w(px(380.0))
                .p_1()
                .gap_2()
                .text_sm()
                .children(text.split("\n\n").map(|paragraph| div().child(paragraph.to_owned())))
        })
        .build(window, cx)
    })
}

struct ProgressView {
    instance: Entity<InstanceEntry>,
    /// Closes the dialog when the optimization ends; dropped with the dialog.
    _close_when_done: Option<Subscription>,
}

impl Render for ProgressView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entry = self.instance.read(cx);
        let (amount, phase) = entry.optimization.unwrap_or((0.0, AotOptimizationPhase::Recording));
        let phase = match phase {
            AotOptimizationPhase::Recording => t::instance::aot::recording(),
            AotOptimizationPhase::Creating => t::instance::aot::creating(),
        };
        let percent = format!("{}", (amount * 100.0).round() as u32);
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(div().flex_1().child(phase))
                    .child(div().text_sm().child(t::instance::aot::percent(&percent))),
            )
            .child(ProgressBar {
                amount,
                ..ProgressBar::new()
            })
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t::instance::aot::approximate()),
            )
    }
}

/// Opens the progress dialog of an optimizing instance. It closes by itself when the
/// optimization ends.
pub fn open(id: InstanceID, data: &DataEntities, window: &mut Window, cx: &mut App) {
    let Some(instance) = data.instances.read(cx).entries.get(&id).cloned() else {
        return;
    };
    let view = cx.new(|cx| {
        cx.observe(&instance, |_, _, cx| cx.notify()).detach();
        ProgressView {
            instance: instance.clone(),
            _close_when_done: None,
        }
    });
    let handle = window.window_handle();
    let close_when_done = cx.observe(&instance, move |instance, cx| {
        if instance.read(cx).status != InstanceStatus::Optimizing {
            _ = handle.update(cx, |_, window, cx| window.close_dialog(cx));
        }
    });
    view.update(cx, |view, _| view._close_when_done = Some(close_when_done));

    let backend_handle = data.backend_handle.clone();
    window.open_dialog(cx, move |dialog, _, _| {
        let backend_handle = backend_handle.clone();
        dialog
            .title(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(t::instance::aot::optimizing())
                    .child(help_mark("aot-optimization-help", t::instance::aot::help())),
            )
            .overlay_closable(true)
            .child(view.clone())
            .child(div().h_2())
            .child(div().text_sm().child(t::instance::aot::cancel_note()))
            .footer(
                h_flex()
                    .gap_2()
                    .w_full()
                    .child(
                        Button::new("aot-optimization-cancel")
                            .danger()
                            .label(t::instance::aot::cancel())
                            .on_click(move |_, window, cx| {
                                backend_handle.send(MessageToBackend::CancelAotOptimization { id: Some(id) });
                                window.close_dialog(cx);
                            }),
                    )
                    .child(
                        Button::new("aot-optimization-hide")
                            .label(t::instance::aot::hide())
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
            )
    });
}

/// Asks before deleting the AOT cache of an instance.
pub fn confirm_delete(id: InstanceID, data: &DataEntities, window: &mut Window, cx: &mut App) {
    let backend_handle = data.backend_handle.clone();
    window.open_dialog(cx, move |dialog, _, _| {
        let backend_handle = backend_handle.clone();
        dialog
            .title(t::instance::aot::delete_title())
            .overlay_closable(true)
            .child(t::instance::aot::delete_body())
            .footer(
                h_flex()
                    .gap_2()
                    .w_full()
                    .child(
                        Button::new("aot-delete-cancel")
                            .label(t::common::cancel())
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(Button::new("aot-delete-confirm").danger().label(t::instance::aot::delete_confirm()).on_click(
                        move |_, window, cx| {
                            backend_handle.send(MessageToBackend::DeleteAotCache { id });
                            window.close_dialog(cx);
                        },
                    )),
            )
    });
}

/// Whether any instance is optimizing its launch.
pub fn any_optimizing(data: &DataEntities, cx: &App) -> bool {
    data.instances
        .read(cx)
        .entries
        .values()
        .any(|entry| entry.read(cx).status == InstanceStatus::Optimizing)
}

/// Asks before closing the launcher while instances optimize. `close` runs if the player confirms
/// (after their optimizations are cancelled). Returns `true` if nothing optimizes and the caller
/// can close right away.
pub fn confirm_close(
    data: &DataEntities,
    window: &mut Window,
    cx: &mut App,
    close: impl Fn(&mut Window, &mut App) + 'static,
) -> bool {
    if !any_optimizing(data, cx) {
        return true;
    }
    let backend_handle = data.backend_handle.clone();
    let close = Rc::new(close);
    window.open_dialog(cx, move |dialog, _, _| {
        let backend_handle = backend_handle.clone();
        let close = close.clone();
        dialog
            .title(t::instance::aot::quit_title())
            .overlay_closable(false)
            .child(t::instance::aot::quit_body())
            .footer(
                h_flex()
                    .gap_2()
                    .w_full()
                    .child(
                        Button::new("aot-quit-cancel")
                            .label(t::common::cancel())
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(Button::new("aot-quit-confirm").danger().label(t::instance::aot::quit_confirm()).on_click(
                        move |_, window, cx| {
                            backend_handle.send(MessageToBackend::CancelAotOptimization { id: None });
                            window.close_dialog(cx);
                            close(window, cx);
                        },
                    )),
            )
    });
    false
}
