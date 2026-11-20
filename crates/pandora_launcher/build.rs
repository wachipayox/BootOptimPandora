#[path = "../../package/icon_policy.rs"]
mod icon_policy;

fn main() {
    icon_policy::emit_rerun_inputs();
    windows();
}

#[cfg(windows)]
fn windows() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let selected = if icon_policy::selected_development() {
        "package/development/windows.ico"
    } else {
        "package/windows.ico"
    };
    println!("cargo:rerun-if-changed={}", root.join(selected).display());
    let mut res = winresource::WindowsResource::new();
    res.set_icon(root.join(selected).to_str().unwrap());
    res.set("ProductName", "Wachiland Launcher");
    res.set("FileDescription", "Wachiland Launcher");
    // Publisher metadata shown by Windows (file properties, SmartScreen and
    // Defender reports). Keep CompanyName equal to the code-signing subject.
    res.set("CompanyName", "Wachiland Elite");
    res.set("LegalCopyright", "Wachiland Elite. Basado en Pandora Launcher de Moulberry (MIT).");
    res.set("InternalName", "WachilandLauncher");
    res.set("OriginalFilename", "WachilandLauncher.exe");
    if let Ok(version) = std::env::var("PANDORA_RELEASE_VERSION") {
        res.set("ProductVersion", &version);
        res.set("FileVersion", &version);
        if icon_policy::stable_version(&version) {
            let parts: Vec<u64> = version.split('+').next().unwrap().split('.').map(|p| p.parse().unwrap()).collect();
            let numeric = (parts[0] << 48) | (parts[1] << 32) | (parts[2] << 16);
            res.set_version_info(winresource::VersionInfo::FILEVERSION, numeric);
            res.set_version_info(winresource::VersionInfo::PRODUCTVERSION, numeric);
        }
    }
    res.compile().unwrap();
}

#[cfg(not(windows))]
fn windows() {}
