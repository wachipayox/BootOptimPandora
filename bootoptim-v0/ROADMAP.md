# BootOptim launcher roadmap

These are product requirements for a later launcher phase. They are not part
of the v0 AppCDS measurement prototype and must not silently expand its scope.

## Self-contained prerequisite installation

- The production installer must detect and, with clear user consent, install
  launcher prerequisites such as the supported Microsoft Visual C++ Redistributable.
- A clean Windows installation must not fail silently because a required native
  runtime DLL is absent. The installer must report a prerequisite failure and
  offer a documented recovery path before the launcher is first run.
- Bundled prerequisite installers must be versioned, architecture-appropriate,
  and independently updatable; the launcher itself must not depend on a random
  system-wide DLL already happening to be present.

## Optional Windows security integration

- On first launch, the installed launcher may explain that real-time scanning
  can make a large modpack materially slower on older storage. It must offer a
  **clearly optional**, administrator-approved and reversible action; never
  disable Defender or alter protection globally.
- Any exclusion must be as narrow as technically possible: the installed,
  versioned launcher executable and launcher-owned generated cache only. Do
  not broadly exclude the game directory, arbitrary user files, downloaded
  mods or an entire drive.
- The UI must state exactly which Windows setting/path will change, record the
  installed rule/exclusion, provide a one-click removal path, and treat denied
  elevation/tamper protection as a normal non-fatal outcome.
- Do not add a firewall exception by default. First establish that a concrete
  launcher feature requires inbound traffic; normal outbound update/login
  traffic should use Windows' default outbound policy. Any later firewall rule
  follows the same explicit-consent, narrow-path and reversible contract.

## Profiles and AppCDS

- The launcher will offer named configuration profiles for the modpack.
- Every profile owns an independent AppCDS identity/cache namespace. Switching
  profiles must never consume another profile's archive.
- Switching away and back to an unchanged profile must reuse its valid archive;
  it must not retrain merely because another profile was selected in between.
- Pack, Java, launch argument, resource selection and profile-local
  launch-affecting configuration changes continue to invalidate only that
  profile's archive. Shared/global inputs must be represented explicitly rather
  than guessed.

## Persistent per-profile game layout

- Every profile owns a persistent prepared `.minecraft` layout and its own
  managed-layout manifest/state; installing, updating or switching profile
  identity publishes changes for that profile rather than rebuilding at Start.
- Managed pack files and user-local files are separate ownership layers. Normal
  managed updates must preserve local additions/edits and must never silently
  overwrite or delete them.
- Managed/local destination conflicts require explicit resolution state (for
  example preserve local plus quarantine, user resolution, or stock fallback),
  with recoverable staging/promotion and rollback after interruption or crash.
- Profile A updates must not mutate Profile B's layout, manifest, recovery state
  or caches, even when immutable content-library sources are shared globally.

## Deferred training

- When a profile has no valid archive, the launcher must let the user choose
  **Train now** or **Start normally**.
- **Start normally** launches stock without deleting a valid prior archive or
  forcing a long archive-generation wait. The launcher should preserve a clear
  pending-training state and offer the choice again later.
- The launcher must accurately say that training can take minutes and that it
  happens after normal game closure; it must not present it as startup time.
- Training/promotion stays opt-in, fail-open and per-profile. A delayed or
  cancelled training attempt must never make the game unlaunchable.

## Offline / non-premium development mode

- The launcher UI must expose an obvious **Add offline account** path, rather
  than requiring a Microsoft-login browser flow or hidden account screens.
- It must clearly label this as a development/local-server mode and let the
  user choose its username and select it per instance. It must not imply that
  offline accounts can join authenticated servers.

## Instance-launch preparation

- Profile launch preparation must be measured as its own boundary: user click
  to Java process creation, separately from Java process creation to usable
  main menu.
- Investigate and remove avoidable subprocesses, repeated metadata reads,
  hashing, filesystem walks, extraction and synchronous network/update checks
  in the instance-launch path. A roughly 20-second fast-PC preparation cost is
  not acceptable when it can scale much worse on an old HDD laptop.
- Reuse fingerprinted, invalidatable launch metadata where it is semantically
  safe; report progress honestly rather than presenting unexplained silent work.
- **High-priority physical finding:** Pandora can spend up to roughly ten
  minutes at “Verifying game assets integrity” on the old HDD laptop. Replace
  unconditional full verification with an incremental, fingerprinted manifest
  that reuses a validated result and invalidates only affected assets. Preserve
  a user-invoked full repair/verification path and fail safely on uncertainty.
