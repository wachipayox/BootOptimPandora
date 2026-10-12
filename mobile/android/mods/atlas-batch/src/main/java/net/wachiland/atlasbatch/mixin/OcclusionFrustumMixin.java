package net.wachiland.atlasbatch.mixin;
import net.wachiland.atlasbatch.CullingFrustumGuard;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Coerce;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;
@Pseudo
@Mixin(targets="com.logisticscraft.occlusionculling.OcclusionCullingInstance",remap=false)
abstract class OcclusionFrustumMixin {
    @Inject(method="isAABBVisible",at=@At("HEAD"),cancellable=true,require=0)
    private void wachiland$outside(@Coerce Object min,@Coerce Object max,@Coerce Object camera,CallbackInfoReturnable<Boolean> ci) {
        if (CullingFrustumGuard.outside(min,max,camera)) ci.setReturnValue(true);
    }
}
