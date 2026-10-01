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
game files. Linking an empty saves directory does not require stopping other
group members. Moving existing worlds still requires stopped members and uses
the existing collision renaming/rollback mechanism.

Instance cards have a 12 px rounded square, inset 8 px at the top-right, colored
deterministically from the group's UUID. Clicking it opens save-group management.
Renaming a group writes only `group.json` atomically: UUID, directory, links,
world data, and indicator color remain stable. Names contain 1–80 characters;
empty names and control characters are rejected.

Validation: focused tests cover ancestor/descendant symmetry, excluding siblings,
missing parents/cycles, metadata-only reads while a layout lock is held, and
renaming without changing identity/storage/world bytes. GUI interactions and
automatic grouping against a live catalog require the local launcher check.
