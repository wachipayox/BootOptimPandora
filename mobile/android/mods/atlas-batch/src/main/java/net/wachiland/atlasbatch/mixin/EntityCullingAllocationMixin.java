package net.wachiland.atlasbatch.mixin;

import java.util.Collections;
import java.util.HashMap;
import java.util.Iterator;
import java.util.Map;
import java.util.Set;
import net.wachiland.atlasbatch.EntityCullingAllocationPolicy;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.Unique;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Redirect;

@Pseudo
@Mixin(targets="dev.tr7zw.entityculling.EntityCullingModBase",remap=false)
abstract class EntityCullingAllocationMixin {
    // Updated after the source publishes its fresh defensive snapshot. Never retain that map.
    @Unique private int wachiland$lastSnapshotSize;

    @Redirect(method="isDynamicWhitelisted",at=@At(value="INVOKE",target="Ljava/util/Set;iterator()Ljava/util/Iterator;"),require=0)
    private Iterator<?> wachiland$emptyWhitelist(Set<?> set) {
        // Nonempty callbacks preserve the original iterator and its order/semantics.
        return EntityCullingAllocationPolicy.active() && set.isEmpty()
            ? Collections.emptyIterator() : set.iterator();
    }

    @Redirect(method="clientTick",at=@At(value="NEW",target="java/util/HashMap"),require=0)
    private HashMap<?,?> wachiland$freshSnapshot() {
        if (!EntityCullingAllocationPolicy.active()) return new HashMap<>();
        int count=Math.min(65536,Math.max(0,wachiland$lastSnapshotSize));
        // Retain the stock minimum table size. Prediction cannot affect Map membership.
        return new HashMap<>(Math.max(16,(count*4+2)/3));
    }

    @Redirect(method="clientTick",at=@At(value="INVOKE",target="Ljava/util/Map;size()I"),require=0)
    private int wachiland$recordSize(Map<?,?> map) {
        int count=map.size();
        wachiland$lastSnapshotSize=Math.min(65536,count);
        return count;
    }
}
