# Global profile families and shared worlds

The launcher carousel presents one card per published root, using the root's
current catalog name, icon and description. An inherited profile is placed under
the parent in its selected stable revision (or latest revision if no stable
channel exists). Root cards with descendants open a scrollable, expandable tree.
Selecting a row opens the installation details; hovering its icon/name displays
the full description. Missing published parents promote their children to visible
roots. A cycle in selected catalog ancestry has one deterministic visible root;
tree traversal cannot recurse indefinitely.

New installations check for existing **direct global-profile instances** whose
profiles are ancestors or descendants of the selected profile. The relationship
works in either creation order and across multiple generations. Equal profile
IDs and sibling profiles alone do not trigger automatic grouping. Local derived
branches continue to inherit their local parent's group through branch creation.

If there is one existing related save group, installation joins it automatically.
If related instances are ungrouped, the launcher creates a group with one related
instance and joins the new instance. If several distinct existing groups are
available, the installation dialog requires an explicit choice. Independent
existing groups are never merged as a side effect. The selected relationship is
checked again after installation so removed/replaced instances cannot be silently
used as targets. Group failures leave the installed modpack usable and report
that Manage save group can retry the operation.

Reading instance ancestry consults only committed manifest/identity metadata,
validates their identity/schema, and neither takes the layout lock nor changes
game files. Changing group routes requires stopped affected instances. Moving
existing worlds uses the existing collision renaming/rollback mechanism.

Instance cards have a 12 px rounded square, inset 8 px at the top-right, colored
deterministically from the group's UUID. Clicking it opens save-group management.
Renaming a group writes only `group.json` atomically: UUID, directory, links,
world data, and indicator color remain stable. Names contain 1–80 characters;
empty names and control characters are rejected.

Validation: focused tests cover ancestor/descendant symmetry, excluding siblings,
missing parents/cycles, metadata-only reads while a layout lock is held, and
renaming without changing identity/storage/world bytes. GUI interactions and
automatic grouping against a live catalog require the local launcher check.

## Shared data and group settings

Global synchronization defaults to ten folders: `screenshots`,
`Distant_Horizons_server_data`, `.voxy`, `journeymap`, `flashback`, `xaero`,
`.bobby`, `.analogaudio`, `schematics`, and `.sable`. There are no predefined
global files. Existing configurations receive the new selection once; custom
routes remain and later user changes are preserved. Removed predefined routes
are materialized locally when the instance next prepares sharing. Startup only
migrates configuration metadata, without walking instance contents.

Instance settings also expose **Gestionar global**. Its master switch defaults
to off. With overrides on, each catalog route chooses NO, G(SÍ)/G(NO), or SÍ;
G follows the current global value and uses the warning/yellow treatment. Turning
the master off preserves dormant choices and resumes global defaults. The dialog
cannot introduce a route that is not offered globally. Existing blanket opt-outs
are represented as explicit NO choices and are normalized when saved.

Each group's settings independently select files and folders, including custom
relative paths. Defaults are `saves`, `config/litematica`, `Sable-Schematics`,
`music_sheets`, `command_history.txt`, and `hotbar.nbt`. Membership is stored in
instance configuration, independently of whether `saves` is enabled. Existing
legacy saves links are adopted once. Empty groups retain their storage when the
last member leaves, so shared data and unselected group routes are not deleted.
Deleting the last instance does not collect either shared store. Per-instance
recovery copies are different: they live inside the instance directory and are
removed if that entire instance is deleted.

Group routes take precedence over overlapping global routes. A folder containing
any enforced inherited path is excluded from sharing in that instance; shared
storage remains unchanged. Default-once/personal paths can remain shared.
Explicit local overrides and tombstones retain the existing profile semantics.
Overwrites displays the owner and inherited conflict, and rejects edits that
would modify another instance's shared data.

Folders use owned directory links. Files are copied before launch and reconciled
after the game exits; a local edit publishes only while its previous shared seed
is unchanged. Divergent copies keep the shared version and save the instance's
version under `<instance>/.pandora-shared-v1/recovery`. Replaced shared file
versions are retained there too. Conflicting world directories are renamed as
whole worlds, never recursively merged. Unknown links are rejected, not followed.

A launcher-wide storage lock and launch/change guards serialize mutations.
Affected running instances prevent route changes; preparing an empty link does
not require reading an entire modpack. Ownership receipts live outside
`.minecraft` and ordinary duplication does not copy them.

## Inherited update flow

Checking for inherited changes retains the current instance page, including a
check started from instance settings. Only a confirmed update dispatched for
application uses Home's update surface; no-op and cancelled checks stay put.

Starting an instance now shows a short update check before the game launch
pipeline. The check compares the committed generation of every local ancestor,
then a metadata-only fingerprint of each local parent's `.minecraft` tree,
and finally the selected revision of the global ancestor. It reads file names,
sizes, and modification times without hashing file contents. `saves`, `logs`,
`crash-reports`, `screenshots`, `session.lock`, and `usercache.json` are excluded from the fingerprint. This
detects normal edits made outside Pandora while keeping HDD reads bounded;
same-size edits with preserved timestamps remain a known limitation.

When an update is available, Enter chooses **Update and start**. The other
option starts the installed state unchanged. Both this action and **Update
inherited files** in instance settings use the same progress window and update
ancestors from oldest to youngest. The instance switches to Quickplay and its
other tabs are disabled during the operation. On success the progress window
closes and the tabs unlock; errors keep the message visible and also unlock
the tabs. Global asset downloads show an aggregate byte bar and a bar for each
downloaded file. A removed local or global ancestor does not invalidate an already
installed descendant.

Command-line starts (`--run-instance*`) use the same check without a dialog:
a pending update exits with code 3 unless `--update` or `--no-update` is
given. See the README "Command line" section for the output contract.
