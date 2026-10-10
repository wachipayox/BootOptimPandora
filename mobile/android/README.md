# Wachiland Launcher for Android — 0.1.11 alpha

First mobile prototype, based on a pinned FCL / HMCL / Amethyst Android engine.
This is a companion client for the same Distribution server. It is not an Android
compilation of GPUI or the desktop Rust binary.

## Included

- Landscape orientation across launcher, game, dialogs and crash pages.
- Dark Wachiland home, touch sized profile and instance cards, original profile
  icons, native account/settings/mod management and game controls.
- Microsoft browser authentication using the desktop launcher's public client ID.
- Distribution catalog, immutable signed revisions, up to eight ancestors,
  content-addressed downloads with SHA-256 verification, fresh configuration rules.
- Minecraft / NeoForge installation through the engine, installation progress and
  normal native game launch. Each installed pack uses an isolated game directory.
- Incomplete installations cannot start; launcher folders use app scoped storage.
- Recovery-time Android process exit history, last game log snapshot and optional
  native/ANR traces, with a touch accessible local diagnostic sharing button.
- Separate app ID `net.wachiland.launcher`, persistent private signing certificate,
  no original engine automatic updater or prelaunch advertisements.

## Prototype boundaries

Android 8.0/API 26 or newer, ARM64 only. The build includes Java runtimes and native
rendering libraries, not Minecraft or paid game assets. A licensed Minecraft Java
account is required for Microsoft launch.

Local derived branches, shared save groups, parent update reconciliation, the
desktop diff editor and its global ignored-path settings are not ported yet. The
catalog currently presents individual profiles rather than the desktop family
tree. Configuration rules support ordinary TOML tables/keys, properties and
physical `text_lines`; unsupported TOML constructs fail explicitly. This prototype
only installs new instances and never upgrades existing trees.

Desktop mods with native libraries or desktop graphics assumptions may require
Android variants or removal. The APK building successfully does not establish
that the user's complete NeoForge pack runs on their device.

## Source and licensing

Engine: https://github.com/FCL-Team/FoldCraftLauncher

Pinned upstream commit: `5e76d7485d6ca34fe2adf86b31156f2714f5ccd9`.

Desktop integration source at start: `8c9d18ca9021103f4735b1a36f57d781091c7755`.

`wachiland.patch` contains all mobile source changes, including new source files.
The mobile code is GPL-3.0-or-later as required by the engine; see `COPYING` and
preserved engine/dependency notices. This does not change the desktop repository's
license. Supply the corresponding modified source with the APK to recipients.
The delivered source archive contains the complete modified engine and this
reproduction bundle. Signing credentials are excluded.

## Rebuild on Windows

Install Git, JDK 21, Android SDK platform 35, NDK `27.0.12077973` and CMake `3.22.1`.
Gradle 8.14.4 is downloaded by the upstream wrapper; dependency downloads require
network access. Then run:

```powershell
.\build.ps1 -AndroidSdk 'C:\path\to\Android\Sdk' -JavaHome 'C:\path\to\jdk-21'
```

The script prepares a pinned engine checkout, applies the patch, creates a signing
key automatically in `%LOCALAPPDATA%\WachilandBuild\private-signing`, and emits an
APK plus checksum in `out`. Back up the builder's key and password for future
compatible updates. Users installing the APK do not handle these files. A newly
generated developer key will not update an APK signed with the delivery key.

The delivered alpha uses the debug Gradle variant with a dedicated persistent
certificate and `testOnly=false`; it can be installed normally without ADB.

## Public API deployment

The phone uses `https://welite.ddns.net` on public HTTPS port 443. The server must
install the prepared read-only Nginx routes; see
[deployment instructions](deployment/NETWORK.md) and
[installer](deployment/install-public-api.sh). The original 8444 listener stays
private. Version 0.1.11 updates earlier alphas in place when signed with the delivery key.

## Installation / first run

1. Transfer the APK to the phone and allow installation from the app opening it.
2. Open Wachiland Launcher, complete first-run runtime setup, and sign in through
   Microsoft. The launcher remains horizontal; the external browser has its own UI.
3. Select a global profile, create its instance, wait for installation, then Start.

Allocate enough free storage for the game, full modpack and download cache. Logs
are under the application's scoped `files/logs` folder and can be obtained with
the engine's log / crash export tools. Do not uninstall to troubleshoot a crash
before exporting worlds: Android uninstallation removes app scoped files.

