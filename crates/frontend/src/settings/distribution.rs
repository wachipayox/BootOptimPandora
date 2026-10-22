use std::rc::Rc;

use bridge::message::MessageToBackend;
use gpui::*;
use gpui_component::{
    ActiveTheme, FocusableExt,
    button::Button,
    input::{Input, InputEvent, InputState},
    select::{Select, SelectEvent, SelectState},
};
use schema::backend_config::{DistributionConfig, LauncherChannel};

use crate::{
    component::named_dropdown::{DropdownName, NamedDropdown, NamedDropdownItem},
    settings::{SettingGroup, SettingItem, SettingItemWidget, SettingPage},
};

#[derive(Clone, Copy)]
enum Field {
    Url,
    CaPath,
}

pub(super) fn create_page() -> SettingPage {
    SettingPage {
        title: || "Global profiles",
        groups: vec![
            SettingGroup {
                title: Some(|| "Launcher channel"),
                items: vec![SettingItem {
                    title: || "Channel",
                    description: || {
                        "Player shows the published global profiles. Developer also shows the test profiles, labelled PRUEBA, that the server hides from players. The same setting is `channel` in the launcher configuration file."
                    },
                    widget: channel_widget(),
                    ..Default::default()
                }]
                .into(),
                searched_items: None,
            },
            SettingGroup {
                title: Some(|| "Private Distribution service"),
                items: vec![
                    item(Field::Url),
                    item(Field::CaPath),
                    SettingItem {
                        title: || "Test HTTPS connection",
                        description: || "Checks the server certificate and protocol version. Profile verification is configured automatically over HTTPS.",
                        widget: SettingItemWidget::Any(Rc::new(|_, cx| {
                            Button::new("check-distribution-connection")
                                .label("Test HTTPS connection")
                                .on_click(cx.listener(|root, _, _, _| {
                                    root.backend_handle.send(MessageToBackend::CheckDistributionConnection);
                                }))
                                .into_any_element()
                        })),
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

fn channel_widget() -> SettingItemWidget {
    SettingItemWidget::Backend(Rc::new(|backend, window, cx| {
        let mut created = false;
        let state = window.use_keyed_state("launcher-channel", cx, |window, cx| {
            created = true;
            let items = vec![
                NamedDropdownItem {
                    name: DropdownName::new("Player"),
                    item: LauncherChannel::Player,
                },
                NamedDropdownItem {
                    name: DropdownName::new("Developer"),
                    item: LauncherChannel::Developer,
                },
            ];
            let mut state = SelectState::new(NamedDropdown::new(items), None, window, cx);
            state.set_selected_value(&backend.channel, window, cx);
            state
        });
        if created {
            cx.subscribe(&state, |root, _, event: &SelectEvent<_>, cx| {
                let SelectEvent::Confirm(Some(channel)) = event else { return };
                if root.backend_config().is_some_and(|config| config.channel != *channel) {
                    root.set_launcher_channel(*channel, cx);
                }
            })
            .detach();
        } else if state.read(cx).selected_value() != Some(&backend.channel) {
            state.update(cx, |state, cx| state.set_selected_value(&backend.channel, window, cx));
        }
        Select::new(&state).menu_width(px(200.0)).into_any_element()
    }))
}

fn item(field: Field) -> SettingItem {
    SettingItem {
        title: match field {
            Field::Url => || "HTTPS server URL",
            Field::CaPath => || "TLS CA certificate path",
        },
        description: match field {
            Field::Url => || "Use the private Distribution HTTPS address, including its port.",
            Field::CaPath => {
                || "Path to the PEM certificate that issued the server certificate. Leave blank only when the server certificate is publicly trusted."
            },
        },
        widget: text_field(field),
        ..Default::default()
    }
}

fn value(config: &DistributionConfig, field: Field) -> &str {
    match field {
        Field::Url => &config.base_url,
        Field::CaPath => &config.tls_ca_certificate_path,
    }
}

fn set_value(config: &mut DistributionConfig, field: Field, value: String) {
    match field {
        Field::Url => config.base_url = value,
        Field::CaPath => config.tls_ca_certificate_path = value,
    }
}

fn text_field(field: Field) -> SettingItemWidget {
    SettingItemWidget::Backend(Rc::new(move |backend, window, cx| {
        let config = &backend.distribution;
        let mut created = false;
        let state = window.use_keyed_state(
            match field {
                Field::Url => "distribution-url",
                Field::CaPath => "distribution-ca-path",
            },
            cx,
            |window, cx| {
                created = true;
                let mut state = InputState::new(window, cx);
                state.set_value(value(config, field), window, cx);
                state
            },
        );
        if created {
            cx.subscribe_in(&state, window, move |root, state, event: &InputEvent, window, cx| {
                let blur_after_save = matches!(event, InputEvent::PressEnter { .. });
                if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    return;
                }
                let Some(mut config) = root.backend_config().map(|backend| backend.distribution.clone()) else {
                    return;
                };
                let updated = state.read(cx).value().to_string();
                if value(&config, field) != updated {
                    set_value(&mut config, field, updated);
                    root.set_distribution_settings(config, cx);
                }
                if blur_after_save {
                    window.blur();
                }
            })
            .detach();
        }

        let current = value(config, field);
        let state_read = state.read(cx);
        let dirty = state_read.value().as_ref() != current && state_read.focus_handle(cx).is_focused(window);
        if !dirty && state_read.value().as_ref() != current {
            state.update(cx, |state, cx| state.set_value(current, window, cx));
        }
        let mut input = Input::new(&state).w(px(320.0));
        if dirty {
            input = input.focus_ring(false).border_1().border_color(cx.theme().warning);
        }
        input.into_any_element()
    }))
}
