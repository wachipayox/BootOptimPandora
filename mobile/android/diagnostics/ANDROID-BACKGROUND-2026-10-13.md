# Android background lifecycle, 2026-10-13

Development branch `codex/android-background-and-hotpaths-20261013`, based on
launcher distribution dfb773701 and FCL engine3fc20c9. BootOptim unchanged.
APK0.1.17-alpha and Support0.2.26. This entry is updated through runtime gates;
compile success alone is not the promotion decision.

## Mechanism and contracts

An Activity-started, user-visible foreground session service owns a partial
wake lock. It never restarts a dead JVM, launches another app, or acquires a
network job. A bounded atomic-file UUID protocol communicates visibility and
initial completion to the embedded Java runtime. TextureView's original
SurfaceTexture/EGL producer is retained across temporary detach. Native swap
is suppressed before the surface detaches. Input focus/reset behavior remains.

Hidden world drawing, framebuffer presentation and invisible texture animation
are skipped. Minecraft scheduled work, reload GL uploads and stock tick
decisions remain. Logical client cadence remains20Hz; loading overlay completion
gets an offscreen1Hz heartbeat. A literal1Hz game loop would break normal ticks
and connection handling. Early Drippy drawing is parked1Hz on its owned thread.
No GL work moves between threads. Unknown/missing state fails visible/stock.

Default `background.enabled=true`, `enabled=false` or `background.enabled=false`
disables the coordinated feature on the next launch. The launcher requires one
Support JAR with the lifecycle marker; old/unknown support uses stock surface
handling. Ready notifications carry the current UUID only, expire on normal
session exit and are cancelled on the next application start after abrupt exit.

Battery saver warning uses public Settings. The requested “Desactivar y lanzar”
button opens Battery Saver Settings and retries launch after return; it cannot
silently disable a device-wide setting. The other button launches immediately.
Notification permission and the public battery-optimization exemption are
requested. Denial is supported. Android/vendor memory/thermal kills remain
possible; a foreground service does not reserve physical RAM.

## Runtime evidence so far

- Host launch-argument harness passes defaults without Drippy, legacy screen,
  independent startup switches, background-only, all disabled, whole addon
  disabled, expert override and unavailable native bridge. Sampler stays off.
- Physical Battery Saver warning, Settings opening and resumed launch passed.
  Temporary ADB Battery Saver/battery simulation restored before game launch.
- First physical prototype emitted game-ready and notification742, but forced
  `runTick(true)` during a deliberate `runTick(false)` teardown. It caused
  reentrant null `gameMode.tick()`. Rejected build, corrected caller contract
  and nested frame/noRender restoration. The first notification is not a full
  success gate.
- Corrected run18458, launch19:24:51, Home during early load, service741 and
  hidden UUID observed. Android terminated it19:30:17 for LOW_MEMORY,
  importance125, PSS4.7GB/RSS4.8GB during atlas publication. No new Java crash
  report. It had not reached ready. This is a failed survival gate, not a
  startup performance result. Battery exemption was not yet granted.
- Public Android permission dialog subsequently accepted; deviceidle whitelist
  confirms `user,net.wachiland.launcher,10671`. No root/platform private bypass.
- Android restoring a dead JVM Activity exposed an existing black-screen
  path with null static bridge. The Activity now returns to the launcher.
  New physical tests continue with exemption and without JFR recording.
- Second run21698, foreground until resource preparation then Home, terminated
  19:40:32 by LOW_MEMORY at importance125, PSS4.0GB/RSS4.1GB, despite exemption
  and no JFR. This rejects exemption as a sufficient fix for this pack. GC log
  near reload end showed2427MiB after a young collection and2566MiB at remark,
  with2816MiB maximum. Do not blindly lower the heap below live-load needs.
- Third run7551 stayed visible during load: ModernFix menu185.488s and world
  255.721s (menu-to-world70.233s). No JFR; not comparable to prior diagnostic
  startups as a performance win. Same safe glass location, changed camera pose
  supplied by user. Original staging remains ON. First-world viewport passed.
  Kernel MemAvailable441752KiB soon after entry. Background/AFK gate continues.

## Attribution improvement, no new profiler

`jfr print` default stack depth5 had truncated previous CPU ownership reports.
Existing600s graph-control recording re-exported at depth64. World execution
samples: main tick path3073, block-entity tick1936, render-level912; CullThread
Provider opacity path4301, Level.getChunk leaf3699. Those are sample counts,
not wall seconds, exclusive CPU or an FPS improvement. Approximate first-sample
phase boundaries are attribution-only; use explicit ready/benchmark markers for
timed comparisons. Create behaviour callbacks and EntityCulling voxel/chunk
queries remain real optimization fronts; no cache or tick skip promoted from
these counts alone.

