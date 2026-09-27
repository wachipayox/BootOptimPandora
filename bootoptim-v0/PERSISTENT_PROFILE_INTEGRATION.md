# Persistent profile layout — implementation status

Last checked: 2026-09-28 against `agent/integration-current` (`f81be6f`).

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

Pandora PRs #67/#68 integrate persistent branch lineage, ownership/recovery,
revision-delta application, and the explicit Repair modpack backend. PR #69
adds HTTPS catalog access, signed revision and SHA-256 object verification,
content-addressed downloads, and creation of a local instance pinned to a
verified global revision. PR #70 adds incremental updates for direct global
instances. PR #71 changes the product-facing name and data directory to
Wachiland Launcher, removes the sandbox and file-sync controls, and leaves
existing PandoraLauncher data untouched. PR #72 adds an HTTPS/protocol probe
that can run before signing-key setup.

The current integration builds and opens locally on Windows. This confirms the
launcher starts with the WachilandLauncher data directory, but does not validate
global profile publication, install, or update against the user's service.
Local child creation, local overlay editing, full ancestry UI, and Repair
modpack UI remain unfinished.

## Structured config rules (schema v2, work in progress)

Branch `codex/toml-config-rules` starts from current
`agent/integration-current` (`f81be6f`). It adds strict reading and
parent-to-child resolution for signed `config_settings` rules, plus
format-aware merge helpers in `crates/backend/src/config_settings.rs`. TOML
uses dotted keys, `.properties` uses simple property keys, and `.txt` uses
one-based physical line identities. A descendant replaces the same path/key
rule from its parent, and a grandchild may replace the descendant's value
again. The parent permission bit gates each override. `enforced` changes only
the selected setting and reports it for conflict retention; `default_once`
markers are stored per setting in local branch metadata. Unselected content
and comments stay local.

This is not yet connected to the launcher install/update transaction or
recoverable conflict-copy path. Do not describe schema-v2 rules as usable in a
release until those are wired and a synthetic three-level profile is
published/resolved/applied. Initial value support is TOML scalars/flat lists
and string-valued `.properties`/`.txt` rules. `.txt` line numbers shift when
lines above are inserted or removed; TOML tables/objects, `null`, escaped
properties keys, Java properties continuations, non-UTF-8 properties files,
and multiline text values are unsupported.

## Historical implementation chain

The following older PRs were candidate drafts before the work was recomposed
and integrated through #67/#68:

- #32 ownership/recovery architecture;
- #33 ownership reconciliation candidate;
- #34 persistent layout backend state;
- #35 signed private distribution service design;
- #36 recoverable persistent-profile client flow;
- #37 cloned-profile identity and per-profile lock;
- #38/#39 native lock validation.

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
