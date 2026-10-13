package net.wachiland.atlasbatch.mixin;

import com.llamalad7.mixinextras.injector.ModifyExpressionValue;
import net.wachiland.atlasbatch.BackgroundMode;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;

/** Pause invisible occlusion work without changing the mod's global toggle. */
@Mixin(targets="dev.tr7zw.entityculling.CullTask",remap=false)
abstract class BackgroundCullTaskMixin {
    @ModifyExpressionValue(method="run",at=@At(value="FIELD",
        target="Ldev/tr7zw/entityculling/versionless/EntityCullingVersionlessBase;enabled:Z",opcode=178))
    private boolean wachiland$visibleCulling(boolean enabled) {
        return enabled && !BackgroundMode.hidden();
    }
}
