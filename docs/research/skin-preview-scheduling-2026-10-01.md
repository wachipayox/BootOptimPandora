# Pandora vs Wachiland skin scheduling audit — 2026-10-01

Source baseline: local gitHEAD87e4f7f19, player_model/widget lineage03d067cd3 (upstream Pandora zoom viewer). Original widget advances pose in Render and drives window.request_animation_frame; original model posts completed worker image immediately. Optimized software renderer, exact texels and native resolution are retained.

## Confirmed differences introduced locally

1. Widget animation used background_executor.timer and resetlast_render=now on each allowed advance. At60Hz, close-to16.666ms callbacks can fall just short of threshold; some are skipped and resetting the phase accumulates drift. This is not a vsync source.
2. A second timer in model completion delayed already-rendered images until16.666ms (formerly33ms), independent of widget timer, actual UI latency and platform refresh. It added explicit latency after CPU rendering finished.
3. Window focus loss paused the preview entirely; original Pandora continued animation. This is visible when user watches launcher on monitor3 while typing into Codex/another window on monitor1. GPUI already throttles inactive windows, so explicit pause was an observable regression.
4. Global carousel had its own recurring50ms offpage notify loop; primary agent independently fixed visibility gating. Its total contribution remains unmeasured and is not attributed exclusively to skins.

## Implemented correction

-Exactly one outstanding Window::on_next_frame callback (the same native frame source behind upstream request_animation_frame).
-Pose advances ONLY through that callback, not from completion/UI-input rerenders. Timers removed entirely from player_model/widget.
-An anchored30Hz pose deadline, with2ms callback scheduling margin, avoids phase-reset drift; delayed frames are coalesced without catch-up rendering bursts. This is scheduling tolerance, not geometric pixel epsilon or quality change.
-Completed images publish immediately; no completion sleep. Native resolution/renderer stays unchanged.
-No freeze on keyboard-focus loss. Offpage stops requesting callbacks because scheduling occurs only in WidgetRender; one already-pending weak callback may run once. No permanent background loop.
-Single renderer task / scratch ownership / invalid-texture negative cache unchanged. No output-frame queue; current input after worker completion drives next render.

## Validation and limits

Two standalone tests extracted from production cadence code pass:
-A60Hz native-callback timeline with jitter±0.8ms yields exactly one pose every2callbacks across120frames (61poses incl initial frame).
-Duplicate callbacks do not advance twice;2-second stalls coalesce to one update; resetting after pause begins cleanly.

These are deterministic scheduling tests, NOT measured UI frame/presentation telemetry. Source inspection confirms artificial delay and focus freeze; it does not prove they were the only cause of observed stutter. Full frontend build, actual launcher responsiveness and monitor3visual check are primary-agent gates.

GPU upload/drop_image paths are inherited and remain outside this measurement. CPU benchmark gains do not imply GUI FPS gains. Native30Hz deliberately targets lower resource use; tests must evaluate cadence regularity and freshness, not just count worker frames.

Artifacts: upstream-widget.rs/upstream-model.rs source snapshots; cadence.rs production helper extract; cadence-tests.exe. Copy this audit to durable launcher docs with primary integration.
