# Instance Overwrites tab

The Overwrites tab compares an instance's effective `.minecraft` files with
each available parent scope:

- A local branch is compared with its direct local parent's current files.
- An instance with global ancestry is compared with the signed effective file
  set of its pinned global revision, including that revision's global parents.

The tab presents the instance's modpack as a collapsible folder tree with
connector lines, file and folder icons, search, and an optional differences
filter. Common modpack folders appear first. Changed files have a short status
badge. Selecting one shows its exact status relative to each
available parent: added, modified, removed, enabled or disabled. A content
change combined with a toggle is shown as both. The `.disabled` suffix for
toggleable mods is treated as local mod state rather than a second mod identity.
Deleted inherited files remain visible in the tree and are labelled as removed.

Right-clicking a file or folder offers to ignore its path. The ignore list is
global to the launcher, uses paths relative to `.minecraft`, and matching a
folder excludes all descendants. The same list filters local-parent snapshots,
the fast parent fingerprint, signed global update comparisons, and this tab.
The Settings > Ignored paths page permits manual additions and removal. The
defaults include `mods/.connector`, `.analogaudio`, `.bootoptim`,
`mods/mcef-cache`, `.mixin.out`, and `unilog`. User data directories
such as `saves` remain separately protected by the runtime exclusion policy.
Ignoring a path does not delete or change its current files. An old child
fingerprint recorded before changing the ignore list can produce one update
notice; the next inherited update records the new filtered fingerprint.

For a changed or removed file that exists in a parent, the context menu offers
restoration from the direct local parent and/or the pinned global revision.
The action asks for confirmation, requires the game to be stopped, and changes
only the selected child. A local parent is read at its current version; a global
source is fetched as a verified object from the pinned revision. The new file
is staged before the old file is moved aside and restored on a failed install.
The parent remains unchanged. A missing parent file does not offer restoration.

Supported UTF-8 text files (`.toml`, `.properties`, `.txt`, `.cfg`, `.ini`,
`.json`, `.mcmeta`, `.yaml`, `.yml`) up to 1 MiB can be viewed and edited
directly. Saving compares the current file hash with the hash read by the
editor and rejects concurrent changes. The original is renamed to a temporary
backup until the replacement is in place; a failed replacement restores it.
Mod JARs can be enabled or disabled from this tab without changing their bytes.
These operations affect only the selected local instance, and require the game
to be stopped. They acquire the inherited-profile lock when that profile has
one, so an update cannot modify the same files concurrently. The tab does not
offer binary editing, deletion, or creation of new files.

The comparison runs when the tab is first opened, explicitly refreshed, or after a file edit.
It hashes files to confirm changes and may take time on an HDD for a large
pack. Global comparison requests signed revision metadata but does not
download modpack objects. If the distribution service is unavailable, the
local-parent section remains available and the global section shows the error.

## Text comparison

Selecting TOML, properties, TXT, CFG, INI, JSON, MCMETA, YAML or YML opens an
aligned comparison: the instance on the left, the selected ancestor on the
right. The right header selects the mother, grandmother, or another ancestor
up to eight levels, following local parents and then the pinned global revision
chain. Local ancestors show their current files; global ancestors show their
installed revision, including its effective enforced and initial config rules.
Only the selected ancestor's content object is downloaded, with signature/hash
verification. Missing files are represented by an empty column.

Line numbers and green/red backgrounds distinguish local and ancestor changes.
Both columns scroll together vertically. Their draggable separator defaults to
equal widths based on the viewport, independently of line length. Each column
has its own horizontal scrolling for long lines, and headers resize with their
column. The chosen split is retained while the comparison view is open. Diff work
An extra empty row at the bottom prevents horizontal overlay scrollbars from
covering the last real line. A two-lane overview beside the vertical scrollbar
marks local additions/changes in green and ancestor removals/changes in red;
clicking a marker centers its first changed row in both columns. Contiguous
changed blocks are grouped off the UI thread when the comparison is built.
runs off the UI thread, uses bounded LCS with a bounded-window fallback for large
files, and virtualizes rows. Text previews remain limited to 1 MiB per file.
The per-page ancestor cache holds at most 32 path/level results and is cleared by
explicit refresh, save, or restoration. Select **Editar archivo** to use the
existing guarded editor; save or discard changes before returning to comparison.
If the server is unavailable, available local comparisons remain usable.

The tab keeps its report while navigating among pages of the same instance.
Adding an ignored path removes matching rows from that report immediately.
On return, a cheap config read checks whether the ignore policy changed; added
paths filter the cached report, whereas removed paths require a new scan.
Explicit refresh and file edits also rescan. File hashes are reused across
scans only while the file size and modification timestamp remain unchanged;
the cache is bounded to 20,000 paths and a file changing during a hash is
rejected for a later refresh.

The default list includes `mods/mcef-libraries`, `.sable`, and `.voxy` as
well as the initial cache paths. The persistent "Otras" checkbox controls an internal `*` rule that ignores other top-level
folders and files outside the conventional modpack roots (`mods`, `config`,
`defaultconfigs`, `resourcepacks`, `shaderpacks`, `datapacks`, `kubejs`,
`scripts`, `openloader`, `global_packs`,
`patchouli_books`, and shader option files). Existing installs gain
these defaults once through a versioned config migration; subsequent user
removals are preserved. `fancymenu_data`, `options.txt`, `optionsviveprofiles.txt`, and `servers.dat`
are part of "Otras"; disabling its checkbox restores their visibility unless
another explicit rule excludes them. The settings list grows with the window and scrolls
when necessary.

Worlds, logs, crash reports, screenshots, known player caches, session/cache state, game libraries,
and other launcher runtime directories are excluded. Symbolic links, junctions
and unsupported entries are never followed and are reported as skipped. If a
local parent has been deleted, its section explains that the comparison cannot
be made; the installed child itself remains usable.
