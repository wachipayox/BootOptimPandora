use std::sync::{
    Arc,
    atomic::{AtomicIsize, Ordering},
};

use bridge::{account::Account, handle::BackendHandle, message::MessageToBackend};
use gpui::{prelude::FluentBuilder, *};
use gpui_component::{
    ActiveTheme, Disableable, WindowExt,
    button::{Button, ButtonVariants},
    h_flex,
    input::InputState,
    sheet::Sheet,
    v_flex,
};
use rand::Rng;
use uuid::Uuid;

use crate::{
    component::{design, shrinking_text::ShrinkingText},
    entity::{
        DataEntities,
        account::{AccountEntries, AccountExt},
    },
    icon::PandoraIcon,
    interface_config::InterfaceConfig,
    png_render_cache,
};

const ACCOUNT_HEIGHT: f32 = 56.0;
const ACCOUNT_GAP: f32 = 8.0;
const ACCOUNT_STRIDE: f32 = ACCOUNT_HEIGHT + ACCOUNT_GAP;

struct AccountDragInfo {
    index: usize,
    name: SharedString,
    start_y: f32,
    delta: AtomicIsize,
    backend_handle: BackendHandle,
}

impl Drop for AccountDragInfo {
    fn drop(&mut self) {
        let delta = self.delta.load(Ordering::Relaxed);
        if delta != 0 {
            self.backend_handle.send(MessageToBackend::ReorderAccounts {
                from_index: self.index,
                delta,
            });
        }
    }
}

struct AnimateMove {
    from: usize,
    to: usize,
}

struct AccountDragPreview {
    name: SharedString,
}

impl Render for AccountDragPreview {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let size = size(px(180.0), px(44.0));

        div().left(px(24.0)).top(-size.height.half()).child(
            h_flex()
                .w(size.width)
                .h(size.height)
                .px_3()
                .gap_2()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().popover)
                .text_color(cx.theme().popover_foreground)
                .shadow_lg()
                .child(PandoraIcon::GripVertical)
                .child(ShrinkingText::new(self.name.clone())),
        )
    }
}

struct Accounts {
    backend_handle: BackendHandle,
    accounts: Entity<AccountEntries>,
    animate_move: Option<AnimateMove>,
    shift_amounts: Vec<f32>,
    last_accounts: Arc<[Account]>,
}

pub fn build_accounts_sheet(
    data: &DataEntities,
    _: &mut Window,
    cx: &mut App,
) -> impl Fn(Sheet, &mut Window, &mut App) -> Sheet + 'static {
    let accounts = cx.new(|_| {
        let accounts = Accounts {
            backend_handle: data.backend_handle.clone(),
            accounts: data.accounts.clone(),
            animate_move: None,
            shift_amounts: Vec::new(),
            last_accounts: Arc::new([]),
        };

        accounts
    });

    move |sheet, _, _| {
        sheet
            .title(t::account::title())
            .size(px(420.))
            .when(cfg!(target_os = "macos"), |this| this.pt_5())
            .child(accounts.clone())
    }
}

impl Render for Accounts {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let hide_skins = InterfaceConfig::get(cx).hide_skins;

        let (accounts, selected_account) = {
            let accounts = self.accounts.read(cx);
            (accounts.accounts.clone(), accounts.selected_account_uuid)
        };

        let start_y = window.mouse_position().y.as_f32();

        if let Some(animate_move) = &self.animate_move {
            let reset = if animate_move.from != animate_move.to {
                !Arc::ptr_eq(&accounts, &self.last_accounts)
            } else {
                !cx.has_active_drag()
            };
            if reset {
                self.shift_amounts.clear();
                self.animate_move = None;
            }
        }
        self.last_accounts = accounts.clone();

