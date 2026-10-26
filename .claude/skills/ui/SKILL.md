# Pandora UI workflow

## UI and theme locations

- Launcher pages and navigation: `crates/frontend/src/pages/` and `ui.rs`.
- Shared components: `crates/frontend/src/component/`.
- Settings and appearance selector: `crates/frontend/src/settings/`, especially `appearance.rs`.
- Theme JSON files are user data under `<launcher data>/WachilandLauncher/themes`; the selector watches this folder and applies themes at runtime.

## Build and capture

- Build the Windows launcher from the repository root: `cargo build --release -p pandora_launcher`.
- Capture current UI screens on Windows: `powershell -ExecutionPolicy Bypass -File scripts/capture_ui.ps1 -DesignViewport`. This uses a 1280 x 860 logical client area, Windows per-monitor DPI and records physical dimensions/DPI in the manifest. It excludes DWM margins and keeps the window on-screen.
- The script copies the exe to a temporary portable directory, uses isolated data, saves PNGs and a manifest, then removes the temporary directory. `WBOMBO_UI_CAPTURE_PAGE` is a debug-only startup route used by the script; supported routes include `instances`, `skins`, `import`, `syncing`, `appearance`, `accounts`, `instance:<name>`, `update:<name>`, `components`, and `components-dialog`, `home-menu`, and `home-news`.
- The instances screenshot includes the global profile carousel. The accounts route opens the Home account menu without signing in. Use `-Components` to capture the shared controls and dialog. `-HomeScreens` adds the instance menu and the news destination; it waits for a process-specific Home readiness signal before saving. Test-profile captures must use the developer channel in the owned portable config so cosmetic test-profile catalog data is visible.
- For instance/update captures use an owned `profile_test-…` via `-TestProfile`, optionally `-Revision rev_…`. The CLI creates `agente-274-ui` and deletes it in cleanup. Publish/repin a newer revision of that owned test profile before the update capture. Never alter Beta or another agent's profile.
- `-PortableDirectory` reuses caller-owned isolated data (the caller cleans it up), and `-Executable` chooses a binary. `-SkipUpdateCapture` supports baseline binaries without the new update route. Capture data contains community presentation: retain it in private wbombo, not this public repository.

## Visual changes

Every visual change must include before and after screenshots in its PR.

## Wachiland design foundation

`assets/theme/tokens.json` is the approved source. Regenerate the Rust palette (including light, metrics and typography) and GPUI JSON with `python scripts/generate_theme.py`. The embedded default is `Wachiland Dark`; `theme.rs` holds component measurements that the handoff defines outside the tokens. Bundled Noto Sans / Chakra Petch carry their OFL files in `assets/fonts/`.

Use `component/design.rs` for new screens: button A, glass Play (sample the actual app surface behind its window bounds), NO/SÍ, chips, tabs, field, setting row, eyebrow and dialog. Preserve native keyboard interaction. Existing layouts migrate in subsequent PRs; do not mistake the capture-only component review for the redesigned Home screen.

After changing the vendored GPUI component, invalidate both cached profiles with `cargo clean -p gpui-component` and `cargo clean --release -p gpui-component`. Cargo treats vendored git dependencies as immutable; otherwise a local build can reuse stale API/paint code. Update the edited source hashes in its `.cargo-checksum.json` too.

The one-exe distribution writes both font OFL notices to `<launcher data>/licenses/fonts/` at startup.

Home lives in `pages/home.rs`; the existing signed launch gate and native account/instance actions remain the execution boundary. Cosmetic JSON/images are cached separately in `home-presentation`. Game-hour totals and daily buckets are local, and days without recorded data stay empty.

News lives in `pages/news.rs`, with revision attachments in `pages/news_changelog.rs`.
`-NewsScreens` captures its native states; `-Pages` selects explicit capture routes.
`-NewsFixture <private JSON>` supplies local cosmetic input for capture routes only.
News captures include native Tab focus and scrolled article/body media.
News captures wait for process-specific feed/media readiness. Fixtures stay outside
this public repo. Restricted Markdown is validated in `schema::news_markdown` against
Distribution's shared vectors before native TextView rendering. Its explicit image
map contains only backend-verified cache paths; an absent image renders alt text.

Instance captures use `-InstanceScreens` with an owned CLI fixture: Content filters, overrides, logs/crashes, Changelog, instance Settings, Create child and Duplicate. The live-output capture feeds synthetic messages through the native viewer; it never starts Minecraft. Explicit routes are `instance-mods:agente-274-…`, `instance-resources:…`, `instance-shaders:…`, `instance-overwrites:…`, `instance-logs:…`, `instance-crashes:…`, `instance-live:…`, `instance-changelog:…`, `instance-settings:…`, `derived:…`, and `duplicate:…`. The script waits for the real data dependencies before saving. Update captures also wait for the exact skipped-revision changelog. Keep every screenshot private in wbombo.

