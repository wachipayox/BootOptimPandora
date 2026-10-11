package net.wachiland.atlasbatch;

import com.mojang.blaze3d.systems.RenderSystem;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.IdentityHashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import net.minecraft.client.Minecraft;
import net.minecraft.client.renderer.block.model.BakedQuad;
import net.wachiland.atlasbatch.mixin.ModelBakeryAccess;
import net.wachiland.atlasbatch.mixin.SimpleModelAccess;
import net.neoforged.fml.loading.FMLPaths;

/** Direct stored-field census only; never invokes custom getQuads, GC or cache mutation. */
final class ModelMemoryCensus {
    private final IdentityHashMap<Object, Boolean> models = new IdentityHashMap<>();
    private final IdentityHashMap<int[], Boolean> arrays = new IdentityHashMap<>();
    private final Map<String, Integer> classes = new LinkedHashMap<>();
    private final long deadline = System.nanoTime() + 500_000_000L;
    private long payload;
    private int quadReferences, simpleModels, examined;
    private boolean truncated;
    static String dump() {
        Minecraft mc = Minecraft.getInstance();
        if (!AndroidSupportConfig.ANDROID || !RenderSystem.isOnRenderThread() || mc.getOverlay()!=null)
            return "Wait for Android resource reload to finish.";
        long start=System.nanoTime();
        try {
            var bakery=mc.getModelManager().getModelBakery();
            if (!(bakery instanceof ModelBakeryAccess access)) return "Model census accessor unavailable.";
            ModelMemoryCensus scan=new ModelMemoryCensus();
            StringBuilder out=new StringBuilder(8192);
            out.append("schema=1\nepoch_ms=").append(System.currentTimeMillis()).append('\n');
            out.append("model_resources_entries=").append(access.wachiland$modelResources().size()).append('\n');
            out.append("unbaked_cache_entries=").append(access.wachiland$unbakedCache().size()).append('\n');
            out.append("top_level_unbaked_entries=").append(access.wachiland$topLevelModels().size()).append('\n');
            out.append("baked_cache_entries=").append(access.wachiland$bakedCache().size()).append('\n');
            out.append("top_level_baked_entries=").append(bakery.getBakedTopLevelModels().size()).append('\n');
            scan.models(bakery.getBakedTopLevelModels());
            if (!scan.truncated) scan.models(access.wachiland$bakedCache());
            out.append("unique_baked_models_examined=").append(scan.models.size()).append('\n');
            out.append("simple_models_examined=").append(scan.simpleModels).append('\n');
            out.append("quad_references_examined=").append(scan.quadReferences).append('\n');
            out.append("unique_vertex_arrays=").append(scan.arrays.size()).append('\n');
            out.append("vertex_array_payload_bytes=").append(scan.payload).append('\n');
            out.append("truncated=").append(scan.truncated).append("\nscan_ns=").append(System.nanoTime()-start).append('\n');
            out.append("Scope: direct top-level/cache model values and stored SimpleBakedModel face lists only. Arrays deduplicated by identity; payload excludes object headers. Custom/wrapped models and unbaked object graphs are not traversed. Not total/live/retained heap. Caps: 50000 models, 100000 arrays, 200000 entries/quad references, 500 ms traversal. No custom model methods or full GC.\n");
            for(var e:scan.classes.entrySet()) out.append("model_class\t").append(e.getKey()).append('\t').append(e.getValue()).append('\n');
            Files.writeString(FMLPaths.GAMEDIR.get().resolve("wachiland-model-memory.txt"),out,StandardCharsets.UTF_8);
            return "Model census saved: vertex payload="+scan.payload+" bytes; truncated="+scan.truncated+".";
        } catch (Exception | LinkageError e) { return "Model census unavailable: "+e.getClass().getSimpleName(); }
    }
    private boolean limited() {
        if (models.size()>=50_000 || arrays.size()>=100_000 || examined>=200_000 || System.nanoTime()>deadline) truncated=true;
        return truncated;
    }
    private void models(Map<?, ?> map) {
        for(Object value:map.values()) {
            if(limited()) return;
            examined++;
            if(value==null || models.put(value,Boolean.TRUE)!=null) continue;
            String kind=value.getClass().getName();
            if(classes.containsKey(kind) || classes.size()<256) classes.merge(kind,1,Integer::sum);
            if(value instanceof SimpleModelAccess faces) {
                simpleModels++;
                quads(faces.wachiland$unculled());
                for(var list:faces.wachiland$culled().values()) { if(limited()) return; quads(list); }
            }
        }
    }
    private void quads(List<BakedQuad> list) {
        if(list==null) return;
        for(BakedQuad quad:list) {
            if(limited()) return;
            examined++; quadReferences++;
            // Only stock exact quads: custom overrides could allocate or mutate in getVertices.
            if(quad==null || quad.getClass()!=BakedQuad.class) continue;
            int[] vertices=quad.getVertices();
            if(vertices!=null && arrays.put(vertices,Boolean.TRUE)==null) payload+=(long)vertices.length*Integer.BYTES;
        }
    }
}
