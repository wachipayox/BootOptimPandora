package net.wachiland.atlasbatch.mixin;
import com.mojang.blaze3d.platform.NativeImage;
import net.wachiland.atlasbatch.AtlasBatch;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;
@Mixin(NativeImage.class)
abstract class NativeImageMixin {
    @Inject(method="close",at=@At("HEAD"))
    private void wachiland$close(CallbackInfo ci) { AtlasBatch.lifecycle(); }
}
