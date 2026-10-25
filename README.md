# Pandora Launcher

Work in progress

## Command line

- `pandora --run-instance "Instance name"` opens the launcher and starts the named instance with full asset verification. This remains the behavior used by existing instance shortcuts.
- `pandora --run-instance-normal "Instance name"` opens the launcher and starts the named instance with the same asset verification mode as the GUI **Start** button. Use this explicit option when automating a normal launcher start.

The two launch options cannot be combined. Without either option, Pandora only opens or focuses its main window.

Command-line starts run the same inherited update check as the GUI **Start** button, but never start silently on stale files. Results are printed to stdout and stderr (and `launcher.log`) with the prefix `[WBOMBO-PROFILE]`; when another launcher process is already running, the forwarding process prints them and returns the code.

- No pending update: `UP-TO-DATE instance=<name> profile=<id> rev <id>` (or `rev local`), then the game starts. Exit code 0.
- Pending update without a flag: one `UPDATE-AVAILABLE instance=<name> [via=<ancestor>] current=<profile> rev <id> available=<profile> rev <id>` line per stale ancestor (local parents print `current=<parent> gen <n> available=<parent> gen <m> kind=local-parent` or `kind=local-parent-files`), then `Relaunch with --update (update then start) or --no-update (start unchanged).` Nothing starts and the launcher exits with code **3**.
- `--update`: applies the same update as **Update and start**, then starts (`UPDATED`). If the update fails, `UPDATE-FAILED reason=...` and exit code **4**.
- `--no-update`: prints `UPDATE-SKIPPED ...` lines and starts the installed state unchanged.
- Check failure (server unreachable, timeout): `UPDATE-CHECK-FAILED reason=...` and the game starts unchanged, like the GUI. Known local parent changes still gate.
- Unknown instance name: `INSTANCE-NOT-FOUND`, exit code **5**.

`--update` and `--no-update` require `--run-instance` or `--run-instance-normal` and exclude each other. Automated measurements should pass `--no-update` so the pack under test does not change between runs.

### Instance management

Agents and scripts can manage instances without the GUI. These commands never open a window and never start the game. They use the same backend paths as the carousel (signed pin, lineage, identity lock and save groups included), never writing instance files by hand. They work with the launcher open (the request is forwarded through `launcher.sock` and the answer comes back to the calling process) and closed (the process starts the launcher headless, runs the command and exits). Results are lines with the prefix `[WBOMBO-INSTANCE]` on stdout and stderr (and `launcher.log`); `--json` prints only JSON, on stdout. Only one command can be used per call, and none can be combined with `--run-instance*`.