For the owned instance routes, `-CaptureState advanced|version|expanded|editing` opens the existing advanced settings, version editor, expanded update changelog or local overrides editor. This changes presentation only. Settings and timeline captures include middle/bottom scrolling; native Tab focus is captured for instance pages/dialogs.

Launcher capture routes: `launcher-general`, `launcher-appearance`, `launcher-network`, `launcher-distribution`, `launcher-ignored`, `launcher-windows`, `skins-url`, `skins-copy`, `account-offline`, `add-instance`, `add-server`, `add-local`, and `content-browser:agente-274-…`. `-LauncherScreens` collects them along with the existing skins/accounts/import/sync routes. Dialog routes open the real native dialog without confirming any action. Capture Distribution settings with a temporary portable directory explicitly configured with `https://example.invalid` so endpoint values and credentials never enter the evidence.

Launcher review also supports `launcher-about`, `skins-import`,
`launcher-defender-dialog`, and `delete:agente-274-…`, `export:…`, `groups:…`.
These routes open native views without confirming deletion, imports or Defender
changes. Distribution captures temporarily replace the isolated endpoint with
`https://example.invalid` and restore the original configuration after close,
including failure cleanup. Private compiled defaults are never safe capture data.

For paired GPU/desktop evidence, build with `cargo build --release -p pandora_launcher --features ui-capture`
and pass `-Framebuffer` to the capture script. Each normal PNG retains the Windows
desktop capture; its `-gpu.png` sibling reads the same native GPUI scene before
presentation. The manifest identifies both methods and checks matching client
dimensions. Use the pair to distinguish renderer output from presentation/capture
artifacts; never describe a framebuffer image as a desktop screenshot. Readback
copies the complete framebuffer inside the fresh draw, before Present rotates or discards buffers, on the window thread;
it does not replay scene sprites that animation may have retired. PNG encoding
runs on the background executor. The
optional feature and file-request loop are absent from ordinary launcher builds.
The manifest hashes the executable actually used, including with reused portable
data. Pass `-ExecutableSourceCommit <verified SHA>` for a binary built from a
different checkout; the script revision is recorded separately and never implies
the binary's source. An explicit executable replaces the isolated portable copy.
`launcher-theme-joke` captures the same dialog opened by the non-dark theme chips.

Launcher update review uses `-LauncherUpdateScreens -LauncherUpdateFixture <private JSON>`
with the optional `ui-capture` build. The fixture contains `releases` in the
Distribution read schema and stays in private wbombo. Routes are
`launcher-update-indicator`, `launcher-update`, `launcher-update-expanded`,
`launcher-update-progress`, `launcher-update-failed`, `launcher-update-checking`,
`launcher-update-current`, `launcher-update-search-error` and
`launcher-update-verification-error`. They render native views with cosmetic
local input and a fixed source marker that the backend rejects before download
or installation. The script records the fixture SHA-256 and readiness signal.
`-HomeScreens` also waits for Home/artwork/glass behind update dialogs.
Do not publish a release to create screenshots, and do not describe fixture
captures as proof of a real server update or successful self-installation.

Frameless-window review uses `-Framebuffer -HitTest`: painted region sidecars
and actual HWND `WM_NCHITTEST` results cover client controls, caption gaps and
eight resize edges, with separate clipped-control checks. This sends mouse-move
packets, never button-down/up or clicks; it does not prove actual dragging or
activation. `-DragRegions` adds capture-only diagnostic colors. Use
`-HitTestPointOnly` with the corrected native backend to query the supplied
message coordinates without moving the cursor or sending mouse-move packets.
`-DesignViewport -ViewportWidth 960 -ViewportHeight 600` for minimum-width
review. `window-dialog` opens a real, nonresizable secondary window;
`sidebar-scroll` adds cosmetic rows without backend instances. If remote test
profile provisioning is unavailable, `WBOMBO_UI_CAPTURE_LOCAL_INSTANCE` accepts
only `agente-274-window-drag` and creates that owned Vanilla instance through
the existing backend message. Clean up its exact owned path afterwards.
`backdrop-probe -ResizeProbe` isolates the app surface and retains the first
fresh GPU draw armed before each resize, instead of waiting for the backdrop
worker; compare it with the settled frame to detect blank resize frames.

All ordinary capture routes bypass startup presentation. `-StartupScreens`
adds deterministic vector reference frames (`startup-frame:<seconds>`),
`startup-splash`, `startup-intro:<seconds>` and `startup-transition:<fraction>`.
The reference routes omit the native border for full-frame pixel comparison;
transition captures compose the real interface and the final vector frame.
`startup-bench:<seconds>` continuously renders a fixed frame and logs CPU time
from native paint entry through path tessellation/scene submission, excluding
GPU execution and Present. Capture both 1920×1080 and 3840×2160 physical pixels;
the manifest records DPI and viewport size. Keep source/reference images private.
