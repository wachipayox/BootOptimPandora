use bridge::message::{ShortcutError, ShortcutIconChoice};
use sha2::Digest;
use std::{
    io::Cursor,
    path::{Path, PathBuf},
};

/// Keep the icon independent of its original image and use a new cache key when it changes.
pub fn prepare_icon(root: &Path, choice: ShortcutIconChoice) -> Result<PathBuf, ShortcutError> {
    if matches!(choice, ShortcutIconChoice::Launcher) {
        #[cfg(target_os = "windows")]
        let bytes = include_bytes!("../../../package/windows.ico").as_slice();
        #[cfg(target_os = "macos")]
        let bytes = include_bytes!("../../../package/mac.icns").as_slice();
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        let bytes = include_bytes!("../../../package/windows_icons/icon_256x256.png").as_slice();
        return cache_icon(root, bytes);
    }
    let bytes = match choice {
        ShortcutIconChoice::Instance(bytes) => bytes.to_vec(),
        ShortcutIconChoice::Custom(path) => {
            if std::fs::metadata(&path).map_err(image_error)?.len() > 32 * 1024 * 1024 {
                return Err(ShortcutError::Image);
            }
            std::fs::read(path).map_err(image_error)?
        },
        ShortcutIconChoice::Launcher => unreachable!(),
    };
    let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(image_error)?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let source = reader.decode().map_err(image_error)?;
    let filter = if source.width() <= 256 && source.height() <= 256 {
        image::imageops::FilterType::Nearest
    } else {
        image::imageops::FilterType::Lanczos3
    };
    let resized = source.resize(256, 256, filter).to_rgba8();
    let mut square = image::RgbaImage::new(256, 256);
    image::imageops::overlay(
        &mut square,
        &resized,
        ((256 - resized.width()) / 2).into(),
        ((256 - resized.height()) / 2).into(),
    );
    let mut encoded = Cursor::new(Vec::new());
    #[cfg(target_os = "windows")]
    let format = image::ImageFormat::Ico;
    #[cfg(not(target_os = "windows"))]
    let format = image::ImageFormat::Png;
    square.write_to(&mut encoded, format).map_err(image_error)?;
    #[cfg(target_os = "macos")]
    let bytes = icns_from_png(&encoded.into_inner());
    #[cfg(not(target_os = "macos"))]
    let bytes = encoded.into_inner();
    cache_icon(root, &bytes)
}
fn image_error(error: impl std::fmt::Display) -> ShortcutError {
    log::warn!("Could not prepare shortcut image: {error}");
    ShortcutError::Image
}
fn cache_icon(root: &Path, bytes: &[u8]) -> Result<PathBuf, ShortcutError> {
    #[cfg(target_os = "windows")]
    let extension = "ico";
    #[cfg(target_os = "macos")]
    let extension = "icns";
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let extension = "png";
    let directory = root.join("shortcut-icons");
    let path = directory.join(format!("{}.{extension}", hex::encode(sha2::Sha256::digest(bytes))));
    let write = || -> std::io::Result<()> {
        std::fs::create_dir_all(&directory)?;
        if std::fs::read(&path).ok().as_deref() != Some(bytes) {
            std::fs::write(&path, bytes)?;
        }
        Ok(())
    };
    write().map_err(|error| {
        log::warn!("Could not store shortcut icon: {error}");
        ShortcutError::Create
    })?;
    Ok(path)
}
#[cfg(any(target_os = "macos", test))]
fn icns_from_png(png: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(png.len() + 16);
    bytes.extend_from_slice(b"icns");
    bytes.extend_from_slice(&((png.len() + 16) as u32).to_be_bytes());
    bytes.extend_from_slice(b"ic08"); // 256 x 256 PNG-backed icon.
    bytes.extend_from_slice(&((png.len() + 8) as u32).to_be_bytes());
    bytes.extend_from_slice(png);
    bytes
}
#[cfg(target_os = "windows")]
pub fn create_shortcut(mut path: PathBuf, name: &str, bin: &Path, args: &[&str], icon: &Path) -> anyhow::Result<()> {
    if !has_extension(&path, "lnk") {
        path.add_extension("lnk");
    }
    let mut link = mslnk::ShellLink::new(bin).map_err(|e| anyhow::anyhow!("{e:?}"))?;
    link.set_arguments(Some(crate::join_windows_shell(args)));
    link.set_name(Some(name.into()));
    link.set_icon_location(Some(icon.to_str().ok_or_else(|| anyhow::anyhow!("Invalid icon path"))?.to_owned()));
    link.create_lnk(path).map_err(|e| anyhow::anyhow!("{e:?}"))
}
#[cfg(target_os = "linux")]
pub fn create_shortcut(mut path: PathBuf, name: &str, bin: &Path, args: &[&str], icon: &Path) -> anyhow::Result<()> {
    if !has_extension(&path, "desktop") {
        path.add_extension("desktop");
    }
    let bin = bin.to_str().ok_or_else(|| anyhow::anyhow!("Invalid executable path"))?;
    let exec = std::iter::once(bin)
        .chain(args.iter().copied())
        .map(desktop_argument)
        .collect::<Vec<_>>()
        .join(" ");
    std::fs::write(
        &path,
        format!(
            "[Desktop Entry]\nType=Application\nVersion=1.0\nName={}\nExec={exec}\nIcon={}\nCategories=Games;Minecraft;Launcher;\n",
            desktop_value(name),
            desktop_value(&icon.to_string_lossy())
        ),
    )?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}
