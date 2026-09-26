# Persistent profile layout — implementation status

Last checked: 2026-09-26 against GitHub `agent/integration-current`.

## Integrated behavior

Pandora PR #66 (`a4aae071d6062adae9bcffe592ca81c541650a82`) is merged into
`agent/integration-current`. It keeps each private instance's existing
`.minecraft` directory live and writable across runs. Start no longer loads
Mods metadata, scans/rebuilds `mods/`, expands third-party modpacks, rotates the
folder to `original_mods`, or recopies extras. A one-time legacy restoration
remains for instances left in the former `mods` / `original_mods` shape.

This protects runtime data written under `.minecraft`, including MCEF's
`mods/mcef-libraries` and `mods/mcef-cache`, and Analog Audio's
`.analogaudio/internal` library cache. PR #66 leaves explicit cross-instance
syncing as an existing launcher feature; the private updater is responsible for
applying private-pack changes before Start.

PR #64 is also integrated and enables the existing launch-fast library policy
and asset USN cache by default. It is independent of profile ancestry/update
reconciliation.

## Not integrated: draft architecture and implementation chain

The following PRs are still open drafts and do not form the production
integration tree as of this check:

- #32 ownership/recovery architecture;
- #33 ownership reconciliation candidate;
- #34 persistent layout backend state;
- #35 signed private distribution service design;
- #36 recoverable persistent-profile client flow;
- #37 cloned-profile identity and per-profile lock;
- #38/#39 native lock validation.

Their branch ancestry is based on older candidate heads. Do not describe them
as integrated, and do not merge the stale chain as-is. Rebase/recompose the
needed ownership, transaction, identity, and recovery work on the current
integration branch, then review and validate it as one coherent foundation.

The design documents `PERSISTENT_PROFILE_LAYOUT_ARCHITECTURE.md`,
`PERSISTENT_PROFILE_LAYOUT_CLIENT_FLOW.md`, and
`PERSISTENT_PROFILE_LAYOUT_IDENTITY_LOCK.md` describe intended behavior and
candidate APIs. They are not evidence that this reconciler is present in the
integrated source. The actual source tree and merged PRs remain authoritative.

## Product boundary

The persistent `.minecraft` behavior already merged in #66 is the correct base:
do not reintroduce per-Start copying or generic Modrinth/CurseForge pack
expansion. The missing layer is a safe, transactional updater that tracks
managed ownership and reconciles global/local profile revisions outside Start.

Future reconciliation must preserve local changes by default, retain forced
conflicts recoverably, isolate state per profile, and avoid full-tree scans for
Start or a no-op update. A separate explicit **Repair modpack** action may run
full parity for global or globally derived profiles. Keep it separate from
**Repair game files**, which belongs in instance maintenance settings.

## Historical laptop smoke boundary (2026-09-20)

An earlier layout-only portable executable validated launching an existing
instance with a persistent game directory; it did not validate global profiles,
ownership reconciliation, or a full update flow. The run was not a performance
A/B. Its log recorded 98 s to scan/display mod content, 58 s before launch
setup, then 229 s in Java/assets/libraries/log-configuration preparation
before the game process. These fields belong to different launch boundaries and
must not be combined into a single startup comparison.

Any new performance claim must use matching Start-to-Java origin/endpoint
markers and the current measurement rules. Runtime profile-update validation
must separately cover menu and representative in-world behavior.
