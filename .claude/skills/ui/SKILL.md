# Pandora UI workflow

## UI and theme locations

- Launcher pages and navigation: `crates/frontend/src/pages/` and `ui.rs`.
- Shared components: `crates/frontend/src/component/`.
- Settings and appearance selector: `crates/frontend/src/settings/`, especially `appearance.rs`.
- Theme JSON files are user data under `<launcher data>/WachilandLauncher/themes`; the selector watches this folder and applies themes at runtime.

## Build and capture

- Build the Windows launcher from the repository root: `cargo build --release -p pandora_launcher`.
- Capture current UI screens on Windows: `powershell -ExecutionPolicy Bypass -File scripts/capture_ui.ps1`.
- The script copies the exe to a temporary portable directory, uses isolated data, saves PNGs and a manifest, then removes the temporary directory. `WBOMBO_UI_CAPTURE_PAGE` is a debug-only startup route used by the script; supported routes include `instances`, `skins`, `import`, `syncing`, `appearance`, `accounts`, `instance:<name>`, `update:<name>`, `components`, and `components-dialog`.
- The instances screenshot includes the global profile carousel. The accounts route opens the empty account sheet without signing in. Use `-Components` to capture the shared controls and dialog.
- For instance/update captures use an owned `profile_test-…` via `-TestProfile`, optionally `-Revision rev_…`. The CLI creates `agente-274-ui` and deletes it in cleanup. Publish/repin a newer revision of that owned test profile before the update capture. Never alter Beta or another agent's profile.
- `-PortableDirectory` reuses caller-owned isolated data (the caller cleans it up), and `-Executable` chooses a binary. `-SkipUpdateCapture` supports baseline binaries without the new update route. Capture data contains community presentation: retain it in private wbombo, not this public repository.

## Visual changes

Every visual change must include before and after screenshots in its PR.

## Wachiland design foundation

`assets/theme/tokens.json` is the approved source. Regenerate the Rust palette (including light, metrics and typography) and GPUI JSON with `python scripts/generate_theme.py`. The embedded default is `Wachiland Dark`; `theme.rs` holds component measurements that the handoff defines outside the tokens. Bundled Noto Sans / Chakra Petch carry their OFL files in `assets/fonts/`.

Use `component/design.rs` for new screens: button A, glass Play (pass its prepared cached artwork crop), NO/SÍ, chips, tabs, field, setting row, eyebrow and dialog. Preserve native keyboard interaction. Existing layouts migrate in subsequent PRs; do not mistake the capture-only component review for the redesigned Home screen.

After changing the vendored GPUI component, invalidate both cached profiles with `cargo clean -p gpui-component` and `cargo clean --release -p gpui-component`. Cargo treats vendored git dependencies as immutable; otherwise a local build can reuse stale API/paint code. Update the edited source hashes in its `.cargo-checksum.json` too.

The one-exe distribution writes both font OFL notices to `<launcher data>/licenses/fonts/` at startup.
