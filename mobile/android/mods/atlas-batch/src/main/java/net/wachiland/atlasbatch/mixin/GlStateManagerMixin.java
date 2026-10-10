package net.wachiland.atlasbatch.mixin;
import com.llamalad7.mixinextras.injector.wrapmethod.WrapMethod;
import com.llamalad7.mixinextras.injector.wrapoperation.Operation;
import com.mojang.blaze3d.platform.GlStateManager;
import net.wachiland.atlasbatch.AtlasBatch;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;
@Mixin(GlStateManager.class)
abstract class GlStateManagerMixin {
    @WrapMethod(method="_texSubImage2D")
    private static void wachiland$measureUpload(int target,int level,int x,int y,int width,int height,int format,int type,long pointer,Operation<Void> original) {
        long started=AtlasBatch.beginUploadProbe();
        long census=AtlasBatch.beginCensusUpload();
        try { original.call(target,level,x,y,width,height,format,type,pointer); }
        finally { AtlasBatch.endUploadProbe(started,width,height); AtlasBatch.endCensusUpload(census,width,height,level); }
    }
    @Inject(method={"_drawElements","_glDrawPixels","_readPixels","_getTexImage","_texImage2D","_deleteTexture","_deleteTextures","_glCopyTexSubImage2D","_glBlitFrameBuffer"},at=@At("HEAD"))
    private static void wachiland$consume(CallbackInfo ci) { AtlasBatch.beforeConsume(); }
    @Inject(method={"_texSubImage2D","_upload"},at=@At("HEAD"))
    private static void wachiland$upload(CallbackInfo ci) { AtlasBatch.beforeUpload(); }
    @Inject(method="_pixelStore",at=@At("RETURN"))
    private static void wachiland$unpack(int name,int value,CallbackInfo ci) { AtlasBatch.unpack(name,value); }
}
