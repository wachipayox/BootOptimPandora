# Pandora Launcher

Work in progress

## Command line

- `pandora --run-instance "Instance name"` opens the launcher and starts the named instance with full asset verification. This remains the behavior used by existing instance shortcuts.
- `pandora --run-instance-normal "Instance name"` opens the launcher and starts the named instance with the same asset verification mode as the GUI **Start** button. Use this explicit option when automating a normal launcher start.

The two launch options cannot be combined. Without either option, Pandora only opens or focuses its main window.

The private launcher starts from the instance's existing `.minecraft` directory and keeps it writable across runs. Start does not rotate or rebuild `mods/`; the private updater must install pack changes into the instance before launch. The former `original_mods` layout is restored only when migrating an instance left by an older launcher build. Third-party Modrinth/CurseForge modpack expansion is not part of this private Start path.

## Features
- (Optional) sandboxing, to prevent mods from harming your system
- Cross-instance file syncing (options, saves, etc.) (https://youtu.be/wb5EY2VsMKg)
- Mod deduplication when installed through launcher (using reflinks or hard links)
- Secure account credential management using platform keyrings
- Uncapped live game log output
- Content browser providing mods from Modrinth and CurseForge
- Unique approach to modpack management (https://youtu.be/cdRVqd7b2BQ)
- Native application (no Electron/Tauri)
- No third-party metadata servers (no downtime, no delay when MC updates)
- Automatic redaction of sensitive information (i.e. access tokens) in logs

## FAQ

### Discord Server

https://pandora.moulberry.com/discord

### Where can I suggest a feature/report a bug?

Please use GitHub issues.

### Why should I use Pandora over other launchers?

1. If you like one of the features above
2. If you like the general design/ux of the launcher, personally I find it very easy to use
3. The launcher is designed to be performant, from storage space to cpu and memory usage

### Will Pandora be monetized?

Unlikely, for a few reasons:
- I believe that it is wrong for launchers to be monetized without distributing revenue back to mod creators that give the launcher value in the first place. Since I don't have the infrastructure to be able to redistribute revenue to mod creators, this is a big barrier.
- Dealing with monetization takes a lot of (ongoing) work, probably more work than creating the launcher itself.
- I personally dislike advertisements.

## Instance Page
![Instance Page](https://raw.githubusercontent.com/Moulberry/PandoraLauncher/refs/heads/master/screenshots/instance.png)

## Private launcher interaction (2026-10-01)
Repair game files is an instance Settings action. Duplicate and Create derived
instance have separate controls. Derived instances reuse an existing managed
save-group link; when the parent is ungrouped, creation asks whether to create a
shared group. Failure/cancellation restores a group created by that attempt.

GUI Start checks committed parent revision metadata with a two-second network
budget. It does not enumerate/hash mods or configs. Updates are optional; an
unavailable server does not block an installed game. Accepting updates processes
local ancestors before children. Local parent snapshots store an applied parent
generation; old branches require one explicit synchronization to establish it.
External edits without a committed revision are not detected by this version-only
check; explicit inherited-file updates still capture them with the existing scan.
Global-to-global revisions still pin their exact base; changing such a base needs
publishing a new child revision on Distribution.

Global profiles appear in a separate moving carousel (three cards), paused on
hover, dialogs and inactive windows. Detail dialogs offer installation. Profile
artwork is optional signed CAS metadata, bounded to PNG2MiB/1024px, and is stored
outside the game file tree; new installs copy it to the instance icon.
Skin preview textures are decoded once per skin/cape and rendered by a single
background task. Movement targets30FPS at native physical resolution (including DPI scaling). Isolated pixel-equivalence and CPU microbenchmarks passed;
these are not measured GUI FPS or claims about laptop performance.

Native skin preview development builds (2026-10-01): explicitly optimize the
frontend workspace package; the wildcard dependency profile excludes it. At
452x768, an isolated synthetic skin/cape benchmark measured 59.042 ms/frame
unoptimized versus 19.846 ms/frame optimized. Thirteen reference comparisons
were pixel-identical. Pixel conversion and RenderImage creation now run on the
render worker rather than the UI thread. These are CPU renderer measurements,
not end-to-end GUI FPS or laptop results. Native resolution is retained.

Skin transparent-overlay culling (2026-10-01): cached conservative alpha
coverage skips only faces incapable of sampling any visible texel, excluding
capes and opaque base faces. Native 452x768 opt3 CPU renderer, three rounds of
60 frames: empty overlay median 18.215 -> 14.387 ms; opaque 19.898 -> 18.825 ms;
partial alpha 29.432 -> 29.166 ms. Only the empty-overlay improvement is
attributable (~21%); 576 reference frames were pixel-identical. Coverage costs
8,450 bytes per modern skin. Resolution, blend order and cadence stay unchanged.
These are isolated renderer CPU timings, not GUI FPS.



2026-10-01 profile presentation: the carousel now moves at 40 px/s with
frame-paced subpixel motion (previously 20), pauses when hidden/hovered/inactive
or a dialog is open, and consumes
mutable HTTPS catalog name/description/icon overrides without changing installed
game revisions. Signed game metadata remains authoritative for game content.
Distribution 0.2.16 implements Edit profile -> Save changes; presentation icons
are still bounded PNG CAS objects verified by size/hash before decoding.

New instances installed from global profiles inherit the signed profile icon.
Derived local branches inherit their parent's icon by default; the branch dialog
can disable reuse or rotate its hue, writing the variation only to the child.
Creating a branch from an ungrouped parent now opts into creating a shared save
group by default.

Skin scheduling audit (2026-10-01): the widget now requests one GPUI native
next-frame callback at a time and advances at an anchored 30Hz cadence. Completed
frames publish immediately; the animation continues while another window has
keyboard focus, matching Pandora behavior. Two artificial timers were removed.
Two deterministic scheduling tests pass; GUI presentation still needs visual
validation. See docs/research/skin-preview-scheduling-2026-10-01.md.


## Private profile publication

Distribution 0.2.17 signs administrator publications automatically. The launcher
obtains unknown public signing identities from the configured certificate-verified
HTTPS server; manual signing-key setup is unnecessary. Existing pinned identities
and revision/object verification remain compatible. Keep the complete server data
directory backed up; losing the administrator PC needs no signing-key migration.

## Starting installed profile branches

Start (including quick play) checks committed parent versions before launching.
The HTTPS lookup has a two-second budget and never scans the installed modpack.
If an update exists, the dialog offers Update and start or Start without updating.
Unavailable remote metadata does not suppress known local-parent changes.

Withdrawing a global profile from Distribution leaves installed instances and
their pinned game files intact, but stops discovering newer global revisions.
Deleting a local parent likewise leaves each child tree intact. The child keeps
its original parent UUID; it is not silently reparented to its grandparent.
Update discovery stops at the missing ancestor, while surviving descendants
can still synchronize with their own existing local parents. Explicit update
from a missing parent reports that the source is unavailable.

Global profile player defaults: root-level `options.txt` and `servers.dat` are
seeded only when the instance is first created. The launcher records them as
user-owned after seeding, so later global or branch updates preserve player
changes. Other player data such as `saves/`, screenshots, and launcher state
remains excluded.
