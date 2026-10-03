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
The Settings > Ignored paths page permits manual additions and removal. New
installations start with `mods/.connector`, `.analogaudio`, `.bootoptim`,
`mods/mcef-cache`, `.mixin.out`, and `unilog`. The last three are generated
library/cache or diagnostic data observed in this pack. Existing installations
receive these defaults when the config field is absent. User data directories
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

The comparison runs only when the tab is opened or refreshed, and after an edit.
It hashes files to confirm changes and may take time on an HDD for a large
pack. Global comparison requests signed revision metadata but does not
download modpack objects. If the distribution service is unavailable, the
local-parent section remains available and the global section shows the error.

Worlds, logs, crash reports, screenshots, known player caches, session/cache state, game libraries,
and other launcher runtime directories are excluded. Symbolic links, junctions
and unsupported entries are never followed and are reported as skipped. If a
local parent has been deleted, its section explains that the comparison cannot
be made; the installed child itself remains usable.
