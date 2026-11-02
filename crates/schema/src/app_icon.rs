//! Compile-time branding shared by launcher shortcuts and their native preview.
#[cfg(development_icon)]
pub const ICO: &[u8] = include_bytes!("../../../package/development/windows.ico");
#[cfg(not(development_icon))]
pub const ICO: &[u8] = include_bytes!("../../../package/windows.ico");
#[cfg(development_icon)]
pub const ICNS: &[u8] = include_bytes!("../../../package/development/mac.icns");
#[cfg(not(development_icon))]
pub const ICNS: &[u8] = include_bytes!("../../../package/mac.icns");
#[cfg(development_icon)]
pub const PNG: &[u8] = include_bytes!("../../../package/development/windows_icons/icon_256x256.png");
#[cfg(not(development_icon))]
pub const PNG: &[u8] = include_bytes!("../../../package/windows_icons/icon_256x256.png");