#[cfg(any(target_os = "linux", test))]
fn desktop_argument(value: &str) -> String {
    let quoted = format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace(char::from(96), "\\`")
            .replace('$', "\\$")
            .replace('%', "%%")
    );
    // Desktop string unescaping happens before Exec argument unquoting.
    // https://specifications.freedesktop.org/desktop-entry/latest/exec-variables.html
    desktop_value(&quoted)
}

#[cfg(any(target_os = "linux", test))]
fn desktop_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\n', "\\n").replace('\r', "\\r").replace('\t', "\\t")
}
#[cfg(target_os = "macos")]
pub fn create_shortcut(mut path: PathBuf, name: &str, bin: &Path, args: &[&str], icon: &Path) -> anyhow::Result<()> {
    if !has_extension(&path, "app") {
        path.add_extension("app");
    }
    let contents = path.join("Contents");
    let macos = contents.join("MacOS");
    let resources = contents.join("Resources");
    std::fs::create_dir_all(&macos)?;
    std::fs::create_dir_all(&resources)?;
    std::fs::copy(icon, resources.join("Shortcut.icns"))?;
    let name = xml_value(name);
    std::fs::write(
        contents.join("Info.plist"),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>run.sh</string>
<key>CFBundleIdentifier</key><string>com.moulberry.pandoralauncher.Shortcut</string>
<key>CFBundleName</key><string>{name}</string>
<key>CFBundleDisplayName</key><string>{name}</string>
<key>CFBundleIconFile</key><string>Shortcut.icns</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>0</string>
</dict></plist>"#
        ),
    )?;
    let bin = bin.to_str().ok_or_else(|| anyhow::anyhow!("Invalid executable path"))?;
    let exec = shell_words::join(std::iter::once(bin).chain(args.iter().copied()));
    let script = macos.join("run.sh");
    std::fs::write(&script, format!("#!/bin/sh\nexec {exec}\n"))?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(script, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}
