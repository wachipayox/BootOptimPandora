package net.wachiland.atlasbatch.mixin;
import com.llamalad7.mixinextras.injector.wrapmethod.WrapMethod;
import com.llamalad7.mixinextras.injector.wrapoperation.Operation;
import net.minecraft.client.renderer.texture.TextureAtlas;
import net.wachiland.atlasbatch.AtlasBatch;
import net.wachiland.atlasbatch.StagedAtlasUpload;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;
@Mixin(TextureAtlas.class)
abstract class TextureAtlasMixin {
    @WrapMethod(method="cycleAnimationFrames")
    private void wachiland$atlas(Operation<Void> original) {
        TextureAtlas observed=AtlasBatch.observeAtlas((TextureAtlas)(Object)this);
        TextureAtlas previous=AtlasBatch.enterAtlas((TextureAtlas)(Object)this);
        try { original.call(); } finally { AtlasBatch.leaveAtlas(previous); AtlasBatch.leaveObservedAtlas(observed); }
    }
    @Inject(method="clearTextureData",at=@At("HEAD"))
    private void wachiland$clear(CallbackInfo ci) { AtlasBatch.lifecycle(); StagedAtlasUpload.invalidatePixels(); }
}
