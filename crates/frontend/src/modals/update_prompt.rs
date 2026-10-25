use bridge::handle::BackendHandle;
use gpui::{App, Window};
use schema::pandora_update::UpdatePrompt;

pub fn open_update_prompt(update: UpdatePrompt, handle: BackendHandle, window: &mut Window, cx: &mut App) {
    crate::launcher_updates::open_prompt(update, handle, window, cx);
}
