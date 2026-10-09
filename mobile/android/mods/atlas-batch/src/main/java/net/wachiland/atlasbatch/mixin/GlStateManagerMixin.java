package net.wachiland.atlasbatch.mixin;
import com.mojang.blaze3d.platform.GlStateManager;
import net.wachiland.atlasbatch.AtlasBatch;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;
@Mixin(GlStateManager.class)
abstract class GlStateManagerMixin {
    @Inject(method={"_drawElements","_glDrawPixels","_readPixels","_getTexImage","_texImage2D","_deleteTexture","_deleteTextures","_glCopyTexSubImage2D","_glBlitFrameBuffer"},at=@At("HEAD"))
    private static void wachiland$consume(CallbackInfo ci) { AtlasBatch.beforeConsume(); }
    @Inject(method={"_texSubImage2D","_upload"},at=@At("HEAD"))
    private static void wachiland$upload(CallbackInfo ci) { AtlasBatch.beforeUpload(); }
    @Inject(method="_pixelStore",at=@At("RETURN"))
    private static void wachiland$unpack(int name,int value,CallbackInfo ci) { AtlasBatch.unpack(name,value); }
}
