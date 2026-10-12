package net.wachiland.atlasbatch.mixin;
import net.minecraft.client.multiplayer.ClientLevel;
import net.minecraft.client.renderer.LevelRenderer;
import net.minecraft.client.renderer.culling.Frustum;
import net.minecraft.world.phys.Vec3;
import net.wachiland.atlasbatch.CullingFrustumGuard;
import org.joml.Matrix4f;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Shadow;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;
@Mixin(LevelRenderer.class)
abstract class LevelFrustumCaptureMixin {
    @Shadow private Frustum cullingFrustum;
    @Inject(method="prepareCullFrustum",at=@At("TAIL"),require=0)
    private void wachiland$capture(Vec3 camera,Matrix4f view,Matrix4f projection,CallbackInfo ci) {
        CullingFrustumGuard.capture(cullingFrustum,camera);
    }
    @Inject(method="setLevel",at=@At("HEAD"),require=0)
    private void wachiland$clear(ClientLevel level,CallbackInfo ci) { CullingFrustumGuard.clear(); }
}
