package net.wachiland.atlasbatch.mixin;
import java.util.List;
import java.util.Map;
import net.minecraft.client.resources.model.SimpleBakedModel;
import net.minecraft.client.renderer.block.model.BakedQuad;
import net.minecraft.core.Direction;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.gen.Accessor;
@Mixin(SimpleBakedModel.class)
public interface SimpleModelAccess {
    @Accessor("unculledFaces") List<BakedQuad> wachiland$unculled();
    @Accessor("culledFaces") Map<Direction, List<BakedQuad>> wachiland$culled();
}
