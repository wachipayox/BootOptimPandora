# Official server FPS investigation (physical Vivo, experimental)

## User steering

User paused the RAM experiment and authorized testing on the official multiplayer server. Target is 30 FPS, preferably 60, or an evidenced hardware limit. Player is in a safe underground shelter. No world, inventory or server changes are authorized by this investigation. The user added a private WebDisplay compatibility fork. This changes pack conditions; the combined 2816 MiB/Palladium/depth32 ZIP trial is interrupted and cannot be treated as a controlled RAM comparison. BootOptim remains untouched.

## First matched shelter capture

Vivo V2041, Mali, MobileGlues; support 0.2.9; staged uploads/direct terrain/frame wait enabled; max heap 2816 MiB, provisional ZIP depth32 and Palladium fastBitSets=false still active. Graphics settings retained, cap30/render distance3/sim5. Existing one-shot launcher frame agent, basic memory enabled, deep attribution off. Camera and player stationary, looking down sandstone tunnel. USB connected; no thermal conclusion yet.

Session memory-1791730418936-3167. Existing frame-agent origin 1791730836081 to 1791730956336 (120.254 s), after 90-second first-world-render fallback and 20-second settling. 1161 completed runTick calls = 9.6546 FPS; counter measures frames, not physical presentation. Render CPU delta 113.349 s. Inclusive client tick 68.081 s / world render26.093 s / swap0.916 s / uploads1.641 s overlap and MUST NOT be summed. 238 selected-thread stack samples: 51 include block entity ticking, 35 SmartBlockEntity, 48 AsyncParticles; these are sampled overlapping stacks, not precise exclusive per-mod attribution. Client ticks2359; AsyncParticles postTick6.637 s inclusive.

Earlier user-run log has Flywheel1.0.6 falling instancing→off after OIT fragment compile error S0060: multiple outputs require locations. Current shelter capture retains instancing and does NOT reproduce that fallback. Do not attribute the entire measured slowdown to the old error. Flywheel mesh uploads/model writes also appear in the sparse samples. Client block/entity tick cost is a separate front.

## Isolated optional shader candidate

0.2.10-flywheel-outputs, experimental.flywheelExplicitOutputs=false default. Optional @Pseudo mixin in Flywheel Compilation, remap=false require0, rewrites only the exact single four-line OIT coefficient output block to explicit layout(location=0..3). Android/config/render-thread/OpenGL33 checks, unknown source/explicit layout untouched, exceptions fail open. OitFramebuffer RENDER_TRANSMITTANCE_DRAW_BUFFERS maps output slots0..3 to attachments1..4 / coefficient layers0..3: locations preserve those intended destinations. No shader arithmetic, texture resolution, OIT algorithm, culling or simulation changes. Compilation passes. Runtime shader success, image/transparency equivalence, FPS A/B and cross-GPU validation pending. No promotion.

Raw evidence: C:/BootOptimBench/android-device-server-fps-20261011, outside Git. Baseline frame and thread CSV, original configs/mod list/log/screenshots retained. APK unchanged0.1.11. No claim of a hardware limit or 30/60 FPS yet.

## Shader trial / follow-up attribution

Support0.2.10 applied explicit outputs to six shader compilations on the actual phone; no repeated S0060 compile failure. Same shelter position and camera. Session memory-1791731190372-11964 captured859 frames /120.241s =7.1443FPS, versus previous9.6546FPS. Different world time and warming prevent causal regression attribution, but no FPS win is supported. Do not promote on this evidence.

Separate Spark Java sampler,10ms requested interval,60s, local --save-to-file. Render thread59720ms sampled wall. Client tick40760ms inclusive; particles13430ms; ParticleRain hook5780ms /tickBlockFX5210ms / Palladium biome3450ms; block-entity tick14930ms, Create smart ticker10470ms. These overlap. ParticleCore light update2850ms inclusive; source creates a ConcurrentHashMap each particle engine tick and eagerly queries cached light before ticking particles, despite their potential invisibility. Source proves mechanism, not net regression: compare LIGHTMAP disabled against same build/settings. Observable redirect is cheap inactive stock delegation; do not blame its stack name.

CPU frequency snapshot: little1500MHz,big1418MHz; readable big scaling_max_freq1418MHz. This is below advertised nominal peak, but thermal/power cause not established and snapshot is not sustained clock profiling. Battery snapshot38.2C on USB is not CPU/GPU temperature. No forced clock changes. Selected shader compatibility trial remains opt-in, defaultfalse. Performance agent capture and subsequent Spark interval are distinct; never combine their durations.

## ParticleCore LIGHTMAP comparison

Only disableOptimizations changed NONE to LIGHTMAP in existing ParticleCore JSON, support0.2.10 otherwise unchanged. Actual startup log confirms ParticleBrightnessCacheMixin and ParticleManagerCachedLightMixin disabled. Session memory-1791732347281-17161:595 frames/120.280s=4.9468FPS; client tick75.972s/2354 calls,world render21.563s/596 calls,AsyncParticles postTick6.377s inclusive. Battery40.0C on USB at end, versus earlier38.2C snapshot; world/weather/time drift and heat prevent assigning regression solely to LIGHTMAP. No win. OriginalJSON/NONE restored rather than removing ParticleCore. Third-party optimization wrapper presence alone did not prove overhead.

## Consecutive ParticleRain biome read candidate

0.2.11-rain-biome-probe, experimental.rainBiomeMemo=false. Exact ParticleRain4.0.0-beta.10 version gate. Render-thread-only scope around tickBlockFX with finally cleanup; one holder/world/copied coordinate key, no retained world after invocation. Nested scopes invalidate and use stock reads. Only consecutive same-position/same-world getBiome queries reuse the first result; getBlockState/collision/height/particle insertion invalidate before mod callbacks. Original RNG, iteration order, particle count, geometry, life and lighting unchanged. Unknown version or disabled flag delegates stock. Build passes; runtime hit counts and same-JVM FPS/visual gates pending. Explicit client rainmemo on/off/status commands do not persist settings. Existing full-frame A/B/A recorder extended with one exclusive marker wachiland-rain-biome-probe.txt;60s warmup,60s control,60s candidate,60s control, then restores original flag. No duplicate profiler or new APK. Heavy launcher frame/Spark/deep collectors disabled for the bounded mod trial. Original particle settings restored. Phone game stopped before staging to cool after battery40.0C.

## Physical rain A/B/A: inconclusive, remains disabled

Actual server run, support0.2.11. First world-frame17:55:05; 60s warmup followed by60s control/candidate/control. Phase1:129/59.930772542s=2.1525FPS; phase2:297/59.809285311s=4.9658FPS; phase3:388/60.100938081s=6.4558FPS. Rising FPS continues after disabling candidate, so it is not evidence of a win. Candidate scope runs1654397 times, exact-version gate passes, but hits=0/misses=0. Either the active weather/particle conditions bypass biome calls or another transformation replaces the getter; physical coverage must be established before interpreting cache efficacy. No claim that read reuse ran. Battery38.9C at end, versus37.9C before launch; USB charging. Different server state/weather and continued warm-up are uncontrolled. No hardware-limit conclusion.

Game exited through Disconnect/Quit after capture. Removed only the specific rain probe marker and restored diagnostics=false/rainBiomeMemo=false config. Candidate remains opt-in, not promoted. APK unchanged. Raw final log rain-final-game.log and restored properties retained outside Git. Next audit: AsyncParticles interactions with ParticleRain/getBiome, then target the larger Create/client tick burden using actual source and comparable evidence.
