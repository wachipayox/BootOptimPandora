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
