package net.wachiland.atlasbatch;

import com.mojang.logging.LogUtils;
import java.lang.reflect.Field;
import java.nio.file.Files;
import net.minecraft.client.Minecraft;
import org.slf4j.Logger;

/** Explicit local marker opts into a reversible, same-session A/B/A diagnostic. */
final class RenderAheadProbe {
    private static final Logger LOG=LogUtils.getLogger();
    private static final long PHASE_NS=60_000_000_000L;
    private static boolean checked, requested, stopped;
    private static boolean stagingTrial, originalStaging;
    private static boolean terrainTrial, originalTerrain;
    private static Object advanced;
    private static Field limit;
    private static int original, phase=-1;
    private static long start, phaseStart, frameStart, frameCount, frameNs, glCount, glNs, pixels;
    private static Thread owner;
    static boolean isRequested() { return requested; }

    static void begin(Minecraft mc, boolean renderLevel) {
        frameStart=0;
        owner=Thread.currentThread();
        if (!AndroidSupportConfig.observe() && requested && !stopped) { stop("configuration_disabled"); return; }
        boolean world=renderLevel && mc.level!=null && mc.screen==null && mc.getOverlay()==null;
        if (!checked && world && AndroidSupportConfig.observe()) {
            checked=true;
            boolean renderAhead=Files.isRegularFile(mc.gameDirectory.toPath().resolve("wachiland-render-ahead-probe.txt"));
            stagingTrial=Files.isRegularFile(mc.gameDirectory.toPath().resolve("wachiland-staging-probe.txt"));
            terrainTrial=Files.isRegularFile(mc.gameDirectory.toPath().resolve("wachiland-terrain-probe.txt"));
            requested=renderAhead || stagingTrial || terrainTrial;
            if ((renderAhead ? 1 : 0)+(stagingTrial ? 1 : 0)+(terrainTrial ? 1 : 0)>1) { requested=false; LOG.warn("[Wachiland render ahead] Conflicting trial markers; no trial started"); }
            if (requested) {
                try {
                    originalStaging=AndroidSupportConfig.stagedUploads;
                    originalTerrain=AndroidSupportConfig.directTerrainDraws;
                    if (!stagingTrial && !terrainTrial) {
                    Object options=Class.forName("net.caffeinemc.mods.sodium.client.SodiumClientMod").getMethod("options").invoke(null);
                    advanced=options.getClass().getField("advanced").get(options);
                    limit=advanced.getClass().getField("cpuRenderAheadLimit");
                    original=limit.getInt(advanced);
                    if (original<0 || original>9) throw new IllegalArgumentException("unknown limit");
                    }
                    start=System.nanoTime();
                    LOG.info("[Wachiland render ahead] start epoch_ms={} mode={} original={} phase_seconds=60 warmup_seconds=60",System.currentTimeMillis(),stagingTrial ? "staging" : terrainTrial ? "terrain" : "render_ahead",original);
                } catch (ReflectiveOperationException | LinkageError | IllegalArgumentException e) {
                    stopped=true;
                    LOG.warn("[Wachiland render ahead] unavailable; no setting changed",e);
                }
            }
        }
        if (!requested || stopped) return;
        if (!world) { stop("screen_or_world_exit"); return; }
        long now=System.nanoTime();
        int next=(int)((now-start)/PHASE_NS);
        if (next>=4) { stop("complete"); return; }
        if (next!=phase) {
            finishPhase(now);
            phase=next;
            int value=phase==2 ? 1 : original;
            try { if (stagingTrial) AndroidSupportConfig.stagedUploads=phase==2; else if (terrainTrial) AndroidSupportConfig.directTerrainDraws=phase==2; else limit.setInt(advanced,value); }
            catch (ReflectiveOperationException e) { stop("setting_failed"); return; }
            phaseStart=now;
            LOG.info("[Wachiland render ahead] phase={} limit={} mode={} staging={} epoch_ms={}",phase,value,stagingTrial ? "staging" : terrainTrial ? "terrain" : "render_ahead",AndroidSupportConfig.stagedUploads,System.currentTimeMillis());
            if (terrainTrial) LOG.info("[Wachiland terrain trial] phase={} {}",phase,DirectTerrainDraw.status());
        }
        frameStart=now;
    }
    static void endFrame() {
        if (frameStart!=0) { frameNs+=System.nanoTime()-frameStart; frameCount++; frameStart=0; }
    }
    static long beginUpload() { return requested && !stopped && frameStart!=0 && Thread.currentThread()==owner ? System.nanoTime() : 0; }
    static void endUpload(long began, int width, int height) {
        if (began==0) return;
        glCount++; glNs+=System.nanoTime()-began;
        pixels+=(long)Math.max(0,width)*Math.max(0,height);
    }
    private static void finishPhase(long now) {
        if (phase<0) return;
        LOG.info("[Wachiland render ahead] result phase={} limit={} elapsed_ns={} frames={} frame_wall_ns={} upload_calls={} upload_wall_ns={} pixels={} epoch_ms={}",phase,phase==2 ? 1 : original,now-phaseStart,frameCount,frameNs,glCount,glNs,pixels,System.currentTimeMillis());
        if (stagingTrial) LOG.info("[Wachiland staging trial] phase={} {}",phase,StagedAtlasUpload.status());
        if (terrainTrial) LOG.info("[Wachiland terrain trial] phase={} {}",phase,DirectTerrainDraw.status());
        frameCount=frameNs=glCount=glNs=pixels=0;
    }
    private static void stop(String why) {
        finishPhase(System.nanoTime());
        stopped=true;
        boolean restored=false;
        try { if (stagingTrial) { AndroidSupportConfig.stagedUploads=originalStaging; restored=true; } else if (terrainTrial) { AndroidSupportConfig.directTerrainDraws=originalTerrain; restored=true; } else if (limit!=null) { limit.setInt(advanced,original); restored=true; } }
        catch (ReflectiveOperationException e) { LOG.warn("[Wachiland render ahead] restore failed",e); }
        LOG.info("[Wachiland render ahead] stopped reason={} restored={} original={} epoch_ms={}",why,restored,original,System.currentTimeMillis());
    }
}
