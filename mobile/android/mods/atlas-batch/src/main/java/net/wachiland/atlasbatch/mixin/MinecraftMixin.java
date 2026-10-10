package net.wachiland.atlasbatch.mixin;
import com.llamalad7.mixinextras.injector.wrapmethod.WrapMethod;
import com.llamalad7.mixinextras.injector.wrapoperation.Operation;
import com.llamalad7.mixinextras.injector.ModifyExpressionValue;
import net.minecraft.client.Minecraft;
import net.wachiland.atlasbatch.AtlasBatch;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;
@Mixin(Minecraft.class)
abstract class MinecraftMixin {
    @WrapMethod(method="runTick")
    private void wachiland$frame(boolean renderLevel, Operation<Void> original) {
        AtlasBatch.begin((Minecraft)(Object)this,renderLevel);
        try { original.call(renderLevel); } finally { try { AtlasBatch.end(); } finally { AtlasBatch.endFrameProbe(); } }
    }
    @Inject(method="runTick", at=@At(value="INVOKE",target="Lcom/mojang/blaze3d/systems/RenderSystem;clear(IZ)V"))
    private void wachiland$beforeRender(boolean renderLevel, CallbackInfo ci) { AtlasBatch.end(); }
    @ModifyExpressionValue(method="runTick",at=@At(value="INVOKE",target="Lnet/minecraft/client/DeltaTracker$Timer;advanceTime(JZ)I"))
    private int wachiland$ticks(int ticks) { AtlasBatch.tickCount(ticks); return ticks; }
}
