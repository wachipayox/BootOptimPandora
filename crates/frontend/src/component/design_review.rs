//! Isolated component review, reached only by the UI capture workflow.
use super::design::*;
use crate::theme;
use gpui::{prelude::*, *};
use gpui_component::{Disableable, WindowExt, h_flex, input::InputState, v_flex};

pub fn render(window: &mut Window, cx: &mut App) -> AnyElement {
    let enabled = window.use_keyed_state("review-toggle", cx, |_, _| true);
    let value = *enabled.read(cx);
    let input =
        window.use_keyed_state("review-input", cx, |window, cx| InputState::new(window, cx).placeholder("Nombre"));
    v_flex()
        .size_full()
        .bg(rgb(theme::BG))
        .p(px(32.))
        .gap(px(24.))
        .text_color(rgb(theme::INK))
        .font_family(theme::SANS)
        .child(eyebrow("WACHILAND LAUNCHER"))
        .child(
            div()
                .font_family(theme::DISPLAY)
                .text_size(px(20.))
                .font_weight(FontWeight::SEMIBOLD)
                .child("Ajustes de la instancia"),
        )
        .child(
            v_flex()
                .w(px(600.))
                .bg(rgb(theme::PANEL))
                .p(px(18.))
                .gap(px(12.))
                .child(
                    h_flex()
                        .gap(px(8.))
                        .child(action_button("cancel", "Cancelar", ButtonAction::Secondary))
                        .child(action_button("save", "Guardar", ButtonAction::Primary))
                        .child(action_button("delete", "Eliminar", ButtonAction::Danger)),
                )
                .child(setting_row(
                    "Optimizar arranque",
                    "Prepara la próxima partida",
                    YesNo::new("enabled").checked(value).on_click(move |v, _, cx| {
                        enabled.update(cx, |value, _| *value = *v);
                        cx.refresh_windows();
                    }),
                ))
                .child(setting_row(
                    "Memoria",
                    "",
                    h_flex()
                        .gap(px(2.))
                        .child(chip("recommended", "Recomendada", true))
                        .child(chip("manual", "Manual", false)),
                ))
                .child(text_field(&input))
                .child(progress("Creando la caché de arranque", 0.62, "62 %")),
        )
        .child(
            h_flex()
                .gap(px(20.))
                .child(tab("content", "Contenido", true))
                .child(tab("logs", "Registros", false))
                .child(tab("settings", "Ajustes de la instancia", false)),
        )
        .child(
            h_flex()
                .gap(px(16.))
                .child(PlayButton::new("play", "Al día", Some("images/wachiland-glass-review.png".into())))
                .child(PlayButton::new("play-disabled", "Sin instancia", None).disabled(true))
                .child(
                    PlayButton::new(
                        "play-updating",
                        "42 % · 120 de 286 MB",
                        Some("images/wachiland-glass-review.png".into()),
                    )
                    .updating(0.42),
                ),
        )
        .child(
            action_button("review-dialog", "Revisión nueva", ButtonAction::Primary).on_click(|_, window, cx| {
                open_review_dialog(window, cx);
            }),
        )
        .into_any_element()
}

pub fn open_review_dialog(window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, |d, _, _| {
        dialog(d)
            .title(dialog_title("Revisión nueva"))
            .child("Tus cambios locales se conservan")
            .footer(
                h_flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        action_button("close-review", "Más tarde", ButtonAction::Secondary)
                            .on_click(|_, w, c| w.close_dialog(c)),
                    )
                    .child(action_button("confirm-review", "Actualizar", ButtonAction::Primary)),
            )
    });
}
