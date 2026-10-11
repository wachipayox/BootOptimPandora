package net.wachiland.atlasbatch;

import com.llamalad7.mixinextras.injector.wrapoperation.Operation;
import com.mojang.blaze3d.systems.RenderSystem;
import net.minecraft.client.multiplayer.ClientLevel;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.world.level.biome.Biome;
import net.neoforged.fml.ModList;

/** Invocation-local read reuse, never a world/tick cache. No RNG or spawn filtering. */
public final class ParticleRainBiomeMemo {
    private static Thread owner;
    private static int depth;
    private static ClientLevel world;
    private static long position;
    private static Holder<Biome> biome;
    private static boolean checked, supported;
    private static long scopes, hits, misses;

    public static boolean begin() {
        if (!AndroidSupportConfig.ANDROID || !AndroidSupportConfig.enabled
            || !AndroidSupportConfig.rainBiomeMemo || !RenderSystem.isOnRenderThread()) return false;
        if (!checked) {
            checked=true;
            supported=ModList.get().getModContainerById("particlerain")
                .map(c -> c.getModInfo().getVersion().toString().equals("4.0.0-beta.10")).orElse(false);
        }
        if (!supported) return false;
        owner=Thread.currentThread(); depth++; scopes++; invalidate(); return true;
    }
    public static void end() {
        invalidate();
        if (--depth==0) owner=null;
    }
    public static void invalidate() {
        if (owner==Thread.currentThread()) { world=null; biome=null; }
    }
    public static Holder<Biome> read(ClientLevel level,BlockPos pos,Operation<Holder<Biome>> original) {
        if (depth!=1 || owner!=Thread.currentThread()) return original.call(level,pos);
        long key=pos.asLong();
        if (level==world && key==position && biome!=null) { hits++; return biome; }
        Holder<Biome> result=original.call(level,pos);
        misses++; world=level; position=key; biome=result;
        return result;
    }
    public static String status() {
        return "rain_hook_installed="+AtlasBatchMixinPlugin.rainHookInstalled()+" rain_biome_memo="+AndroidSupportConfig.rainBiomeMemo+" version_checked="+checked
            +" supported="+supported+" scopes="+scopes+" hits="+hits+" misses="+misses;
    }
}
