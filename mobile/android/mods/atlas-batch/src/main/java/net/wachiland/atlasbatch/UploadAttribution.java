package net.wachiland.atlasbatch;

import com.mojang.blaze3d.platform.NativeImage;
import com.mojang.logging.LogUtils;
import java.util.IdentityHashMap;
import java.util.Map;
import net.minecraft.client.Minecraft;
import net.minecraft.client.renderer.texture.SpriteContents;
import net.minecraft.client.renderer.texture.TextureAtlas;
import org.slf4j.Logger;

/** One bounded upload census; image data/pointers are never retained. */
final class UploadAttribution {
    private static final Logger LOG=LogUtils.getLogger();
    private static final int MAX_SPRITES=256;
    private static final long WARMUP_NS=60_000_000_000L, END_NS=180_000_000_000L;
    private static final Map<SpriteContents,Metric> SPRITES=new IdentityHashMap<>();
    private static final Metric OTHER=new Metric("outside_sprite_scope","unknown",false);
    private static Metric current;
    private static boolean active,finished;
    private static Thread owner;
    private static long start, frames, frameStart, frameWall, lastReport;
    private static int frameUploads;
    static final class Metric {
        final String name,atlas;
        final boolean interpolated;
        long calls,wall,pixels,maxWall,firstCalls,firstWall;
        final long[] levels=new long[16];
        final long[] levelWall=new long[16];
        Metric(String name,String atlas,boolean interpolation) { this.name=name; this.atlas=atlas; interpolated=interpolation; }
        String summary() { return "sprite="+name+" atlas="+atlas+" first_upload_interpolated="+interpolated+" calls="+calls+" wall_ns="+wall+" max_call_ns="+maxWall+" pixels="+pixels+" mip_calls="+java.util.Arrays.toString(levels)+" mip_wall_ns="+java.util.Arrays.toString(levelWall)+" first_frame_calls="+firstCalls+" first_frame_wall_ns="+firstWall; }
    }
    static void begin(Minecraft mc,boolean renderLevel) {
        owner=Thread.currentThread(); frameStart=0; current=null; frameUploads=0;
        boolean world=renderLevel && mc.level!=null && mc.screen==null && mc.getOverlay()==null;
        long now=System.nanoTime();
        if (world && start==0) {
            start=now;
            String osTid="unavailable";
            try { osTid=java.nio.file.Files.readString(java.nio.file.Path.of("/proc/thread-self/stat")).split(" ",2)[0]; }
            catch (Exception ignored) { /* Optional Linux attribution; never block a launch. */ }
            LOG.info("[Wachiland upload census] warmup epoch_ms={} warmup_seconds=60 capture_seconds=120 os_tid={} java_thread={}",System.currentTimeMillis(),osTid,owner.getName());
        }
        active=world && start!=0 && now-start>=WARMUP_NS && now-start<END_NS && !RenderAheadProbe.isRequested();
        if (active) {
            frameStart=now;
            if (frames==0) LOG.info("[Wachiland upload census] capture_start epoch_ms={}",System.currentTimeMillis());
            if (now-lastReport>=30_000_000_000L) { lastReport=now; report(false); }
        } else if (!finished && start!=0 && now-start>=END_NS) { finished=true; report(true); SPRITES.clear(); }
    }
    static Object enter(SpriteContents sprite,TextureAtlas atlas,NativeImage[] images) {
        Metric previous=current;
        if (!active || Thread.currentThread()!=owner) return previous;
        Metric metric=SPRITES.get(sprite);
        if (metric==null && SPRITES.size()<MAX_SPRITES) {
            metric=new Metric(sprite.name().toString(),atlas==null ? "outside_atlas_scope" : atlas.location().toString(),images!=sprite.byMipLevel);
            SPRITES.put(sprite,metric);
        }
        current=metric==null ? OTHER : metric;
        return previous;
    }
    static void leave(Object previous) { if (Thread.currentThread()==owner) current=(Metric)previous; }
    static long beginUpload() { return active && Thread.currentThread()==owner ? System.nanoTime() : 0; }
    static void endUpload(long began,int width,int height,int level) {
        if (began==0) return;
        Metric metric=current==null ? OTHER : current;
        long elapsed=System.nanoTime()-began;
        metric.calls++; metric.wall+=elapsed; metric.maxWall=Math.max(metric.maxWall,elapsed);
        if (frameUploads++==0) { metric.firstCalls++; metric.firstWall+=elapsed; }
        metric.pixels+=(long)Math.max(0,width)*Math.max(0,height);
        if (level>=0 && level<16) { metric.levels[level]++; metric.levelWall[level]+=elapsed; }
    }
    static void endFrame() { if (frameStart!=0) { frames++; frameWall+=System.nanoTime()-frameStart; frameStart=0; } }
    private static void report(boolean done) {
        LOG.info("[Wachiland upload census] complete={} frames={} frame_wall_ns={} tracked_sprites={} epoch_ms={}",done,frames,frameWall,SPRITES.size(),System.currentTimeMillis());
        for (Metric metric:SPRITES.values()) LOG.info("[Wachiland upload sprite] {}",metric.summary());
        if (OTHER.calls!=0) LOG.info("[Wachiland upload sprite] {}",OTHER.summary());
    }
}
