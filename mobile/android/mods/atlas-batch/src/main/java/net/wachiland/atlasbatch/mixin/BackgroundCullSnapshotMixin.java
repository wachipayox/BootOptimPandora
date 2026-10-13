package net.wachiland.atlasbatch.mixin;

import net.wachiland.atlasbatch.BackgroundMode;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

/** Keep late registration/key handling, omit only invisible snapshot preparation. */
@Mixin(targets="dev.tr7zw.entityculling.EntityCullingModBase",remap=false)
abstract class BackgroundCullSnapshotMixin {
    @Inject(method="clientTick",at=@At(value="INVOKE",target="Ljava/lang/System;nanoTime()J",ordinal=0),cancellable=true)
    private void wachiland$invisibleSnapshots(CallbackInfo callback) {
        if (BackgroundMode.hidden()) callback.cancel();
    }
}
