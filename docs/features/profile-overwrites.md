# Instance Overwrites tab

The Overwrites tab compares an instance's effective `.minecraft` files with
each available parent scope:

- A local branch is compared with its direct local parent's current files.
- An instance with global ancestry is compared with the signed effective file
  set of its pinned global revision, including that revision's global parents.

The tab presents the instance's modpack as a collapsible folder tree. Changed
files have a marker. Selecting one shows its exact status relative to each
available parent: added, modified, removed, enabled or disabled. A content
change combined with a toggle is shown as both. The `.disabled` suffix for
toggleable mods is treated as local mod state rather than a second mod identity.

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

The comparison runs only when the tab is opened or refreshed, and after an edit.
It hashes files to confirm changes and may take time on an HDD for a large
pack. Global comparison requests signed revision metadata but does not
download modpack objects. If the distribution service is unavailable, the
local-parent section remains available and the global section shows the error.

Worlds, logs, crash reports, screenshots, session/cache state, game libraries,
and other launcher runtime directories are excluded. Symbolic links, junctions
and unsupported entries are never followed and are reported as skipped. If a
local parent has been deleted, its section explains that the comparison cannot
be made; the installed child itself remains usable.