Raw evidence outside Git: `C:/BootOptimBench/android-background-20261013`.
World AFK gate passed in run7551: same UUID/PID survived Home and returned via
the service notification with the original scene/texture restored. Stationary
30s captures were valid, rain0 and no trace overflow. Visible first4.761loops/s,
p99406.974ms; hidden1.831logical loops/s,18.143ticks/s; visible return8.898loops/s,
p99186.701ms, maximum202.750ms, no >250ms frames. First visible was still warming;
do not claim foreground FPS gain from this comparison. Hidden metrics confirmed
zero world_render and zero gl_texture_upload calls; native swap wrapper still
called56times but immediate-skip wall only176386ns. Tick work continued.

Screen sleep21:22:22 was interrupted by the user waking the phone (window resize
21:22:44). Same PID/UUID and image survived, but no complete intended screen-off
duration or Doze/endurance gate claimed. Support0.2.26 adds the next candidate:
pause EntityCulling1.10.4
raycasts and snapshot preparation only while hidden. Its global enable flag,
late init, key handling and foreground callbacks remain unchanged. Compiles;
physical semantic/counter gate was pending at this point and passed below.
No visible-world FPS claim.

Support0.2.26 gate in run14176 passed menu193.685s/world274.285s, safe glass
viewport and notification return without a new Mixin/game failure. These are
not startup wins: no comparable new optimization is active while visible.
The running launcher had cached the preceding version settings; the actual
probe base remained background2 (confirmed from the current log). Collector
corrected to that real base, rather than attributing missing output to the game.

Valid60s hidden capture1197tick calls,19.856ticks/s, zero world renders/uploads,
419swap wrappers/1.427552ms inclusive skip wall. Completed logical loops6.963/s
are not presentation FPS. Hidden loop p99538ms is not a visible frame stutter.
Foreground60s6.740loops/s,p99313.9ms remains a different/warming interval, not
evidence of an FPS win. Same pose, rain0, no trace overflow. CullThread
CPU counters (USER_HZ100)20838+282 ->20870+282 over approximately55s hidden:
0.32CPU seconds, compared with many CPU seconds accumulated while visible.
No global culling toggle changes; raycasts resume on return. Screenshot passed.
The retained change removes invisible work; no quality/RAM/foreground-FPS gain
claimed. Raw bg26-visible/hidden captures in the existing frame evidence folder.

Screen-off retry passed: sleep21:42:11, Asleep/hidden/samePID14176 observed
21:43:11, wake21:43:36 (85s interval). User unlocked the secure screen;
samePID/UUID/scene returned visible, screenshot21:44:39. No long Doze or vendor
background endurance inference from this bounded test. No lock bypass.

Initial overlay handoff hypothesis audited and not implemented. Minecraft's
onResourceLoadFinished sets gameLoadFinished before running initial screens,
and the ready hook executes only after those callbacks return. Neither hidden
startup failure reached that ready marker. Removing the overlay before its
completion callback would skip required contracts; eliminating the later fade
alone has no demonstrated saving at the observed failure point. Initial-load
RAM remains unresolved. No unvalidated cache release or forced GC introduced.

Normal disconnect/title/quit passed in both world runs; the final log ends in
`EGLBridge: Terminating`, PID14176 is gone and no foreground service remains.
Native exit can bypass the Java exit callback, leaving IPC files on disk.
The launcher now clears those files/AtomicFile backups and notifications only
when its next main process starts; diagnostic subprocess creation does not
touch a live game's session. Ordinary launch arguments are restored after
stopping the launcher so its cached version settings cannot overwrite them.
The cleanup-only follow-up compiled, retained the installation signing key and
installed successfully. Fresh main-process startup physically removed the stale
IPC and no own session notification remained. Restored config readback matches
the ordinary profile:2816MiB, MobileGlues, existing guarded ZipFS depth32 patch,
no manual frame/JFR/startup measurement arguments. Launcher/catalog screenshot
passed and installed Support0.2.26 hash matches the packaged artifact.

Checkpoint scope: short stationary-server AFK/Home and85s screen-off survival,
return image and logical tick preservation passed. Initial background loading,
long unattended AFK/Doze, meaningful startup reduction and30FPS server goal
remain unresolved. No BootOptim edit or hardware-limit conclusion.
