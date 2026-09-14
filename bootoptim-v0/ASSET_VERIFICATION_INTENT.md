# Agent 161 — asset verification intent contract

Base: PR #9 head `6c1e3bcb85b49769b330a630bb3b3f986c224ab5`.

This change preserves launch intent only. It does **not** enable the USN cache, does not alter `AssetUsnCacheRuntime::can_skip_sha1`, and does not change Pandora's current asset SHA-1/download/repair behavior.

## Code facts

The existing GUI Start action in `crates/frontend/src/pages/instance/instance_page.rs` calls `root::start_instance(...)`. Quick-play launches also enter the same frontend helper. `root::start_instance` sends `MessageToBackend::StartInstance` with one `ModalAction` clone.

`crates/bridge/src/message.rs` contains the two launch message forms `StartInstance` and `StartInstanceByName`. At this revision it contains no stock `Repair`, `Verify`, `Redownload`, asset-repair, or full-verification launch action. `RequestMetadata { force_reload }`, `DownloadAllMetadata`, `UpdateCheck`, `InstallContent`, and `UpdateContent` are metadata/content operations; none is routed into Mojang asset-object verification as a user repair launch.

`crates/backend/src/backend_handler.rs` handles `StartInstance` by passing the same `ModalAction` into its private `start_instance`. `StartInstanceByName` constructs a default `ModalAction` in the backend. `BackendState::start_instance` passes the action unchanged to `Launcher::launch`.

`Launcher::launch` passes that same action to `load_assets`. `load_assets` creates the assets `ProgressTracker` from the action and calls `do_asset_objects_load`. `do_asset_objects_load` currently performs Pandora's full per-object SHA-1 check and downloads/replaces an object on mismatch. It has no repair-mode parameter today.

Therefore there is currently **no distinct stock UI repair action to preserve**. Treating any existing button as an explicit repair would be an inference and would misdescribe stock behavior.

## Minimal semantic contract

`bridge::modal_action` now owns:

```text
AssetVerificationMode::Normal
AssetVerificationMode::FullVerification
```

`FullVerification` is deliberately the default. It does not claim that the user clicked a repair button; it means only that a future optimization is **not authorized** to skip full verification. This makes unknown, legacy and future callers fail closed.

The existing GUI launch helper explicitly constructs `ModalAction::normal_launch()`. The mode is immutable for that action, survives `Arc` cloning across the frontend/backend message boundary and async handlers, and is copied into every `ProgressTracker` created from the action. The assets tracker can therefore expose the same authority at the exact `load_assets -> do_asset_objects_load` boundary without adding UI or inferring intent from filesystem state.

`StartInstanceByName` is intentionally left on the default `FullVerification` mode. That path currently has no stock UI-origin intent object proving it is the same normal GUI action. Keeping it conservative preserves current correctness and prevents a future cache from silently treating a legacy/shortcut launch as cache-eligible. If that route is later audited and intentionally declared a normal launch, it should construct/pass `Normal` explicitly in a separate semantic change.

## Future explicit repair action

If Pandora later adds a stock Repair / Verify files / Redownload action, that action must explicitly construct or retain `FullVerification` at its UI/command authority and carry the same action through the existing bridge/backend path. No file event, mtime, cache state, missing-file observation, or download result may synthesize this intent.

The enum deliberately uses `FullVerification` rather than `ExplicitRepair` because this revision has no demonstrated stock repair UI. This avoids falsely labelling legacy/unknown callers as user-requested repair while still providing the fail-closed state PR #9 needs.

## How PR #9 must consume this

A future PR #9 integration may consider USN reuse only when all of its existing security/identity/TOCTOU gates pass **and** the assets tracker reports exactly:

```text
AssetVerificationMode::Normal
```

Any other mode or inability to obtain the mode must execute Pandora's existing full SHA-1 path. PR #9 should consume the mode at the asset boundary; it should not alter this intent producer in the same cache-enabling change. `can_skip_sha1()` remains hard-wired to `false` in this PR.

## Tests and CI

Bridge tests pin both sides of the contract:

- `ModalAction::default()` is `FullVerification`, and a tracker created from it remains `FullVerification`;
- `ModalAction::normal_launch()` remains `Normal` after cloning and tracker creation, covering the representation used across message/handler/async ownership transitions.

The cross-platform build workflow runs these targeted tests before the existing release build and before any future cache activation is considered.

No performance or TTMM claim is made by this change.
