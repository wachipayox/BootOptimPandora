use std::rc::Rc;

use bridge::message::MessageToBackend;
use gpui::*;
use gpui_component::{
    ActiveTheme, FocusableExt,
    button::Button,
    input::{Input, InputEvent, InputState},
};
use schema::backend_config::DistributionConfig;

use crate::settings::{SettingGroup, SettingItem, SettingItemWidget, SettingPage};

#[derive(Clone, Copy)]
enum Field {
    Url,
    CaPath,
    KeyId,
    PublicKey,
}

pub(super) fn create_page() -> SettingPage {
    SettingPage {
        title: || "Global profiles",
        groups: vec![SettingGroup {
            title: Some(|| "Private Distribution service"),
            items: vec![
                item(Field::Url),
                item(Field::CaPath),
                item(Field::KeyId),
                item(Field::PublicKey),
                SettingItem {
                    title: || "Test HTTPS connection",
                    description: || "Checks the server certificate and protocol version. Release signing keys are only needed to browse profiles.",
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
        }]
        .into(),
        searched_groups: None,
    }
}

fn item(field: Field) -> SettingItem {
    SettingItem {
        title: match field {
            Field::Url => || "HTTPS server URL",
            Field::CaPath => || "TLS CA certificate path",
            Field::KeyId => || "Release signing key ID",
            Field::PublicKey => || "Release public key",
        },
        description: match field {
            Field::Url => || "Use the private Distribution HTTPS address, including its port.",
            Field::CaPath => {
                || "Path to the PEM certificate that issued the server certificate. Leave blank only when the server certificate is publicly trusted."
            },
            Field::KeyId => || "The trusted Ed25519 release key ID configured on Distribution.",
            Field::PublicKey => || "The matching 32-byte Ed25519 public key in unpadded base64url.",
        },
        widget: text_field(field),
        ..Default::default()
    }
}

fn value(config: &DistributionConfig, field: Field) -> &str {
    match field {
        Field::Url => &config.base_url,
        Field::CaPath => &config.tls_ca_certificate_path,
        Field::KeyId => &config.release_key_id,
        Field::PublicKey => &config.release_public_key_base64url,
    }
}

fn set_value(config: &mut DistributionConfig, field: Field, value: String) {
    match field {
        Field::Url => config.base_url = value,
        Field::CaPath => config.tls_ca_certificate_path = value,
        Field::KeyId => config.release_key_id = value,
        Field::PublicKey => config.release_public_key_base64url = value,
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
                Field::KeyId => "distribution-key-id",
                Field::PublicKey => "distribution-public-key",
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
                if !matches!(event, InputEvent::PressEnter { .. }) {
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
                window.blur();
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
