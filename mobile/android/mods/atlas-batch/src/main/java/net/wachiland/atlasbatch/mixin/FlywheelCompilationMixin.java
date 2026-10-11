package net.wachiland.atlasbatch.mixin;

import net.wachiland.atlasbatch.FlywheelExplicitOutputs;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.ModifyArg;

@Pseudo
@Mixin(targets="dev.engine_room.flywheel.backend.compile.core.Compilation",remap=false)
abstract class FlywheelCompilationMixin {
    @ModifyArg(method="compile",at=@At(value="INVOKE",target="Ldev/engine_room/flywheel/backend/gl/GlCompat;safeShaderSource(ILjava/lang/CharSequence;)V"),index=1,require=0,remap=false)
    private CharSequence wachiland$explicitOutputs(CharSequence source) {
        return FlywheelExplicitOutputs.rewrite(source);
    }
}
