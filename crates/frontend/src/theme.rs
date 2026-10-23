//! Application design tokens. The generated palette retains both specified modes;
//! only the approved dark mode is installed as the default GPUI theme.
pub use crate::theme_palette::{Palette, dark, light};
pub use crate::theme_palette::dark::*;
use gpui::App;
pub const SANS: &str = "Noto Sans";
pub const DISPLAY: &str = "Chakra Petch";
pub const SPACE: [f32; 7] = [4., 8., 12., 16., 24., 32., 48.];
pub const RADIUS: f32 = 0.;
pub const NOTCH: f32 = 6.;
pub const BUTTON_H: f32 = 34.;
pub const FIELD_H: f32 = 32.;
pub const PLAY_H: f32 = 52.;
pub const PLAY_MIN_W: f32 = 250.;
pub const PROGRESS_H: f32 = 3.;
pub const SIDEBAR_W: f32 = 220.;
pub const FOCUS_RING: f32 = 2.;
pub const FOCUS_GAP: f32 = 2.;
pub const DIALOG_BG: u32 = 0x0a0a0cf2;
pub const DIALOG_OUTLINE: u32 = 0xffffff0d;
pub fn init(cx: &mut App) {
    gpui_component::ThemeRegistry::global_mut(cx)
        .load_themes_from_str(include_str!("../../../assets/theme/wachiland.json"))
        .expect("embedded Wachiland theme must be valid");
}
