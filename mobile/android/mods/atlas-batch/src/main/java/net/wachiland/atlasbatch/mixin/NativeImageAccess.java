package net.wachiland.atlasbatch.mixin;

import com.mojang.blaze3d.platform.NativeImage;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.gen.Accessor;

@Mixin(NativeImage.class)
public interface NativeImageAccess {
    @Accessor("pixels") long wachiland$address();
    @Accessor("size") long wachiland$bytes();
}
