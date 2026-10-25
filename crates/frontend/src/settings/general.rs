use std::rc::Rc;

use gpui::*;
use gpui_component::select::{Select, SelectEvent};

use crate::{
    component::named_dropdown::{DropdownName, NamedDropdown, NamedDropdownItem, SearchableNamedDropdown},
    interface_config::{InterfaceConfig, LiveGameOutputDisplay},
    settings::{SettingGroup, SettingItem, SettingItemWidget, SettingPage},
};

pub(super) fn create_page(window: &mut Window, cx: &mut App) -> SettingPage {
    SettingPage {
        title: || "General",
        groups: vec![
            SettingGroup {
                title: Some(|| "General"),
                items: vec![
                    SettingItem {
                        title: || "Idioma",
                        description: || "",
                        widget: create_language_dropdown(window, cx),
                        ..Default::default()
                    },
                    SettingItem {
                        title: || "Borrado rápido de mods",
                        description: || "Mayús+clic en borrar no pide confirmación",
                        widget: SettingItemWidget::Switch(
                            |cfg| cfg.quick_delete_mods,
                            |cfg, val| cfg.quick_delete_mods = val,
                        ),
                        ..Default::default()
                    },
                    SettingItem {
                        title: || "Borrado rápido de instancias",
                        description: || "Igual, al eliminar una instancia",
                        widget: SettingItemWidget::Switch(
                            |cfg| cfg.quick_delete_instance,
                            |cfg, val| cfg.quick_delete_instance = val,
                        ),
                        ..Default::default()
                    },
                    SettingItem {
                        title: || "Salida del juego en vivo",
                        description: || "",
                        widget: create_live_game_output_dropdown(window, cx),
                        ..Default::default()
                    },
                ]
                .into(),
                searched_items: None,
            },
            SettingGroup {
                title: Some(|| "Privacidad"),
                items: vec![
                    SettingItem {
                        title: || "Ocultar nombres de usuario",
                        description: || "",
                        widget: SettingItemWidget::Switch(
                            |cfg| cfg.hide_usernames,
                            |cfg, val| cfg.hide_usernames = val,
                        ),
                        ..Default::default()
                    },
                    SettingItem {
                        title: || "Ocultar aspectos",
                        description: || "",
                        widget: SettingItemWidget::Switch(|cfg| cfg.hide_skins, |cfg, val| cfg.hide_skins = val),
                        ..Default::default()
                    },
                    SettingItem {
                        title: || "Ocultar direcciones de servidor",
                        description: || "Útil al hacer directos",
                        widget: SettingItemWidget::Switch(
                            |cfg| cfg.hide_server_addresses,
                            |cfg, val| cfg.hide_server_addresses = val,
                        ),
                        ..Default::default()
                    },
                    // todo: Only hide when OBS is open
                ]
                .into(),
                searched_items: None,
            },
        ]
        .into(),
        searched_groups: None,
    }
}

fn create_language_dropdown(window: &mut Window, cx: &mut App) -> SettingItemWidget {
    let languages = std::iter::once(NamedDropdownItem {
        name: DropdownName::new("Según el sistema"),
        item: SharedString::new_static("system"),
    })
    .chain(t::languages().into_iter().map(|(id, name)| NamedDropdownItem {
        name: DropdownName::Literal(SharedString::new_static(*name)),
        item: SharedString::new_static(*id),
    }))
    .collect();

    let initial = match &InterfaceConfig::get(cx).language {
        t::Language::System => "system".into(),
        t::Language::Code(code) => code.into(),
    };
    let dropdown = SearchableNamedDropdown::create_and_select(languages, initial, window, cx);

    cx.subscribe(&dropdown, |_, event: &SelectEvent<_>, cx| {
        let SelectEvent::Confirm(Some(value)) = event else {
            return;
        };
        let language = match &**value {
            "system" => t::Language::System,
            lang => t::Language::Code(lang.into()),
        };
        t::set_lang(&language);
        InterfaceConfig::get_mut(cx).language = language;
        cx.refresh_windows();
    })
    .detach();

    SettingItemWidget::Any(Rc::new(move |_, _| {
        Select::new(&dropdown)
            .h(px(32.))
            .bg(rgb(crate::theme::PANEL_SUNKEN))
            .rounded(px(0.))
            .text_size(px(13.))
            .w(px(180.))
            .menu_width(px(200.0))
            .search_placeholder(t::common::search())
            .into_any_element()
    }))
}

fn create_live_game_output_dropdown(window: &mut Window, cx: &mut App) -> SettingItemWidget {
    let options = vec![
        NamedDropdownItem {
            name: DropdownName::new("Pestaña de la instancia"),
            item: LiveGameOutputDisplay::TabOnInstancePage,
        },
        NamedDropdownItem {
            name: DropdownName::new("Ventana aparte"),
            item: LiveGameOutputDisplay::SeparateWindow,
        },
        NamedDropdownItem {
            name: DropdownName::new("Oculta"),
            item: LiveGameOutputDisplay::Hidden,
        },
    ];

    let initial = InterfaceConfig::get(cx).live_game_output_display;
    let dropdown = NamedDropdown::create_and_select(options, initial, window, cx);

    cx.subscribe(&dropdown, |_, event: &SelectEvent<_>, cx| {
        let SelectEvent::Confirm(Some(value)) = event else {
            return;
        };
        InterfaceConfig::get_mut(cx).live_game_output_display = *value;
        cx.refresh_windows();
    })
    .detach();

    SettingItemWidget::Any(Rc::new(move |_, _| {
        Select::new(&dropdown)
            .h(px(32.))
            .bg(rgb(crate::theme::PANEL_SUNKEN))
            .rounded(px(0.))
            .text_size(px(13.))
            .menu_width(px(200.0))
            .into_any_element()
    }))
}
