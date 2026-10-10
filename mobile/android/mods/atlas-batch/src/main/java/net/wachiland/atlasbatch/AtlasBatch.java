package net.wachiland.atlasbatch;

import com.mojang.blaze3d.platform.GlStateManager;
import com.mojang.blaze3d.platform.NativeImage;
import com.mojang.blaze3d.systems.RenderSystem;
import com.mojang.logging.LogUtils;
import java.util.LinkedHashMap;
import java.util.ArrayList;
import java.util.Map;
import net.minecraft.client.Minecraft;
import net.minecraft.client.renderer.texture.SpriteContents;
import net.minecraft.client.renderer.texture.TextureAtlas;
import net.neoforged.fml.ModList;
import net.wachiland.atlasbatch.mixin.SpriteUploadInvoker;
import org.slf4j.Logger;

/** Same-render-frame references only. Never retain a raw NativeImage pointer. */
public final class AtlasBatch {
    private static final Logger LOG = LogUtils.getLogger();
    private static final int MAX_PENDING = 2048;
    private static final Map<Key, Upload> PENDING = new LinkedHashMap<>();
    private static volatile boolean enabled = Boolean.parseBoolean(System.getProperty("wachiland.atlasBatch", "false"));
    private static Thread owner;
    private static TextureAtlas atlas;
    private static TextureAtlas observedAtlas;
    private static boolean eligible, batching, replaying;
    private static volatile boolean invalidated;
    private static boolean compatibilityChecked, compatible=true;
    private static final int[] UNPACK = {4,0,0,0}; // alignment, row length, skip rows, skip pixels
    private static long frames, deferred, replaced, submitted, flushes, limitFallbacks;
    private static long lastReport;
    private static final class Key {
        final SpriteContents sprite; final TextureAtlas atlas; final int unit, x, y;
        Key(SpriteContents sprite, TextureAtlas atlas, int unit, int x, int y) { this.sprite=sprite; this.atlas=atlas; this.unit=unit; this.x=x; this.y=y; }
        public int hashCode() { return ((System.identityHashCode(sprite)*31 + System.identityHashCode(atlas))*31+unit)*31+x*31+y; }
        public boolean equals(Object o) { return o instanceof Key k && sprite==k.sprite && atlas==k.atlas && unit==k.unit && x==k.x && y==k.y; }
    }
    private record Upload(Key key, int frameX, int frameY, NativeImage[] images) { }

    public static void begin(Minecraft mc, boolean renderLevel) {
        // Flush stale work before a new scope; screen/reload/menu paths stay stock.
        flush();
        owner=Thread.currentThread(); atlas=null;
        observedAtlas=null;
        RenderAheadProbe.begin(mc,renderLevel);
        UploadAttribution.begin(mc,renderLevel);
        AtlasDiagnostics.begin(mc,renderLevel);
        if (!compatibilityChecked && mc.level!=null) {
            compatibilityChecked=true;
            // Kerria owns upload-cache/PBO lifecycle. Do not defer its callbacks.
            compatible=!ModList.get().isLoaded("kerria");
            if (!compatible) LOG.warn("[Wachiland atlas batch] Stock fallback: Kerria is loaded");
        }
        batching=false;
        eligible=enabled && compatible && AtlasBatchMixinPlugin.ready() && renderLevel && mc.level!=null && mc.screen==null && mc.getOverlay()==null && RenderSystem.isOnRenderThread();
        report(false);
    }
    public static void tickCount(int ticks) { batching=eligible && ticks>1; if (batching) frames++; }
    public static void end() { try { flush(); } finally { batching=false; eligible=false; atlas=null; } }
    public static TextureAtlas enterAtlas(TextureAtlas value) {
        AtlasDiagnostics.visit();
        TextureAtlas previous=atlas;
        if (Thread.currentThread()!=owner) return null;
        if (batching && value.getClass()==TextureAtlas.class) atlas=value;
        else { AtlasDiagnostics.reason(batching ? "atlas_class" : "atlas_outside_batch"); flush(); batching=false; atlas=null; }
        return previous;
    }
    public static void leaveAtlas(TextureAtlas previous) { if (Thread.currentThread()==owner) atlas=previous; }
    public static TextureAtlas observeAtlas(TextureAtlas value) { TextureAtlas previous=observedAtlas; if (Thread.currentThread()==owner) observedAtlas=value; return previous; }
    public static void leaveObservedAtlas(TextureAtlas previous) { if (Thread.currentThread()==owner) observedAtlas=previous; }
    public static void endFrameProbe() { RenderAheadProbe.endFrame(); UploadAttribution.endFrame(); }
    public static long beginUploadProbe() { return RenderAheadProbe.beginUpload(); }
    public static void endUploadProbe(long began,int width,int height) { RenderAheadProbe.endUpload(began,width,height); }
    public static Object enterSpriteProbe(SpriteContents sprite,NativeImage[] images) { return UploadAttribution.enter(sprite,observedAtlas,images); }
    public static void leaveSpriteProbe(Object previous) { UploadAttribution.leave(previous); }
    public static long beginCensusUpload() { return UploadAttribution.beginUpload(); }
    public static void endCensusUpload(long began,int width,int height,int level) { UploadAttribution.endUpload(began,width,height,level); }