- `pandora --list-instances [--json]`: one `INSTANCE name=… profile=… revision=… sequence=… loader=… minecraft=… created_by_cli=… minecraft_dir=…` line per instance, then `LISTED count=<n>`. With `--json`, an array of objects with the fields `name`, `profile`, `revision`, `manifest_sha256`, `loader`, `minecraft`, `minecraft_dir`, `created_by_cli`. The instance stores the revision id and digest, not the sequence number: `sequence` is `unknown` in the list. While an instance is running its profile state cannot be read, so `profile` and `revision` are `none`/`null` for it. A launcher started by one of these commands logs to `launcher.log` only, so stdout carries just the result lines or the JSON.
- `pandora --instance-info "Name" [--json]`: the same for one instance, plus `sequence` when the server answers.
- `pandora --create-instance "Name" --from-profile <profile_id> [--revision latest|rev_<id>]`: creates an instance of a global profile, as the carousel does, and marks it as created by the command line (`created_by_cli` in `info_v1.json`; older launchers ignore the field). Without `--revision` it uses what the GUI uses: the profile's `stable` revision, or its latest revision if it has none. `--from-profile` accepts a test profile (`profile_test-…`) whatever the launcher channel is. Prints `CREATING …` and, at the end, `CREATED instance=<name> profile=<id> rev <id>` (the lines arrive when the command finishes; the launcher shows live progress only in the GUI). If the name is taken: `EXISTS instance=<name>`, exit code **6** (the GUI would rename it to `Name (1)`, the command line never does).
- `pandora --update-instance "Name" [--revision latest|rev_<id>]`: updates without starting. Without `--revision`, the same rules as **Update and start** (every stale inherited ancestor, then the instance); with it, the instance's direct global profile is moved to that exact signed revision. Prints `UPDATED instance=<name> profile=<id> from rev <old> to rev <new>` or `UP-TO-DATE …`.
- `pandora --delete-instance "Name" --confirm "Name"`: deletes an instance, but **only** one created by `--create-instance` or one of a `profile_test-…` profile. Any other instance (the player's own) is `REFUSED instance=<name> reason=…` with exit code **7** and nothing changes. `--confirm` must repeat the exact name. Prints `DELETED instance=<name>`.

Exit codes of all command-line requests: **0** ok, **3** update pending (start only), **4** the operation failed or the instance is busy (`UPDATE-FAILED`, `CREATE-FAILED`, `DELETE-FAILED`, `FAILED reason=…`), **5** no instance with that name (`INSTANCE-NOT-FOUND`), **6** the name already exists, **7** refused by the safety rules (`REFUSED`; also an instance name that is not a single valid folder name).

### Launcher channel

The launcher configuration has a `channel` option with two values: `player` (the default) and `developer`. Change it in Settings → Global profiles → Launcher channel, or by editing the `channel` field of `config.json` in the launcher folder, with no password. In `developer` the carousel asks the server for `GET /v1/profiles?include_test=1` and marks the test profiles (`profile_test-…`) with a **PRUEBA** label; in `player` they never appear. A server older than Distribution 0.3.0 ignores the parameter. It has nothing to do with the revision channels of a profile (`stable`), which do not change.

The private launcher starts from the instance's existing `.minecraft` directory and keeps it writable across runs. Start does not rotate or rebuild `mods/`; the private updater must install pack changes into the instance before launch. The former `original_mods` layout is restored only when migrating an instance left by an older launcher build. Third-party Modrinth/CurseForge modpack expansion is not part of this private Start path.

## Signed launcher updates

The launcher reads `/v1/launcher/pandora/releases` from its configured Distribution service, using the current platform, Player/Developer channel and installed SemVer. The Distribution service version is independent of the launcher version. The update icon beside the sidebar logo opens the version dialog; **Buscar actualización** in About also checks explicitly. **Más tarde** saves the skipped version locally and suppresses its automatic dialog; the icon and manual check remain available.

Before offering an update, Pandora verifies Ed25519 over the exact decoded `artifact.statement` bytes, using the same HTTPS signing-key discovery and configured pins as profiles. It checks the statement type, application, platform, version, SHA-256 and size, and requires strictly greater SemVer precedence (build metadata does not make a version newer). Before installation it rechecks the current channel/source and release, then verifies the bounded download's SHA-256 and length. Redirects and external artifact origins are not accepted. See Distribution's [launcher contract](https://github.com/wachipayox/BootOptimDistribution/blob/agent/integration-current/docs/PROFILE_PROTOCOL.md).

Windows installs an EXE; Linux accepts AppImage/ELF; macOS uses `.app.tar.gz`. A standalone macOS installation extracts only the bundle executable; the same rule protects a portable bundle containing launcher data. Archive paths/links and expansion are bounded. The current installation path is kept and a uniquely named `.wbombo-previous-…` sibling is retained for recovery. Existing Linux/macOS authorization is preserved when the installation needs elevated permissions. Games, AOT, authentication and pending file operations must be idle; replacement uses the existing window/IPC shutdown coordinator before restarting. Launcher configuration, accounts and instances are preserved.

Legacy GitHub update manifests produced by packaging scripts are not read by this client. This implementation does not publish Distribution releases.

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

## Java for Wachiland profiles

Instances of global profiles `profile_wachiland-*` on PC start with Java 25 (Mojang's `java-runtime-epsilon`, hash-verified). Other instances keep Java 21. Setting a custom Java binary on an instance, or `FORCE_EXTERNAL_JAVA`, always wins; enabling a Java 21 binary there forces Java 21. If a Java 25 start exits with an error within 15 seconds, Pandora logs `[WBOMBO-COMPAT] FALLBACK ... mechanism=java25-default` and uses Java 21 for that instance; remove `java25_fallback` from its `info_v1.json` (with the launcher closed) to try Java 25 again.

## AOT cache for Wachiland profiles

On Java 25, Pandora builds a JDK AOT cache (about 300 MB) per instance in two steps: the first game records a training profile when it closes, then the launcher turns it into the cache ("Optimizing launch", cancellable and resumed later). Later starts use it; it is dropped and rebuilt when the JDK build, the profile revision or the mod list changes, the instance settings have an "Optimize launch" option, and any problem falls back to a plain start (`[WBOMBO-COMPAT] ... mechanism=jdk25-aot-cache`). Details: [docs/features/jdk25-aot-cache.md](docs/features/jdk25-aot-cache.md).

## Optional zipfs read-only agent (Windows PC)

When a Wachiland profile starts on Java 25 on Windows, Pandora adds `-javaagent:<launcher>/agents/zipfs-readonly-agent.jar`. The JAR is embedded in the launcher (`crates/backend/src/launch/zipfs-agent/`, with its source commit and SHA-256) and written to `agents/` when that file is missing or different, so it always matches the launcher version. The agent makes the JDK open the mod and library JARs read-only, which avoids a per-open volume lookup while SecureJarHandler loads them. The game's stderr is not kept on a normal start, so the agent writes its own `ACTIVE` and `FALLBACK` lines to `<instance>/.minecraft/logs/wbombo-zipfs-agent.log`. Pandora logs its decision to `launcher.log` as `[WBOMBO-COMPAT] ACTIVE|SKIPPED|FALLBACK ... mechanism=zipfs-readonly-agent`.

Disable it for a session with `WBOMBO_ZIPFS_AGENT=0`. If the JAR cannot be written, Pandora logs `FALLBACK` and starts without it. If the Java 25 start fails early, the Java 25 fallback above also removes the agent, because the Java 21 path does not add it.

## G1 heap sizing of Microsoft OpenJDK 25 (Windows PC)

Mojang's Java 25 runtime is a Microsoft build whose time-based G1 heap sizing can kill the JVM 13 to 23 minutes into a game (`g1HeapRegionManager.cpp:55`, on the `WatcherThread`). When a Wachiland profile starts on Java 25, Pandora adds `-XX:+UnlockDiagnosticVMOptions -XX:-G1UseTimeBasedHeapSizing`, but only if that JVM lists the flag (other JVMs would refuse to start); the answer is cached per JDK build in `jvm-flag-probe.json`. A player or profile setting of the same flag wins. `launcher.log` has the `[WBOMBO-COMPAT] ACTIVE|SKIPPED|FALLBACK ... mechanism=g1-time-based-heap-sizing-off` line. Evidence and sources: [docs/research/g1-time-based-heap-sizing-microsoft-jdk25-2026-10-22.md](docs/research/g1-time-based-heap-sizing-microsoft-jdk25-2026-10-22.md).

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
Global profiles are grouped by ancestry in the carousel. Root cards open an
expandable family tree with description previews. New direct global-profile
instances share worlds with installed ancestors/descendants automatically; when
several existing groups are available, the user chooses one before installation.
The group marker at the top-right of an instance card opens group management,
which supports renaming without moving worlds. See
[profile families and save groups](docs/features/profile-families-save-groups.md).

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

Private builds can set `WBOMBO_DISTRIBUTION_DEFAULT_URL` in the build process
before running Cargo. The compiler embeds this optional default; public builds
without it start with no Distribution endpoint. Existing saved endpoints take
precedence. Keep the value in private build configuration, never in source,
workflow files or logs. The normal build and release workflows read the repository
secret of the same name; builds without secret access leave the default empty. This variable is an endpoint, not an
API token. The configuration E2E example reads `WBOMBO_DIST_URL` at runtime.

`WBOMBO_COMMUNITY_DISCORD_URL` optionally supplies the private community link in
About. The build workflows read the matching repository secret. When absent or
not an HTTPS URL, that action is omitted. Keep invite values out of public source.

UI capture instructions are in the [UI workflow](.claude/skills/ui/SKILL.md).
Investigation decisions and evidence are indexed in [research records](docs/research/README.md).
