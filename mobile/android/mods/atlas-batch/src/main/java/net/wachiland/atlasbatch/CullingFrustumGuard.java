package net.wachiland.atlasbatch;

import java.nio.file.Files;
import java.util.concurrent.atomic.LongAdder;
import net.minecraft.client.Minecraft;
import net.minecraft.client.multiplayer.ClientLevel;
import net.minecraft.client.renderer.culling.Frustum;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.Vec3;
import net.neoforged.fml.loading.FMLPaths;
import net.wachiland.atlasbatch.mixin.OcclusionVecAccess;

/** Visible-only shortcut: never rejects an object. Immutable camera copies, no worker GL. */
public final class CullingFrustumGuard {
    private record Snapshot(ClientLevel level, Frustum frustum, Vec3 camera, long created) { }
    private static volatile Snapshot snapshot;
    private static final LongAdder calls=new LongAdder(), skipped=new LongAdder();
    private CullingFrustumGuard() { }
    private static boolean active() {
        return AndroidSupportConfig.ANDROID && AndroidSupportConfig.enabled && AndroidSupportConfig.frustumCullingQueries;
    }
    public static void capture(Frustum frustum, Vec3 camera) {
        Minecraft mc=Minecraft.getInstance();
        if (!active() || mc.level==null || mc.screen!=null) { snapshot=null; return; }
        // Frustum copy owns its intersection/matrix. Never call its mutating offset method.
        snapshot=new Snapshot(mc.level,new Frustum(frustum),camera,System.nanoTime());
    }
    public static boolean outside(Object min, Object max, Object camera) {
        if (!active() || !Thread.currentThread().getName().equals("CullThread")) return false;
        calls.increment();
        Snapshot s=snapshot;
        if (s==null || System.nanoTime()-s.created>500_000_000L) return false;
        Minecraft mc=Minecraft.getInstance();
        if (mc.level!=s.level || mc.screen!=null || !(min instanceof OcclusionVecAccess a)
            || !(max instanceof OcclusionVecAccess b) || !(camera instanceof OcclusionVecAccess c)) return false;
        double cx=c.wachiland$x(),cy=c.wachiland$y(),cz=c.wachiland$z();
        if (!Double.isFinite(cx)||!Double.isFinite(cy)||!Double.isFinite(cz)
            || Math.abs(cx-s.camera.x)>0.001 || Math.abs(cy-s.camera.y)>0.001 || Math.abs(cz-s.camera.z)>0.001) return false;
        double x=a.wachiland$x(),y=a.wachiland$y(),z=a.wachiland$z();
        double xx=b.wachiland$x(),yy=b.wachiland$y(),zz=b.wachiland$z();
        if (!Double.isFinite(x)||!Double.isFinite(y)||!Double.isFinite(z)||!Double.isFinite(xx)
            ||!Double.isFinite(yy)||!Double.isFinite(zz)||xx<x||yy<y||zz<z) return false;
        // Same corrected bounds that EntityCulling/Nowheel would pass into its rays.
        if (!s.frustum.isVisible(new AABB(x,y,z,xx,yy,zz))) { skipped.increment(); return true; }
        return false;
    }
    public static void clear() {
        snapshot=null; // Do not retain a previous level after disconnect/reload.
        long count=calls.sumThenReset(), avoided=skipped.sumThenReset();
        if (count==0) return;
        // World transition only, after measurement. No per-frame/per-query file writes.
        try { Files.writeString(FMLPaths.GAMEDIR.get().resolve("wachiland-culling-frustum.txt"),
            "epoch_ms="+System.currentTimeMillis()+"\ncalls="+count+"\nskipped="+avoided+"\nvisible_only=true\n"); }
        catch (Exception ignored) { }
    }
}