    public static boolean defer(SpriteContents sprite, int x, int y, int frameX, int frameY, NativeImage[] images) {
        if (Thread.currentThread()!=owner) return false;
        if (!replaying) AtlasDiagnostics.upload(sprite,observedAtlas,x,y,images==sprite.byMipLevel);
        if (invalidated) { AtlasDiagnostics.reason("worker_invalidation"); PENDING.clear(); batching=false; invalidated=false; }
        if (!batching || replaying || atlas==null) { if (!replaying) AtlasDiagnostics.reason(!batching ? "upload_batch_inactive" : "upload_no_atlas"); return false; }
        // Interpolation buffers and unknown subclasses always keep their original route.
        // If a pending discrete update exists, preserve ordering before that route.
        if (sprite.getClass()!=SpriteContents.class) { AtlasDiagnostics.reason("sprite_class"); flush(); batching=false; return false; }
        if (images==null || images.length==0 || images.length>16 || x<0 || y<0 || frameX<0 || frameY<0) { AtlasDiagnostics.reason("upload_shape"); flush(); batching=false; return false; }
        int unit=GlStateManager._getActiveTexture()-33984;
        if (unit<0 || unit>=GlStateManager.TEXTURES.length || GlStateManager.TEXTURES[unit].binding!=atlas.getId()) { AtlasDiagnostics.reason("atlas_binding"); flush(); batching=false; return false; }
        Key key=new Key(sprite,atlas,unit,x,y);
        // A live interpolation upload supersedes any earlier queued discrete
        // update of this same sprite. Other disjoint sprite updates stay pending.
        if (images!=sprite.byMipLevel) { AtlasDiagnostics.reason("interpolation_immediate"); if (PENDING.remove(key)!=null) replaced++; return false; }
        if (PENDING.size()>=MAX_PENDING && !PENDING.containsKey(key)) { limitFallbacks++; flush(); batching=false; return false; }
        if (PENDING.put(key,new Upload(key,frameX,frameY,images))!=null) replaced++;
        deferred++;
        return true;
    }
    public static void flush() {
        if (invalidated && Thread.currentThread()==owner) { PENDING.clear(); batching=false; invalidated=false; }
        if (replaying || PENDING.isEmpty()) return;
        if (!RenderSystem.isOnRenderThread() || Thread.currentThread()!=owner) return;
        int active=GlStateManager._getActiveTexture();
        int[] unpack=UNPACK.clone();
        int[] saved=new int[GlStateManager.TEXTURES.length];
        boolean[] touched=new boolean[saved.length];
        ArrayList<Upload> uploads=new ArrayList<>(PENDING.values());
        PENDING.clear(); replaying=true; flushes++;
        try {
            for (Upload u : uploads) {
                if (invalidated) { batching=false; break; }
                int unit=u.key.unit;
                if (!touched[unit]) { saved[unit]=GlStateManager.TEXTURES[unit].binding; touched[unit]=true; }
                GlStateManager._activeTexture(33984+unit);
                u.key.atlas.bind();
                // Invoke the original Minecraft upload with the latest dirty coordinates.
                // All mip levels and original exceptions are preserved.
                ((SpriteUploadInvoker)(Object)u.key.sprite).wachiland$upload(u.key.x,u.key.y,u.frameX,u.frameY,u.images);
                submitted++;
            }
        } finally {
            try {
                for (int unit=0;unit<touched.length;unit++) if (touched[unit]) { GlStateManager._activeTexture(33984+unit); GlStateManager._bindTexture(saved[unit]); }
                GlStateManager._activeTexture(active);
                GlStateManager._pixelStore(3317,unpack[0]);
                GlStateManager._pixelStore(3314,unpack[1]);
                GlStateManager._pixelStore(3315,unpack[2]);
                GlStateManager._pixelStore(3316,unpack[3]);
            } finally { replaying=false; uploads.clear(); }
        }
    }
    public static void beforeConsume() { if (!PENDING.isEmpty() && !replaying) AtlasDiagnostics.reason("consume_flush"); flush(); }
    public static void beforeUpload() { if (atlas==null || !batching) { if (!PENDING.isEmpty() && !replaying) AtlasDiagnostics.reason("upload_flush"); flush(); } }
    public static void unpack(int name,int value) {
        switch (name) { case 3317 -> UNPACK[0]=value; case 3314 -> UNPACK[1]=value; case 3315 -> UNPACK[2]=value; case 3316 -> UNPACK[3]=value; default -> { } }
    }
    public static void lifecycle() {
        if (Thread.currentThread()==owner && RenderSystem.isOnRenderThread()) { AtlasDiagnostics.reason("owner_lifecycle"); end(); }
        else invalidated=true; // Drop invalidated generations, never call GL from a worker.
    }
    public static void setEnabled(boolean value) { end(); enabled=value; report(true); }
    public static String status() { return "Wachiland atlas: enabled="+enabled+" compatible="+compatible+" hooks_ready="+AtlasBatchMixinPlugin.ready()+" frames="+frames+" deferred="+deferred+" replaced="+replaced+" submitted="+submitted+" pending="+PENDING.size(); }
    private static void report(boolean force) {
        long now=System.currentTimeMillis();
        if (force || now-lastReport>=30000) { lastReport=now; LOG.info("[Wachiland atlas batch] {} flushes={} limit_fallbacks={}",status(),flushes,limitFallbacks); LOG.info("[Wachiland atlas attribution] {}",AtlasDiagnostics.summary()); }
    }
}