        let items = accounts.iter().enumerate().filter_map(|(account_index, account)| {
            let head = if hide_skins {
                gpui::img(ImageSource::Resource(Resource::Embedded("images/hidden_head.png".into())))
            } else if let Some(head) = &account.head {
                let resize = png_render_cache::ImageTransformation::Resize { width: 32, height: 32 };
                png_render_cache::render_with_transform(head.clone(), resize, cx)
            } else {
                gpui::img(ImageSource::Resource(Resource::Embedded("images/default_head.png".into())))
            };
            let account_name = account.username(InterfaceConfig::get(cx).hide_usernames);

            let selected = Some(account.uuid) == selected_account;
            let group_name: SharedString = format!("account-row-{account_index}").into();
            let uuid = account.uuid;
            let drag_info = AccountDragInfo {
                index: account_index,
                name: account_name.clone(),
                start_y,
                backend_handle: self.backend_handle.clone(),
                delta: AtomicIsize::new(0),
            };

            let row_bg = if selected {
                cx.theme().info.opacity(0.2)
            } else {
                cx.theme().input_background()
            };
            let row_border = if selected {
                cx.theme().info
            } else {
                cx.theme().border.opacity(0.55)
            };
            let hover_bg = if selected {
                cx.theme().info_hover.opacity(0.3)
            } else {
                cx.theme().secondary_hover
            };

            let mut shift = false;
            let mut shift_offset = 0.0;
            if let Some(animate_move) = &self.animate_move {
                if animate_move.from == account_index {
                    return None;
                }
                shift = if account_index > animate_move.from {
                    shift_offset = ACCOUNT_STRIDE;
                    account_index > animate_move.to
                } else {
                    account_index >= animate_move.to
                };
            }
            let previous_shift_amount = self.shift_amounts.get(account_index).copied().unwrap_or(0.0);
            let desired_shift_amount = if shift {
                ACCOUNT_STRIDE - shift_offset
            } else {
                0.0 - shift_offset
            };
            let shift_amount_delta = previous_shift_amount - desired_shift_amount;
            let new_shift_amount = if shift_amount_delta.abs() < 1.0 {
                desired_shift_amount
            } else {
                previous_shift_amount * 0.5 + desired_shift_amount * 0.5
            };

            if previous_shift_amount != new_shift_amount {
                window.request_animation_frame();
                if self.shift_amounts.len() < account_index + 1 {
                    self.shift_amounts.resize(account_index + 1, 0.0);
                }
                self.shift_amounts[account_index] = new_shift_amount;
            }

            Some(
                h_flex()
                    .id(("account-row", account_index))
                    .group(group_name.clone())
                    .w_full()
                    .h(px(ACCOUNT_HEIGHT))
                    .top(px(new_shift_amount + shift_offset))
                    .px_3()
                    .gap_3()
                    .rounded(cx.theme().radius)
                    .border_1()
                    .border_color(row_border)
                    .bg(row_bg)
                    .text_size(rems(0.9375))
                    .line_height(rems(1.0))
                    .hover(|style| style.bg(hover_bg))
                    .when(!selected, |this| {
                        this.on_click({
                            let backend_handle = self.backend_handle.clone();
                            move |_, _, _| {
                                backend_handle.send(MessageToBackend::SelectAccount { uuid });
                            }
                        })
                    })
                    .child(
                        div()
                            .id(("account-drag", account_index))
                            .flex()
                            .items_center()
                            .justify_center()
                            .size_6()
                            .min_w_6()
                            .text_color(cx.theme().muted_foreground)
                            .hover(|style| style.text_color(cx.theme().foreground))
                            .child(PandoraIcon::GripVertical)
                            .cursor_grab()
                            .on_drag(drag_info, move |info: &AccountDragInfo, _, _, cx| {
                                cx.new(|_| AccountDragPreview {
                                    name: info.name.clone(),
                                })
                            }),
                    )
                    .child(head.size_8().min_w_8().min_h_8())
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .text_color(if selected {
                                cx.theme().info
                            } else {
                                cx.theme().foreground
                            })
                            .child(ShrinkingText::new(account_name.clone())),
                    )
                    .child(
                        Button::new(("account-delete", account_index))
                            .invisible()
                            .group_hover(group_name.clone(), |style| style.visible())
                            .ghost()
                            .compact()
                            .icon(PandoraIcon::Trash2)
                            .h_8()
                            .w_8()
                            .danger()
                            .on_click({
                                let backend_handle = self.backend_handle.clone();
                                move |_, _, cx| {
                                    cx.stop_propagation();
                                    backend_handle.send(MessageToBackend::DeleteAccount { uuid });
                                }
                            }),
                    )
                    .into_any_element(),
            )
        });

        v_flex()
            .gap(px(ACCOUNT_GAP))
            .h_full()
            .child(
                Button::new("add-account")
                    .h_10()
                    .success()
                    .icon(PandoraIcon::Plus)
                    .label(t::account::add::label())
                    .on_click({
                        let backend_handle = self.backend_handle.clone();
                        move |_, window, cx| {
                            crate::root::start_new_account_login(&backend_handle, window, cx);
                        }
                    }),
            )
            .child(
                Button::new("add-offline")
                    .h_10()
                    .success()
                    .icon(PandoraIcon::Plus)
                    .label(t::account::add::offline())
                    .on_click({
                        let backend_handle = self.backend_handle.clone();
                        move |_, window, cx| {
                            open_offline_account(backend_handle.clone(), window, cx);
                        }
                    }),
            )
            .children(items)
            .on_drag_move(cx.listener(move |this, event: &DragMoveEvent<AccountDragInfo>, window, cx| {
                if cx.active_drag_cursor_style() != Some(CursorStyle::ClosedHand) {
                    cx.set_active_drag_cursor_style(CursorStyle::ClosedHand, window);
                }

                let max_delta = this.accounts.read(cx).accounts.len().saturating_sub(1) as isize;

                let drag = event.drag(cx);
                let from_index = drag.index as isize;

                let delta = event.event.position.y.as_f32() - drag.start_y;
                let relative_delta = delta / ACCOUNT_STRIDE;
                let item_delta = relative_delta.round() as isize;
                let item_delta = item_delta.clamp(-from_index, max_delta - from_index);

                drag.delta.store(item_delta, Ordering::Relaxed);

                let to = (from_index + item_delta) as usize;

                if let Some(animate_move) = &this.animate_move {
                    if animate_move.to == to {
                        return;
                    }
                }

                this.animate_move = Some(AnimateMove {
                    from: from_index as usize,
                    to,
                });
                cx.notify();
            }))
    }
}

