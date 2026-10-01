use std::sync::Arc;

use bridge::{handle::BackendHandle, instance::InstanceID, message::MessageToBackend, modal_action::ModalAction};
use gpui::{prelude::*, *};
use gpui_component::{ActiveTheme, Disableable, Sizable, WindowExt, button::Button, checkbox::Checkbox, dialog::Dialog, h_flex, input::{Input, InputState}, slider::{Slider, SliderEvent, SliderState}, v_flex};
use crate::{entity::instance::InstanceEntries, get_unique_instance_name, modals::generic};

struct DuplicateInstanceModalState {
    instance_id: InstanceID,
    backend: BackendHandle,
    name: Entity<InputState>,
    instances: Entity<InstanceEntries>,
    default_name: SharedString,
    as_branch: bool,
    group: Option<Result<bool, String>>,
    create_group: bool,
    reuse_icon: bool,
    hue: Entity<SliderState>,
}
impl DuplicateInstanceModalState {
    fn render(&mut self, dialog: Dialog, window: &mut Window, cx: &mut Context<Self>) -> Dialog {
        let name = self.name.read(cx).value();
        let name = if name.trim().is_empty() { self.default_name.to_string() } else { name.trim().to_owned() };
        let valid = crate::is_valid_instance_name(&name) && !self.instances.read(cx).entries.values().any(|i| i.read(cx).name.as_str() == name);
        let ready = !self.as_branch || matches!(self.group, Some(Ok(_)));
        let enter_backend = self.backend.clone();
        let enter_id = self.instance_id;
        let enter_name = name.clone();
        let enter_as_branch = self.as_branch;
        let enter_create_group = self.create_group;
        let enter_reuse_icon = self.reuse_icon;
        let enter_hue_degrees = match self.hue.read(cx).value() {
            gpui_component::slider::SliderValue::Single(value) => value.round() as i32 - 180,
            gpui_component::slider::SliderValue::Range(value, _) => value.round() as i32 - 180,
        };
        let dialog = dialog.on_ok(move |_, window, cx| {
            if !valid || !ready { return false; }
            let modal_action = ModalAction::default();
            window.close_dialog(cx);
            generic::show_modal(
                window,
                cx,
                if enter_as_branch { "Creating derived instance…".into() } else { t::instance::duplicate::progress().into() },
                if enter_as_branch { "Unable to create derived instance".into() } else { t::instance::duplicate::error().into() },
                modal_action.clone(),
            );
            if enter_as_branch {
                enter_backend.send(MessageToBackend::CreateLocalBranch {
                    id: enter_id,
                    name: enter_name.as_str().into(),
                    create_save_group: enter_create_group,
                    reuse_parent_icon: enter_reuse_icon,
                    icon_hue_degrees: enter_hue_degrees,
                    modal_action,
                });
            } else {
                enter_backend.send(MessageToBackend::DuplicateInstance { id: enter_id, name: enter_name.as_str().into(), modal_action });
            }
            // Close this dialog before opening the progress dialog; leave it to the explicit
            // close above instead of letting the default confirmation close the new dialog.
            false
        });
        let mut content = v_flex().gap_3().child(crate::labelled(t::instance::name(), Input::new(&self.name)));
        if self.as_branch {
            content = content.child(match &self.group {
                None => div().child("Checking the parent's save groupâ€¦").into_any_element(),
                Some(Err(error)) => div().text_color(cx.theme().danger).child(error.clone()).into_any_element(),
                Some(Ok(true)) => div().child("This instance will share its parent's worlds.").into_any_element(),
                Some(Ok(false)) => v_flex().gap_2()
                    .child("The parent has no save group. Create one to share worlds between these instances?")
                    .child(Checkbox::new("branch-share-worlds").label("Create a shared save group").checked(self.create_group)
                        .on_click(cx.listener(|this, value, _, cx| { this.create_group = *value; cx.notify(); }))).into_any_element(),
            });
            content = content.child(Checkbox::new("branch-reuse-icon").label("Reuse parent's icon")
                .checked(self.reuse_icon)
                .on_click(cx.listener(|this, value, _, cx| { this.reuse_icon = *value; cx.notify(); })));
            if self.reuse_icon {
                let hue_value = match self.hue.read(cx).value() { gpui_component::slider::SliderValue::Single(v) => v, gpui_component::slider::SliderValue::Range(v, _) => v };
                let hue_degrees = hue_value.round() as i32 - 180;
                let parent_icon = self.instances.read(cx).entries.get(&self.instance_id)
                    .and_then(|instance| instance.read(cx).icon.clone());
                let icon_preview = parent_icon.and_then(|bytes| {
                    let image = image::load_from_memory(&bytes).ok()?.huerotate(hue_degrees);
                    let mut encoded = std::io::Cursor::new(Vec::new());
                    image.write_to(&mut encoded, image::ImageFormat::Png).ok()?;
                    Some(Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Png, encoded.into_inner())))
                });
                let preview = match icon_preview {
                    Some(image) => gpui::img(image).size_8().rounded_md().into_any_element(),
                    None => div().size_8().rounded_md().bg(cx.theme().muted).into_any_element(),
                };
                let hue_row = h_flex().gap_2().items_center()
                    .child(preview)
                    .child(Slider::new(&self.hue).flex_1())
                    .child(div().w(px(56.0)).flex_shrink_0().whitespace_nowrap().child(format!("{}°", hue_value.round() as i32)))
                    .child(Button::new("randomize-parent-icon-hue").label("↻").small()
                        .on_click(cx.listener(|this, _, window, cx| {
                            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().subsec_nanos();
                            let value = (nanos % 361) as f32;
                            this.hue.update(cx, |slider, cx| slider.set_value(value, window, cx));
                            cx.notify();
                        })));
                content = content.child(crate::labelled("Icon hue · neutral at 180°", hue_row));
            }
        }
        dialog.title(if self.as_branch { "Create derived instance".into() } else { t::instance::duplicate::title() })
            .overlay_closable(false).child(content).footer(h_flex().gap_2().w_full()
                .child(Button::new("cancel").label(t::common::cancel()).on_click(|_, window, cx| window.close_dialog(cx)))
                .child(Button::new("create").label(if self.as_branch { "Create derived instance" } else { "Duplicate" })
                    .disabled(!valid || !ready).on_click(cx.listener(move |this, _, window, cx| {
                        let modal_action = ModalAction::default();
                        window.close_dialog(cx);
                        generic::show_modal(window, cx,
                            if this.as_branch { "Creating derived instanceâ€¦".into() } else { t::instance::duplicate::progress().into() },
                            if this.as_branch { "Unable to create derived instance".into() } else { t::instance::duplicate::error().into() }, modal_action.clone());
                        if this.as_branch {
                            this.backend.send(MessageToBackend::CreateLocalBranch { id: this.instance_id, name: name.as_str().into(), create_save_group: this.create_group, reuse_parent_icon: this.reuse_icon, icon_hue_degrees: { let value = match this.hue.read(cx).value() { gpui_component::slider::SliderValue::Single(v) => v, gpui_component::slider::SliderValue::Range(v, _) => v }; value.round() as i32 - 180 }, modal_action });
                        } else {
                            this.backend.send(MessageToBackend::DuplicateInstance { id: this.instance_id, name: name.as_str().into(), modal_action });
                        }
                    }))))
    }
}
pub fn open_duplicate_instance(id: InstanceID, name: SharedString, instances: Entity<InstanceEntries>, backend: BackendHandle, window: &mut Window, cx: &mut App) {
    open_instance_copy(id, name, instances, backend, false, window, cx);
}
pub fn open_derived_instance(id: InstanceID, name: SharedString, instances: Entity<InstanceEntries>, backend: BackendHandle, window: &mut Window, cx: &mut App) {
    open_instance_copy(id, name, instances, backend, true, window, cx);
}
fn open_instance_copy(id: InstanceID, name: SharedString, instances: Entity<InstanceEntries>, backend: BackendHandle, as_branch: bool, window: &mut Window, cx: &mut App) {
    let names = instances.read(cx).entries.values().map(|i| i.read(cx).name.to_string()).collect::<Vec<_>>();
    let refs = names.iter().map(String::as_str).collect::<Vec<_>>();
    let default_name: SharedString = get_unique_instance_name(&format!("{} {}", if as_branch { "Branch of" } else { "Copy of" }, name), &refs).into();
    let state = cx.new(|cx| DuplicateInstanceModalState { instance_id: id, backend: backend.clone(), name: cx.new(|cx| InputState::new(window, cx).placeholder(default_name.clone())), instances, default_name, as_branch, group: None, create_group: true, reuse_icon: true, hue: cx.new(|_| SliderState::new().min(0.0).max(360.0).default_value(180.0)) });
    // Repaint validation as the name is edited.
    let input = state.read(cx).name.clone();
    cx.subscribe(&input, { let state = state.clone(); move |_, _: &gpui_component::input::InputEvent, cx| { state.update(cx, |_, cx| cx.notify()); } }).detach();
    if as_branch {
        let hue = state.read(cx).hue.clone();
        cx.subscribe(&hue, { let state = state.clone(); move |_, _: &SliderEvent, cx| { state.update(cx, |_, cx| cx.notify()); } }).detach();
        let (send, receive) = tokio::sync::oneshot::channel();
        backend.send(MessageToBackend::GetSaveGroups { id, channel: send });
        let state = state.clone();
        cx.spawn(async move |cx| {
            let result = receive.await.unwrap_or_else(|_| Err("Launcher backend stopped".into())).map(|groups| groups.iter().any(|g| g.selected));
            _ = cx.update_entity(&state, |state, cx| { state.group = Some(result); cx.notify(); });
        }).detach();
    }
    window.open_dialog(cx, move |dialog, window, cx| cx.update_entity(&state, |state, cx| state.render(dialog, window, cx)));
}