## Android exit diagnostics

After an abrupt restart, open the launcher before launching Minecraft again.
The launcher queries Android's own recent process exit records in the background
(Android 11/API 30+). It records reason, signal/exit status, timestamps, process,
sampled RSS/PSS, a snapshot of current available physical memory and the last
game log. Native tombstones, when retained by Android 12+, are saved as protobuf
`.pb` files; retained ANR traces are saved separately as text.

Use **Diagnóstico** at the bottom of the Wachiland home, then **Compartir
diagnóstico**, to share `wachiland-android-diagnostic.zip`. Recovery may also
show the last abnormal exit with that same sharing action. Collection is local;
no automatic uploads or permission prompts are added. Logs mask access tokens.
Exit history is collected on recovery/on request. Optional memory recording is enabled by default in 0.1.7 and can be disabled with the checkbox in Diagnóstico; it runs every 10 seconds during the game (three-hour cap), with Debug.MemoryInfo every 30 seconds and one initial mapping summary. Missing detailed fields between full samples are intentional. The ZIP includes two older memory sessions plus the current one. Basic recording never forces GC, changes the renderer/modpack or uploads automatically. Optional deep attribution explicitly requests one live census and full GC after the ModernFix menu marker; see the next section. The 0.1.6 default minimum of 512 MiB is preserved with selected maximum and explicit minimum overrides; its phone gate showed this alone was insufficient to prevent LOW_MEMORY. See diagnostics/MEMORY-SAMPLING-2026-10-07.md for metric boundaries.

The first installation of this feature can read older records if Android kept
them, but cannot correlate those runs with its new durable game session marker.
Some devices report memory kills as SIGKILL; that signal alone is inconclusive.
RSS/PSS are sampled values, not peaks. A missing record/trace is reported as
unavailable, never classified as a proven memory or mod crash. See `BUILD.md`.

## Optional Java object attribution (0.1.7)

Open **Diagnóstico**, enable **Investigar objetos Java en la próxima partida**,
then start the game. The checkbox is consumed by one launch and is off by default.
It also enables basic memory recording. Stay at the main menu for at least 30
seconds before attempting world creation, then share the normal diagnostic ZIP.

The optional recording samples allocation stacks (20/s target), old-object
allocation stacks and GC events for at most 20 minutes. Complete JFR snapshots
replace the previous snapshot every 30 seconds; an abrupt kill preserves the
latest complete file. The rolling repository target is 16 MiB, export cap 32 MiB
per snapshot. JFR does not record argument/system-property events or object values.

After the current-session ModernFix menu marker, one occupied-class histogram
and one live-class histogram are requested. The live census requests a full GC
and can pause the game. A recent Android sample must show at least 768 MiB
available and no low-memory flag, otherwise that census is skipped. This margin
is a diagnostic guard, not a guarantee against Android termination. If the marker
or HotSpot diagnostic facility is unavailable, no substitute forced census is
run; `heap-status.txt` describes outcomes and operation durations.

Allocation weights are traffic estimates, not retained bytes. Histograms report
shallow sizes; an array's owner cannot be inferred from its class alone. Old-object
samples can identify creation sites of surviving objects, but this first pass
does not request expensive reference paths to GC roots. Missing samples are not
proof that a mod retains nothing. These runs are not startup timing benchmarks
or proof of a memory improvement. No mod/cache/renderer/quality changes are made.

For offline analysis on a PC with JDK 21:

```powershell
java mobile/android/tools/HeapAttributionReport.java path/to/memory-session-allocation.jfr
```

See [diagnostic design and evidence](diagnostics/JAVA-OBJECT-ATTRIBUTION-2026-10-07.md) and the [physical Vivo JFR/OEM-stop result](diagnostics/VIVO-JFR-THERMAL-2026-10-07.md). The phone produced valid JFR; the live census was skipped for insufficient headroom.

## Thermal candidate and recording (0.1.8)

**Diagnóstico → Modo térmico en la próxima partida** defaults on. It requests
sustained performance if the platform supports it and replaces the engine's
maximum-refresh request with60Hz on Android12+. Resolution, renderer and game
settings are preserved. This is an OS request, not a game FPS limiter or a proven
fix for overheating. The checkbox restores the prior policy when switched off.

Basic recording now includes an independent10s thermal/battery/charging/process
CPU sampler and thermal-status change events in the local ZIP. Battery temperature
is not the CPU/GPU temperature, and absent thermal data is not proof the phone is
cool. Leave deep Java attribution **off** for thermal trials. See
[design, boundaries and phone gate](diagnostics/ANDROID-THERMAL-2026-10-07.md).

