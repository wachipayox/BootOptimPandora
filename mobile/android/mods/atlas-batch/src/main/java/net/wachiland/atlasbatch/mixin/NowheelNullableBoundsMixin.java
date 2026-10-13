package net.wachiland.atlasbatch.mixin;

import com.llamalad7.mixinextras.injector.wrapmethod.WrapMethod;
import com.llamalad7.mixinextras.injector.wrapoperation.Operation;
import net.minecraft.core.BlockPos;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.phys.AABB;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Unique;

/** Exact Nowheel null-render-bounds failure: keep the object visible, finish the pass. */
@Mixin(targets="dev.tr7zw.entityculling.EntityCullingMod",remap=false)
public abstract class NowheelNullableBoundsMixin {
    @Unique private static final AABB wachiland$unknownBounds=new AABB(-1e100,-1e100,-1e100,1e100,1e100,1e100);
    @Unique private static boolean wachiland$reported;
    @WrapMethod(method="setupAABB")
    private AABB wachiland$nullableBounds(BlockEntity entity,BlockPos pos,Operation<AABB> original) {
        try { return original.call(entity,pos); }
        catch(NullPointerException error) {
            String message=error.getMessage();
            if(message!=null && message.contains("\"bb\" is null")) {
                for(StackTraceElement at:error.getStackTrace()) {
                    if(at.getClassName().equals("dev.tr7zw.entityculling.EntityCullingMod") && at.getMethodName().contains("nowheel$properAABB")) {
                        if(!wachiland$reported){wachiland$reported=true;System.out.println("[Wachiland Android support] Nowheel null render bounds: object forced visible; culling pass continues");}
                        // EntityCulling's block-entity path does NOT accept null. Its
                        // existing hitbox-limit guard makes this box explicitly visible.
                        return wachiland$unknownBounds;
                    }
                }
            }
            throw error;
        }
    }
}
