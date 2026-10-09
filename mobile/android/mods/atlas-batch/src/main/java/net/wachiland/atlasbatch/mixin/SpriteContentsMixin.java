package net.wachiland.atlasbatch.mixin;
import com.mojang.blaze3d.platform.NativeImage;
import net.minecraft.client.renderer.texture.SpriteContents;
import net.wachiland.atlasbatch.AtlasBatch;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;
@Mixin(SpriteContents.class)
abstract class SpriteContentsMixin {
    @Inject(method="upload",at=@At("HEAD"),cancellable=true)
    private void wachiland$upload(int x,int y,int frameX,int frameY,NativeImage[] images,CallbackInfo ci) {
        if (AtlasBatch.defer((SpriteContents)(Object)this,x,y,frameX,frameY,images)) ci.cancel();
    }
    @Inject(method={"close","increaseMipLevel"},at=@At("HEAD"))
    private void wachiland$lifecycle(CallbackInfo ci) { AtlasBatch.lifecycle(); }
}
