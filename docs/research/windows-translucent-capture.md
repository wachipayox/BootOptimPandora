# Windows translucent surfaces: capture boundary

Decision in [PR #116](https://github.com/wachipayox/BootOptimPandora/pull/116): retain the approved translucent colors and provide paired native-framebuffer/Windows capture evidence. No shader or global display-setting workaround is adopted.

## Question and evidence

Windows captures on the local PC showed black patches in translucent headers and dialogs. A minimal GPUI scene reproduced them without launcher data: a `#f08040` to `#060607` gradient behind `#0a0a0cf2` rectangles, with solid/gradient fills and with/without borders. Opaque rectangles were unaffected.

Neither a constant two-stop gradient, initialized gradient vertex outputs, `PrintWindow`, nor disabling DirectComposition removed the patches. The vertex-initialization experiment was reverted.

Reading the GPU render target produced a clean image of the minimal scene: in the inspected 100 × 100 patch, red was at least 12, with no zero-red pixels; the Windows capture had 3,363 zero-red pixels there. Later paired captures of settled launcher Settings and the offline-account dialog at source `234042809` also showed clean readback images alongside affected Windows images. This is not solely a first-frame observation.

This establishes a difference between renderer output and Windows presentation/capture on that machine. It does not identify a particular driver, compositor or display component, and does not establish that every Windows machine is affected.

## Retained implementation

- `ui-capture` enables native framebuffer copying through a narrow `native-capture` feature, without enabling GPUI's test dependencies.
- The script retains its Windows PNG and adds a separately identified `-gpu.png` of the native scene, including focused/scrolled states. It verifies matching client dimensions and records executable SHA-256, caller-supplied binary source and script revision separately.
- GPU readback copies the already drawn render target after a fresh frame on the window thread. The test-only `render_to_image` API replays a scene; both a direct call and an `on_next_frame` call could hit evicted animated-image atlas entries. Live capture therefore uses a separate copy-only API, sharing the existing staging/readback implementation. PNG encoding runs on the background executor. The request loop is excluded from ordinary builds.

Capture instructions have one home: [the UI workflow](../../.claude/skills/ui/SKILL.md). Community screenshots remain in the private review linked from the PR. A framebuffer image must never be described as a desktop screenshot.

## Reopening criterion

Reopen renderer changes if the defect occurs in settled GPU readback, or if a reproducible presentation failure can be localized and corrected without changing the approved appearance. Preserve paired evidence and test the proposed correction against the minimal scene and real launcher surfaces. A clean build alone does not prove a visual fix.
