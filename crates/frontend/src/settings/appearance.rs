use std::rc::Rc;

use gpui::*;
use gpui_component::{
    ActiveTheme, ThemeRegistry, WindowExt,
    button::{Button, ButtonVariants},
    h_flex,
    select::{Select, SelectEvent},
};

use crate::{
    component::design,
    component::named_dropdown::{DropdownName, NamedDropdownItem, SearchableNamedDropdown},
    entity::DataEntities,
    icon::PandoraIcon,
    interface_config::InterfaceConfig,
    settings::{SettingGroup, SettingItem, SettingItemWidget, SettingPage},
};

pub(super) fn create_page(data: &DataEntities, window: &mut Window, cx: &mut App) -> SettingPage {
    let background = cx.new(|cx| crate::background::BackgroundSelector::new(window, cx));
    SettingPage {
        title: || "Apariencia",
        groups: vec![
            SettingGroup {
                title: Some(|| "Tema"),
                items: vec![SettingItem {
                    widget: SettingItemWidget::AnyWide(Rc::new(|_, _| {
                        let mut choices = h_flex().gap(px(8.));
                        for (index, label) in ["Oscuro", "Claro", "Según el sistema"].into_iter().enumerate() {
                            choices = choices.child(design::chip(("theme-choice", index), label, index == 0).on_click(
                                move |_, window, cx| {
                                    if index == 0 {
                                        InterfaceConfig::get_mut(cx).set_active_theme("Wachiland Dark".into());
                                        InterfaceConfig::apply_theme(cx, true);
                                        return;
                                    }
                                    window.open_dialog(cx, |dialog, _, _| {
                                        design::dialog(dialog)
                                            .title(design::dialog_title("Lo sentimos"))
                                            .child("El launcher no acepta semejante gilipollez.")
                                            .footer(
                                                h_flex().justify_end().child(
                                                    design::action_button(
                                                        "keep-dark",
                                                        "Vale, oscuro",
                                                        design::ButtonAction::Primary,
                                                    )
                                                    .on_click(|_, w, c| w.close_dialog(c)),
                                                ),
                                            )
                                    });
                                },
                            ));
                        }
                        choices.into_any_element()
                    })),
                    ..Default::default()
                }]
                .into(),
                searched_items: None,
            },
            SettingGroup {
                title: Some(|| "Fondo de la app"),
                items: vec![SettingItem {
                    description: || "La imagen difuminada que se ve detrás de todas las pantallas.",
                    widget: SettingItemWidget::AnyWide(Rc::new(move |_, _| background.clone().into_any_element())),
                    ..Default::default()
                }]
                .into(),
                searched_items: None,
            },
            SettingGroup {
                title: Some(|| "Apariencia"),
                items: vec![
                    SettingItem {
                        title: || "Temas propios",
                        description: || "",
                        widget: create_theme_dropdown(window, cx),
                        ..Default::default()
                    },
                    SettingItem {
                        title: || "Abrir carpeta de temas",
                        description: || "",
                        widget: SettingItemWidget::Any(Rc::new({
                            let theme_folder = data.theme_folder.clone();
                            move |_, _| {
                                Button::new("open-theme-folder")
                                    .ghost()
                                    .rounded(px(0.))
                                    .text_size(px(13.))
                                    .icon(PandoraIcon::FolderOpen)
                                    .label("Abrir carpeta de temas")
                                    .on_click({
                                        let theme_folder = theme_folder.clone();
                                        move |_, window, cx| {
                                            crate::open_folder(&theme_folder, window, cx);
                                        }
                                    })
                                    .into_any_element()
                            }
                        })),
                        ..Default::default()
                    },
                    SettingItem {
                        title: || "Fuente",
                        description: || "",
                        widget: create_font_dropdown(window, cx),
                        ..Default::default()
                    },
                    SettingItem {
                        title: || "Tamaño de letra",
                        description: || "",
                        widget: SettingItemWidget::Integer {
                            get: |cx| cx.theme().font_size.as_f32().round() as i32,
                            set: |val, cx| {
                                InterfaceConfig::get_mut(cx).set_font_size(val);
                                InterfaceConfig::apply_theme(cx, true);
                            },
                            min: Some(8),
                            max: Some(32),
                        },
                        ..Default::default()
                    },
                ]
                .into(),
                searched_items: None,
            },
            SettingGroup {
                title: Some(|| "Ventana"),
                items: vec![
                    SettingItem {
                        title: || "Usar la barra de título del sistema",
                        description: || "Requiere reiniciar",
                        widget: SettingItemWidget::Switch(
                            |cfg| cfg.use_os_titlebar,
                            |cfg, val| cfg.use_os_titlebar = val,
                        ),
                        ..Default::default()
                    },
                    SettingItem {
                        title: || "Ocultar el launcher al jugar",
                        description: || "Vuelve a abrirse al cerrar el juego",
                        widget: SettingItemWidget::Switch(
                            |cfg| cfg.hide_main_window_on_launch,
                            |cfg, val| cfg.hide_main_window_on_launch = val,
                        ),
                        ..Default::default()
                    },
                    SettingItem {
                        title: || "Salir al cerrar la ventana principal",
                        description: || "",
                        widget: SettingItemWidget::Switch(
                            |cfg| cfg.quit_on_main_closed,
                            |cfg, val| cfg.quit_on_main_closed = val,
                        ),
                        ..Default::default()
                    },
                ]
                .into(),
                searched_items: None,
            },
        ]
        .into(),
        searched_groups: None,
    }
}

fn create_theme_dropdown(window: &mut Window, cx: &mut App) -> SettingItemWidget {
    let themes = ThemeRegistry::global(cx)
        .sorted_themes()
        .iter()
        .map(|theme| NamedDropdownItem {
            name: DropdownName::Literal(theme.name.clone()),
            item: theme.name.clone(),
        })
        .collect();

    let initial = cx.theme().theme_name().clone();
    let dropdown = SearchableNamedDropdown::create_and_select(themes, initial, window, cx);

    cx.subscribe(&dropdown, |_, event: &SelectEvent<_>, cx| {
        let SelectEvent::Confirm(Some(theme_name)) = event else {
            return;
        };
        InterfaceConfig::get_mut(cx).set_active_theme(theme_name.clone());
        InterfaceConfig::apply_theme(cx, true);
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

fn create_font_dropdown(window: &mut Window, cx: &mut App) -> SettingItemWidget {
    let fonts = cx
        .text_system()
        .all_font_names()
        .iter()
        .filter(|name| !name.starts_with('.'))
        .map(|font_name| {
            let font_name = SharedString::from(font_name);
            NamedDropdownItem {
                name: font_name.clone().into(),
                item: font_name,
            }
        })
        .collect();

    let initial = cx.theme().font_family.clone();
    let dropdown = SearchableNamedDropdown::create_and_select(fonts, initial, window, cx);

    cx.subscribe(&dropdown, |_, event: &SelectEvent<_>, cx| {
        let SelectEvent::Confirm(Some(font_family)) = event else {
            return;
        };

        InterfaceConfig::get_mut(cx).set_font_family(font_family.clone());
        InterfaceConfig::apply_theme(cx, true);
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
