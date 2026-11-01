//! Shared signed launch/stop boundary used by Home and instance headers.
use super::design::PlayButton;
use crate::{
    entity::{DataEntities, instance::InstanceEntry},
    root,
};
use bridge::{instance::InstanceStatus, message::MessageToBackend};
use gpui::*;
use gpui_component::Disableable;

pub fn instance_play_button(
    button_id: impl Into<ElementId>,
    i: &InstanceEntry,
    state: InstanceStatus,
    data: &DataEntities,
    window: &mut Window,
    _cx: &mut App,
) -> PlayButton {
    let button_id = button_id.into();
    let mut play = PlayButton::new(button_id.clone(), "Al día", None);
    let data = data.clone();
    let id = i.id;
    let name = i.name.clone();
    match state {
        InstanceStatus::Launching => {
            play = PlayButton::new(button_id.clone(), "Abriendo Minecraft", None)
                .title("Iniciando")
                .disabled(true)
        },
        InstanceStatus::Stopping => {
            play = PlayButton::new(button_id.clone(), "Cerrando Minecraft", None)
                .title("Deteniendo")
                .disabled(true)
        },
        InstanceStatus::Optimizing => {
            let progress = i.optimization.map(|p| p.0).unwrap_or(0.).clamp(0., 1.);
            play = PlayButton::new(
                button_id.clone(),
                format!("Creando la caché de arranque · {:.0} %", progress * 100.),
                None,
            )
            .title("Iniciando")
            .tint(0x176daf60)
            .progress(progress);
        },
        InstanceStatus::Running => {
            play = PlayButton::new(button_id.clone(), "En marcha", None)
                .title("Detener")
                .stop_icon()
                .tint(0xc52222b0)
                .on_click(move |_, _, _| data.backend_handle.send(MessageToBackend::KillInstance { id }))
        },
        InstanceStatus::NotRunning => {
            play = play.disabled(root::inherited_operation_active(id)).on_click(move |_, w, c| {
                if data.accounts.read(c).selected_account.is_none() {
                    root::open_home_accounts(c);
                } else {
                    root::start_instance(id, name.clone(), None, &data, w, c);
                }
            })
        },
    }

    if let Some((progress, count, total)) = root::home_update_progress(i.id) {
        let detail = if total > 0 {
            format!("{:.0} % · {} de {} MB", progress * 100., count / 1024 / 1024, total / 1024 / 1024)
        } else {
            String::new()
        };
        play = PlayButton::new(button_id.clone(), detail, None).updating(progress);
        window.request_animation_frame();
    }

    play
}
