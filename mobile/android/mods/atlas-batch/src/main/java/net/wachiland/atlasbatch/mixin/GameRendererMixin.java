package net.wachiland.atlasbatch.mixin;
import net.minecraft.client.DeltaTracker;
import net.minecraft.client.renderer.GameRenderer;
import net.wachiland.atlasbatch.AtlasBatch;
import net.wachiland.atlasbatch.BackgroundMode;
import net.minecraft.client.Minecraft;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;
@Mixin(GameRenderer.class)
abstract class GameRendererMixin {
    @Inject(method="render",at=@At("HEAD"),cancellable=true)
    private void wachiland$beforeRender(DeltaTracker tracker, boolean renderLevel, CallbackInfo ci) {
        AtlasBatch.end();
        Minecraft mc=Minecraft.getInstance();
        if(BackgroundMode.hidden()&&mc.isGameLoadFinished()&&mc.getOverlay()==null)ci.cancel();
    }
}
