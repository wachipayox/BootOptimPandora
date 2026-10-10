package net.wachiland.atlasbatch.mixin;

import com.llamalad7.mixinextras.injector.wrapoperation.Operation;
import com.llamalad7.mixinextras.injector.wrapoperation.WrapOperation;
import com.mojang.blaze3d.systems.RenderSystem;
import net.wachiland.atlasbatch.AndroidFrameWait;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;

@Mixin(RenderSystem.class)
abstract class RenderSystemWaitMixin {
    @WrapOperation(method="limitDisplayFPS",at=@At(value="INVOKE",target="Lorg/lwjgl/glfw/GLFW;glfwWaitEventsTimeout(D)V"))
    private static void wachiland$wait(double timeout,Operation<Void> original) {
        AndroidFrameWait.waitFor(timeout);
        original.call(timeout);
    }
}
