package net.wachiland.atlasbatch.mixin;
import com.mojang.blaze3d.platform.NativeImage;
import net.minecraft.client.renderer.texture.SpriteContents;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.gen.Invoker;
@Mixin(SpriteContents.class)
public interface SpriteUploadInvoker {
    @Invoker("upload") void wachiland$upload(int x,int y,int frameX,int frameY,NativeImage[] images);
}
