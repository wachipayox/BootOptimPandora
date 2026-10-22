# Pandora UI workflow

## UI and theme locations

- Launcher pages and navigation: `crates/frontend/src/pages/` and `ui.rs`.
- Shared components: `crates/frontend/src/component/`.
- Settings and appearance selector: `crates/frontend/src/settings/`, especially `appearance.rs`.
- Theme JSON files are user data under `<launcher data>/WachilandLauncher/themes`; the selector watches this folder and applies themes at runtime.

## Build and capture

- Build the Windows launcher from the repository root: `cargo build --release -p pandora_launcher`.
- Capture current UI screens on Windows: `powershell -ExecutionPolicy Bypass -File scripts/capture_ui.ps1`.
- The script copies the exe to a temporary portable directory, uses isolated data, saves PNGs and a manifest, then removes the temporary directory. `WBOMBO_UI_CAPTURE_PAGE` is a debug-only startup route used by the script; supported routes include `instances`, `skins`, `import`, `syncing`, `appearance`, `accounts`, and `instance:<name>`.
- The instances screenshot includes the global profile carousel. The accounts route opens the empty account sheet without signing in. The clean data folder has no installed instance for its detail view or signed update fixture; the manifest lists those unavailable views.

## Visual changes

Every visual change must include before and after screenshots in its PR.

## Sistema de diseño: pendiente (agente 274)