Physical0.1.8 follow-up: thermal capture worked, sustained mode is unsupported
on the Vivo, and world entry failed with Java heap exhaustion during WorldEdit
initialization followed by a later signal35 exit. See the
[WorldEdit and thermal result](diagnostics/VIVO-WORLDEDIT-THERMAL-2026-10-07.md).

Next run without WorldEdit/SableEdit entered and saved a world, but the user
reported~1FPS. JEI first-join initialization took4.569min, thermal headroom was
high and memory remained tight. See
[world-entry/performance evidence](diagnostics/VIVO-WORLD-ENTRY-2026-10-07.md).

Visual-mod reduction trial retained~1FPS and ended in Android LOW_MEMORY during
resource reload. FancyMenu logged only one active panorama; see
[reload memory evidence and attribution limits](diagnostics/VIVO-RELOAD-LMK-2026-10-07.md).

## Optional frame investigation (0.1.9)

**Diagnostico -> Investigar FPS en la proxima partida** manually arms one automatic
capture; fresh preferences default off from 0.1.11. Enter a world, let JEI finish, then remain in the
same scene for about four minutes. Exit normally and share the usual diagnostic
ZIP. Do not run Spark or deep Java attribution during this capture. Rearm the
checkbox to repeat a comparison without transferring another APK or mod.

The agent records frame/tick/upload/render/presentation wall durations, selected
thread CPU/stacks, effective Kerria state, and matched memory/GC/thermal series.
It changes no graphics settings. Inclusive wall metrics overlap and do not measure
GPU execution directly. Missing hooks are reported explicitly. Java 17+ and the
runtime instrumentation library are required; unsupported runtimes skip the agent.
Physical phone validation and performance conclusions remain pending. See
[design and interpretation](diagnostics/ANDROID-FRAME-DIAGNOSTICS-2026-10-08.md),
[Spark baseline](diagnostics/VIVO-SPARK-CLIENT-2026-10-08.md), and
[optimization-mod audit](diagnostics/ANDROID-OPTIMIZATION-MOD-AUDIT-2026-10-08.md).

## Diagnostic recovery hardening (0.1.10)

The first0.1.9 armed attempt ended before JVM/profiler output; its cause is
unknown because the next run overwrote its log. The later run exited normally
with both collection switches off. See
[diagnostic10 evidence](diagnostics/VIVO-DIAGNOSTIC-10-2026-10-09.md).

0.1.10 stages a minimal premain bootstrap and activates the profiler from the
ordinary wrapper after basic telemetry starts. Java profiler loading failures
are caught and recorded; native failures/OS termination remain possible. The ZIP
now includes the allowlisted launch-settings JSON, three prior attempt logs and
three crash-page reports. Markers preserve each initialization stage.

Finish any Spark trial with **FPS capture off, basic recording on**. For the
separate frame trial, enable **Investigar FPS en la proxima partida** manually;
the previous consumed/disabled preference is preserved across the update.
Physical startup, hook coverage and observer cost remain pending.

## Native profiler crash fix (0.1.11)

The 0.1.10 first-attempt log identifies a native crash in dynamic bootstrap JAR
append. Version 0.1.11 loads the counters using the startup bootstrap path and
preserves Cacio entries. See [confirmed cause and device gate](diagnostics/ANDROID-PROFILER-NATIVE-CRASH-2026-10-09.md).

## Experimental animated-atlas candidate (separate mod)

The complete [Vivo frame capture 11](diagnostics/VIVO-FRAME-CAPTURE-11-2026-10-09.md)
identifies repeated animated-atlas uploads as the largest measured frame front.
[Wachiland Atlas Batch](mods/atlas-batch/README.md) is an isolated experimental
client JAR usable with APK 0.1.11. It retains the latest dirty discrete upload
before rendering, preserving logical ticks and keeping interpolation immediate.
It has runtime on/off commands and phone validation remains pending. See the
[mechanism, limits and decision gate](diagnostics/ANDROID-ATLAS-BATCH-2026-10-09.md).

## Build evidence

Compilation and APK signature/manifest inspection are recorded in `BUILD.md`.
No physical Android device was attached during implementation. Microsoft's device
authorization endpoint accepted the public client ID; user account authentication,
touch layout and actual Minecraft / modpack startup still need the phone run.
