use crate::{
    component::design::{self, ButtonAction},
    entity::DataEntities,
};
use bridge::instance::{ContentFolder, InstanceID};
use gpui::{prelude::*, *};
use gpui_component::{WindowExt, checkbox::Checkbox, h_flex, tooltip::Tooltip, v_flex};
use schema::content::ContentSource;
use std::{cell::Cell, path::Path, rc::Rc, sync::Arc};

const NOTICE: &str = "Tienes este mod instalado pero deshabilitado";

pub(crate) fn original(
    data: &DataEntities,
    instance: Option<InstanceID>,
    source: &ContentSource,
    cx: &App,
) -> Option<Arc<Path>> {
    let entry = data.instances.read(cx).entries.get(&instance?)?.read(cx);
    let mods = entry.content[ContentFolder::Mods].read(cx).as_ref()?;
    let mut disabled = None;
    for item in mods.iter().filter(|item| &item.content_source == source) {
        if item.enabled {
            return None;
        }
        if disabled.is_none() && item.path.extension().is_some_and(|extension| extension == "disabled") {
            disabled = Some(item.path.clone());
        }
    }
    disabled
}

pub(crate) fn badge(index: usize) -> impl IntoElement {
    div()
        .id(("disabled-installed", index))
        .text_color(rgb(0xf0cc66))
        .tooltip(|window, cx| Tooltip::new(NOTICE).build(window, cx))
        .child(svg().path("icons/triangle-alert.svg").size(px(14.)).text_color(rgb(0xf0cc66)))
}

pub(crate) fn confirm(
    original: Arc<Path>,
    window: &mut Window,
    cx: &mut App,
    install: impl Fn(Option<Arc<Path>>, &mut Window, &mut App) + 'static,
) {
    let remove = Rc::new(Cell::new(true));
    let install = Rc::new(install);
    window.open_dialog(cx, move |dialog, _, _| {
        let checkbox = remove.clone();
        let proceed = remove.clone();
        let original = original.clone();
        let install = install.clone();
        design::dialog(dialog)
            .title(design::dialog_title(NOTICE))
            .child(
                v_flex().child(
                    Checkbox::new("remove-original-disabled")
                        .label("Eliminar mod deshabilitado original")
                        .checked(remove.get())
                        .on_click(move |value, _, cx| {
                            checkbox.set(*value);
                            cx.refresh_windows();
                        }),
                ),
            )
            .footer(
                h_flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        design::action_button("cancel-disabled-install", "Cancelar", ButtonAction::Secondary)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        design::action_button("confirm-disabled-install", "Instalar aparte", ButtonAction::Primary)
                            .on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                                install(proceed.get().then(|| original.clone()), window, cx);
                            }),
                    ),
            )
    });
}
