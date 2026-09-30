# Persistent profile layout — implementation status

Last checked: 2026-09-30 against `agent/integration-current`
(`98c75be5b6448ac8ce01819855a39e7de91fcb93`).

## Integrated behavior

Pandora PR #66 is merged into
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

Pandora PRs #67/#68 are also integrated. They add the persistent branch lineage,
ownership/recovery transaction, revision delta application, and explicit
Repair modpack backend. These are the current source of truth; the older PR
chain below is historical input, not outstanding work to merge.

Pandora PRs #69–#72 are integrated. Together they add verified catalog and
revision reads, signed-object downloads, initial global-instance creation,
incremental updates for direct global instances, the Wachiland Launcher
name/data path, removal of unused sandbox/file-sync settings, and a connection
probe that can run before local signing-key setup.

Pandora PR #76 is also merged. It connects schema-v2 TOML, `.properties`, and
`.txt` setting rules to revision resolution and the persistent install/update
transaction, including recoverable conflicts and first-install markers. The
live Distribution service reports 0.2.14 at commit
`d98d1944272e1d47a4fe06e0808defbc22d52486`. The backend E2E harness passed
against that service using signed synthetic root and child revisions: six
inherited files resolved; the three formats merged on first install; inherited
mandatory rules replaced child rules; and `default_once` preserved local edits
on a later merge.

This E2E validates the signed Distribution API, signature verification,
revision resolution, and backend merge transaction. It does not yet validate
the complete graphical launcher flow, a normal global-instance update through
the UI, or the Repair modpack UI. Local-child creation, local overlay editing,
ancestry UI, update history/progress, and Repair modpack UI remain future
client work.

The earlier `codex/toml-config-rules` branch is stale; the implementation was
continued and integrated through PR #76. On Windows, file payloads and
journals are flushed, but directory-entry flush is not available through the
standard library; abrupt power-loss durability is therefore weaker than on
Unix. See the branch cleanup/status record for its disposition.

## Historical implementation chain

The following older PRs were candidate drafts before the work was recomposed
and integrated through #67/#68. They are superseded historical input, not
pending work to merge:

- #32 ownership/recovery architecture;
- #33 ownership reconciliation candidate;
- #34 persistent layout backend state;
- #35 signed private distribution service design;
- #36 recoverable persistent-profile client flow;
- #37 cloned-profile identity and per-profile lock;
- #38/#39 native lock validation. The old candidate chain exposed a Windows
  directory-handle durability failure and later focused native lock probes.
  Those results are historical evidence for the old implementation, not a
  reason to reuse its branches; the current integrated implementation and its
  own checks are authoritative.

Their old branch ancestry is stale. Do not merge that chain over the current
implementation or report it as a second pending foundation.

The design documents `PERSISTENT_PROFILE_LAYOUT_ARCHITECTURE.md`,
`PERSISTENT_PROFILE_LAYOUT_CLIENT_FLOW.md`, and
`PERSISTENT_PROFILE_LAYOUT_IDENTITY_LOCK.md` remain useful architecture
references. The actual source tree and merged PRs remain authoritative.

## Product boundary

The persistent `.minecraft` behavior already merged in #66 is the correct base:
do not reintroduce per-Start copying or generic Modrinth/CurseForge pack
expansion. The missing layer is a safe, transactional updater that tracks
managed ownership and reconciles global/local profile revisions outside Start.

The integrated reconciler preserves local changes by default, retains enforced
conflicts recoverably, isolates state per profile, and avoids full-tree scans
for Start or a no-op revision delta. A separate explicit **Repair modpack**
backend action can run managed-file parity for global or globally derived
profiles. Keep it separate from **Repair game files**, which belongs in
instance maintenance settings. The new client must preserve these boundaries.

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
