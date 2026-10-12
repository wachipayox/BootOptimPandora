package net.wachiland.atlasbatch.mixin;

import org.spongepowered.asm.mixin.Final;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Overwrite;
import org.spongepowered.asm.mixin.Shadow;
import org.spongepowered.asm.mixin.Unique;

/** Memoizes only pure modulo arithmetic. Live slot/range/chunk reads stay stock. */
@Mixin(targets="net.minecraft.client.multiplayer.ClientChunkCache$Storage")
abstract class CullingSlotIndexMixin {
    @Shadow @Final private int viewRange;
    @Unique private volatile Thread wachiland$cullOwner;
    @Unique private boolean wachiland$hasX, wachiland$hasZ;
    @Unique private int wachiland$x, wachiland$z, wachiland$modX, wachiland$rowZ;

    /**
     * @author Wachiland
     * @reason Repeated culling rays reuse immutable storage-index arithmetic;
     * no callback allocation, chunk/state cache, or altered range checks.
     */
    @Overwrite
    int getIndex(int x, int z) {
        Thread thread=Thread.currentThread();
        Thread owner=wachiland$cullOwner;
        if (owner==null) {
            // Only one designated worker may mutate these four primitive values.
            if (!thread.getName().equals("CullThread")) return stockIndex(x,z);
            synchronized (this) {
                if (wachiland$cullOwner==null) wachiland$cullOwner=thread;
                owner=wachiland$cullOwner;
            }
        }
        if (thread!=owner) return stockIndex(x,z);
        if (!wachiland$hasX || x!=wachiland$x) {
            wachiland$modX=Math.floorMod(x,viewRange);
            wachiland$x=x;
            wachiland$hasX=true;
        }
        if (!wachiland$hasZ || z!=wachiland$z) {
            wachiland$rowZ=Math.floorMod(z,viewRange)*viewRange;
            wachiland$z=z;
            wachiland$hasZ=true;
        }
        return wachiland$rowZ+wachiland$modX;
    }

    @Unique private int stockIndex(int x,int z) {
        return Math.floorMod(z,viewRange)*viewRange+Math.floorMod(x,viewRange);
    }
}
