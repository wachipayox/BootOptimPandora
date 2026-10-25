use crate::{component::design, theme};
use bridge::{handle::BackendHandle, instance::InstanceID, message::MessageToBackend};
use gpui::{prelude::*, *};
use gpui_component::{
    Disableable, WindowExt,
    checkbox::Checkbox,
    h_flex,
    input::{InputEvent, InputState},
    v_flex,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub fn open_delete_instance(
    instance: InstanceID,
    instance_name: SharedString,
    backend: BackendHandle,
    window: &mut Window,
    cx: &mut App,
) {
    let acknowledged = Arc::new(AtomicBool::new(false));
    let correct = Arc::new(AtomicBool::new(false));
    let input = cx.new(|cx| InputState::new(window, cx));
    let expected = instance_name.clone();
    let correct_input = correct.clone();
    let subscription = cx.subscribe(&input, move |input, event: &InputEvent, cx| {
        if matches!(event, InputEvent::Change) {
            correct_input.store(input.read(cx).value() == expected, Ordering::Relaxed);
            cx.refresh_windows();
        }
    });
    window.open_dialog(cx, move |dialog, _, _| {
        let _ = &subscription;
        let ack = acknowledged.load(Ordering::Relaxed);
        let can_delete = ack && correct.load(Ordering::Relaxed);
        let checkbox_ack = acknowledged.clone();
        let enter_ack = acknowledged.clone();
        let enter_correct = correct.clone();
        let enter_backend = backend.clone();
        let button_ack = acknowledged.clone();
        let button_correct = correct.clone();
        let button_backend = backend.clone();
        design::dialog(dialog)
            .title(design::dialog_title(format!("Eliminar {instance_name}")))
            .on_ok(move |_, _, _| {
                if !enter_ack.load(Ordering::Relaxed) || !enter_correct.load(Ordering::Relaxed) {
                    return false;
                }
                enter_backend.send(MessageToBackend::DeleteInstance { id: instance });
                true
            })
            .child(
                v_flex()
                    .gap(px(12.))
                    .child(div().text_size(px(13.)).text_color(rgb(theme::INK_MUTED)).child(
                        "Se borran para siempre sus mundos, mods, configuración y capturas. No se puede deshacer.",
                    ))
                    .child(
                        Checkbox::new("delete-understood")
                            .label("He leído y entiendo lo que se borra")
                            .checked(ack)
                            .on_click(move |value, _, cx| {
                                checkbox_ack.store(*value, Ordering::Relaxed);
                                cx.refresh_windows();
                            }),
                    )
                    .child(div().text_size(px(12.)).child(format!("Para confirmar, escribe «{instance_name}»")))
                    .child(design::text_field(&input)),
            )
            .footer(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        design::action_button("cancel-delete", "Cancelar", design::ButtonAction::Secondary)
                            .on_click(|_, w, c| w.close_dialog(c)),
                    )
                    .child(
                        design::action_button("confirm-delete", "Eliminar", design::ButtonAction::Danger)
                            .disabled(!can_delete)
                            .on_click(move |_, w, c| {
                                if !button_ack.load(Ordering::Relaxed) || !button_correct.load(Ordering::Relaxed) {
                                    return;
                                }
                                button_backend.send(MessageToBackend::DeleteInstance { id: instance });
                                w.close_all_dialogs(c);
                            }),
                    ),
            )
    });
}
