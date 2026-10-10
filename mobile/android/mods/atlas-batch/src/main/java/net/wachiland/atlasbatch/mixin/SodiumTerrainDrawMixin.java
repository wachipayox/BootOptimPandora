package net.wachiland.atlasbatch.mixin;

import com.llamalad7.mixinextras.injector.wrapoperation.Operation;
import com.llamalad7.mixinextras.injector.wrapoperation.WrapOperation;
import net.wachiland.atlasbatch.DirectTerrainDraw;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.injection.At;

@Pseudo
@Mixin(targets="net.caffeinemc.mods.sodium.client.gl.device.GLRenderDevice$ImmediateDrawCommandList",remap=false)
abstract class SodiumTerrainDrawMixin {
    @WrapOperation(method="multiDrawElementsBaseVertex",at=@At(value="INVOKE",target="Lorg/lwjgl/opengl/GL32C;nglMultiDrawElementsBaseVertex(IJIJIJ)V"),remap=false)
    private void wachiland$direct(int mode,long counts,int type,long indices,int size,long bases,Operation<Void> original) {
        if (!DirectTerrainDraw.draw(mode,counts,type,indices,size,bases)) original.call(mode,counts,type,indices,size,bases);
    }
}
