package net.wachiland.atlasbatch.mixin;
import java.util.Map;
import net.minecraft.client.resources.model.ModelBakery;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.gen.Accessor;
@Mixin(ModelBakery.class)
public interface ModelBakeryAccess {
    @Accessor("modelResources") Map<?, ?> wachiland$modelResources();
    @Accessor("unbakedCache") Map<?, ?> wachiland$unbakedCache();
    @Accessor("bakedCache") Map<?, ?> wachiland$bakedCache();
    @Accessor("topLevelModels") Map<?, ?> wachiland$topLevelModels();
}
