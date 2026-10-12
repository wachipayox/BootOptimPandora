package net.wachiland.atlasbatch.mixin;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.gen.Accessor;
@Pseudo
@Mixin(targets="com.logisticscraft.occlusionculling.util.Vec3d",remap=false)
public interface OcclusionVecAccess {
    @Accessor("x") double wachiland$x();
    @Accessor("y") double wachiland$y();
    @Accessor("z") double wachiland$z();
}
