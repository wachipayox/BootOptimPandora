# Vivo atlas batch 0.1.0: first direct ADB trial

## Access and artifact

User authorized direct operation of their USB-connected phone, then accepted the
ADB host and unlocked the secure keyguard themselves. Shell input, app activity
launch, mod transfer/hash verification, Minecraft menu selection, world entry,
normal save/quit and direct diagnostic collection all worked. No screen stream,
root, account changes or APK update was needed. Sparse UI snapshots were used
only to locate controls; startup waits replaced repeated UI checks.

Vivo V2041, APK 0.1.11-alpha/code12, MobileGlues, thermal preference true,
deep attribution false, performance_next armed. The basic switch remained false;
the armed performance run supplies basic collection as in diagnostic 11. Kerria
JAR remains disabled. BootOptim and all other pack files are unchanged.
Candidate SHA256 c5964d804b6f855e62c6e0696ed1b67a4f09bf60a8f877b756179e714c92ae7f
was verified on-device after transfer to the local Android version's mods folder.

Session prefix memory-1791647461562-27640. Local files and raw private logs remain
under C:/BootOptimBench/android-device-atlas-trial-20261010, outside Git. The
pre-trial diagnostic preferences were backed up before arming the next launch.

## Runtime evidence and bounded result

Bootstrap/counters/agent readiness succeeded. All seven candidate mixins report
hooks_ready=true; enablement and compatibility are true. ModernFix reports
203.703s to game start and 86.33229s main-menu to in-game. These are its own
markers, not a paired startup improvement or a launcher-to-menu measurement.

The most recently played existing New World (1) was selected without creating,
editing or deleting worlds. Automatic capture follows the matched world/JEI
marker. Origin 1791648002566; final counter row 1791648123209, elapsed120.643s.
World save/quit completed at18:06:36 device local time; then Quit Game was tapped.
No new candidate Mixin error or crash was observed in this completed trial.
This proves basic startup/world entry and collection, not visual equivalence
across arbitrary mod callbacks or reload safety.

| Completed metric | Calls | Inclusive wall seconds |
| --- | ---: | ---: |
| Frame | 976 | 120.384 |
| Client tick | 2399 | 73.774 |
| GL texture upload | 9481 | 62.060 |
| World render | 976 | 23.266 |
| Swap | 977 | 1.518 |
| AsyncParticles postTick | 2399 | 4.193 |

Approximately8.090 completed frames/s and2.458 client ticks/frame. These wall
metrics overlap and must not be summed or labeled GPU execution time. Pixel
counter coverage remains partial as documented for capture11.

The candidate's cumulative world totals reached eligible multi-tick frames1860,
deferred5360, submitted5360, replaced0, flushes1860, queue0, overflow fallbacks0
before normal world exit. These cumulative counts are NOT the matched120.643s
capture counters. Zero replacements means this run establishes no avoided
submission from this mechanism, even though the queue and replay paths execute.

Render stack samples238;114 have native nglTexSubImage2D at the top. Of these,
7 include AtlasBatch.flush,107 are immediate. The immediate paths comprise
64 InterpolationData.uploadInterpolatedFrame stacks,42 discrete uploadFrame
stacks and one LightTexture upload. Stack counts are samples, not exclusive CPU
fractions or precise per-method timings. Mixin wrappers lengthen stacks; lack
of a deeper method below the recorded stack limit is not evidence it did not run.

## Interpretation and next premise

The apparent rate differs from capture11's3.410/s, but this is not an A/B.
The phone is USB-powered/charging, began at battery27.7C and was34.5C in the
post-capture snapshot; world time/scene and thermal state are not matched to that
older run. Do not attribute the difference to the mod, especially with replaced0.

The prototype has insufficient demonstrated coverage of the measured upload
front. Investigate why discrete uploads stay immediate (scope/eligibility/lifecycle
guards) and whether final-state interpolation submissions can be coalesced safely.
Do not simply broaden the scope, retain mutable images blindly or remove safety
guards. The increased interpolation stack share could also reflect moving an
implicit GPU wait to another first upload; native samples do not prove a separate
interpolation algorithm bottleneck. Further attribution and a matched control
are needed before an optimization claim.

The exact phone AsyncParticles JAR was copied read-only, SHA256
051408b17049742b6bb69661a59082aa48861f927047b126e7e6facb84521fd0.
Its NeoForge-prefixed TextureManager mixin bytecode confirms the audited
deferredTextureTick/isShouldTickParticles branch and original-operation enqueue
or immediate execution. This confirms that wrapper's shape, not all scheduling
or callback equivalence. No AsyncParticles source/pack setting was changed.

Disposition: experimental PR remains draft. Retain this result as a successful
remote device/control smoke and an inconclusive performance candidate with
zero observed coalescing. No promotion or claim of quality-preserving FPS gain.