pub fn open_offline_account(backend_handle: BackendHandle, window: &mut Window, cx: &mut App) {
    let name_input = cx.new(|cx| InputState::new(window, cx));
    let uuid_input = cx.new(|cx| InputState::new(window, cx));
    let custom_uuid = cx.new(|_| false);
    window.open_dialog(cx, move |dialog, _, cx| {
        let username = name_input.read(cx).value();
        let valid_name =
            !username.is_empty() && username.len() <= 16 && username.as_bytes().iter().all(|c| *c > 32 && *c < 127);
        let custom = *custom_uuid.read(cx);
        let uuid = uuid_input.read(cx).value();
        let valid_uuid = !custom || Uuid::try_parse(&uuid).is_ok();
        let backend = backend_handle.clone();
        let add = design::action_button("add-offline", "Añadir", design::ButtonAction::Primary)
            .disabled(!valid_name || !valid_uuid)
            .on_click(move |_, window, cx| {
                let uuid = if custom {
                    let Ok(uuid) = Uuid::try_parse(&uuid) else {
                        return;
                    };
                    uuid
                } else {
                    let value: u128 = rand::thread_rng().r#gen();
                    Uuid::from_u128((value & !0xF0000000000000000000) | 0x30000000000000000000)
                };
                backend.send(MessageToBackend::AddOfflineAccount {
                    name: username.clone().into(),
                    uuid,
                });
                window.close_all_dialogs(cx);
            });
        let mut uuid_choices = h_flex().gap(px(8.));
        for (index, label) in ["Aleatorio", "Propio"].into_iter().enumerate() {
            let mode = custom_uuid.clone();
            uuid_choices = uuid_choices.child(
                design::chip(("offline-uuid-mode", index), label, custom == (index == 1)).on_click(move |_, _, cx| {
                    mode.update(cx, |mode, cx| {
                        *mode = index == 1;
                        cx.notify();
                    })
                }),
            );
        }
        design::dialog(dialog)
            .title(design::dialog_title("Añadir cuenta sin conexión"))
            .child(
                v_flex()
                    .gap(px(12.))
                    .child(crate::labelled("Nombre", design::text_field(&name_input)))
                    .child(crate::labelled("UUID", uuid_choices))
                    .when(custom, |body| body.child(design::text_field(&uuid_input)))
                    .child(div().text_size(px(12.)).text_color(rgb(crate::theme::INK_MUTED)).child(
                        "Sin conexión no se pueden cambiar aspectos ni entrar en servidores con autenticación.",
                    )),
            )
            .footer(
                h_flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        design::action_button("cancel-offline", "Cancelar", design::ButtonAction::Secondary)
                            .on_click(|_, w, c| w.close_dialog(c)),
                    )
                    .child(add),
            )
    });
}