#[cfg(any(target_os = "macos", test))]
fn xml_value(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension().is_some_and(|v| v.as_encoded_bytes() == extension.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("pandora-shortcut-test-{}", uuid::Uuid::from_bytes(rand::random())));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn source_png() -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(8, 4, image::Rgba([40, 80, 120, 255]));
        let mut encoded = Cursor::new(Vec::new());
        image.write_to(&mut encoded, image::ImageFormat::Png).unwrap();
        encoded.into_inner()
    }
    #[test]
    fn custom_icon_survives_its_source_and_preserves_proportions_and_pixels() {
        let scratch = Scratch::new();
        let source = scratch.0.join("source.png");
        let png = source_png();
        std::fs::write(&source, &png).unwrap();
        let cached = prepare_icon(&scratch.0, ShortcutIconChoice::Custom(source.clone())).unwrap();
        std::fs::remove_file(source).unwrap();
        let bytes = std::fs::read(&cached).unwrap();
        #[cfg(target_os = "macos")]
        let bytes = &bytes[16..];
        let decoded = image::load_from_memory(&bytes).unwrap().to_rgba8();
        assert_eq!(decoded.dimensions(), (256, 256));
        assert_eq!(decoded.get_pixel(128, 128).0, [40, 80, 120, 255]);
        assert_eq!(decoded.get_pixel(0, 128).0, [40, 80, 120, 255]);
        assert_eq!(decoded.get_pixel(128, 63).0[3], 0);
        assert_eq!(decoded.get_pixel(128, 64).0[3], 255);
        assert_eq!(decoded.get_pixel(0, 0).0[3], 0);
        let same = prepare_icon(&scratch.0, ShortcutIconChoice::Instance(png.into())).unwrap();
        assert_eq!(cached, same);
    }
    #[test]
    fn invalid_images_and_unwritable_destinations_report_different_errors() {
        let scratch = Scratch::new();
        assert_eq!(
            prepare_icon(&scratch.0, ShortcutIconChoice::Instance(vec![1, 2, 3].into())),
            Err(ShortcutError::Image)
        );
        let blocked = scratch.0.join("blocked");
        std::fs::write(&blocked, b"not a directory").unwrap();
        assert_eq!(
            prepare_icon(&blocked, ShortcutIconChoice::Instance(source_png().into())),
            Err(ShortcutError::Create)
        );
        let icon = prepare_icon(&scratch.0, ShortcutIconChoice::Launcher).unwrap();
        assert!(
            create_shortcut(
                blocked.join("output"),
                "Prueba",
                &std::env::current_exe().unwrap(),
                &["--run-instance", "Prueba con espacios"],
                &icon
            )
            .is_err()
        );
    }
    #[test]
    fn icns_contains_a_decodable_full_color_icon_and_valid_container_lengths() {
        let image = image::RgbaImage::from_pixel(256, 256, image::Rgba([40, 80, 120, 255]));
        let mut png = Cursor::new(Vec::new());
        image.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let encoded = icns_from_png(&png.into_inner());
        assert_eq!(&encoded[..4], b"icns");
        assert_eq!(u32::from_be_bytes(encoded[4..8].try_into().unwrap()) as usize, encoded.len());
        assert_eq!(&encoded[8..12], b"ic08");
        assert_eq!(u32::from_be_bytes(encoded[12..16].try_into().unwrap()) as usize, encoded.len() - 8);
        assert_eq!(
            image::load_from_memory(&encoded[16..]).unwrap().to_rgba8().get_pixel(0, 0).0,
            [40, 80, 120, 255]
        );
    }
    #[test]
    fn launcher_file_values_cannot_inject_fields_or_expand_exec_placeholders() {
        assert_eq!(desktop_argument("Pack %f $name"), "\"Pack %%f \\\\$name\"");
        assert_eq!(desktop_value("A\nIcon=other"), "A\\nIcon=other");
        assert_eq!(xml_value("A<&\"'"), "A&lt;&amp;&quot;&apos;");
    }
    #[cfg(target_os = "windows")]
    #[test]
    fn windows_link_contains_the_selected_cached_icon_and_quoted_instance_argument() {
        let scratch = Scratch::new();
        let icon = prepare_icon(&scratch.0, ShortcutIconChoice::Instance(source_png().into())).unwrap();
        create_shortcut(
            scratch.0.join("start"),
            "Prueba",
            &std::env::current_exe().unwrap(),
            &["--run-instance", "Prueba con espacios"],
            &icon,
        )
        .unwrap();
        let link = std::fs::read(scratch.0.join("start.lnk")).unwrap();
        for value in [
            icon.to_string_lossy().to_string(),
            "--run-instance \"Prueba con espacios\"".into(),
        ] {
            let wide: Vec<u8> = value.encode_utf16().flat_map(u16::to_le_bytes).collect();
            assert!(
                link.windows(wide.len()).any(|p| p == wide) || link.windows(value.len()).any(|p| p == value.as_bytes())
            );
        }
    }
}
