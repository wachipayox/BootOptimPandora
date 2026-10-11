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
    private final long deadline = System.nanoTime() + 1_500_000_000L;
    private long payload;
    private int quadReferences, simpleModels, examined;
    private boolean truncated, verticesTruncated;
    private int customModels, fieldFailures;
    private final Map<String, java.lang.reflect.Field> fields = new LinkedHashMap<>();
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
            scan.unbaked(access.wachiland$modelResources(), out, "resources");
            scan.unbaked(access.wachiland$unbakedCache(), out, "unbaked");
            scan.unbaked(access.wachiland$topLevelModels(), out, "top_level");
            out.append("unique_baked_models_examined=").append(scan.models.size()).append('\n');
            out.append("custom_models_examined=").append(scan.customModels).append("\nfield_failures=").append(scan.fieldFailures).append("\nvertex_scan_truncated=").append(scan.verticesTruncated).append("\n");
            out.append("simple_models_examined=").append(scan.simpleModels).append('\n');
            out.append("quad_references_examined=").append(scan.quadReferences).append('\n');
            out.append("unique_vertex_arrays=").append(scan.arrays.size()).append('\n');
            out.append("vertex_array_payload_bytes=").append(scan.payload).append('\n');
            out.append("truncated=").append(scan.truncated).append("\nscan_ns=").append(System.nanoTime()-start).append('\n');
            out.append("Scope: direct top-level/cache model values and stored SimpleBakedModel faces plus pinned Decocraft and BootOptim direct quad fields. Arrays deduplicated by identity; payload excludes object headers. Other custom/wrapped graphs are not traversed; unbaked class counts are reported separately. Not total/live/retained heap. Independent caps: 65536 unique models, 400000 arrays, 1000000 model entries, 2000000 quad references, 1500 ms traversal. No mod getQuads calls. No custom model methods or full GC.\n");
            for(var e:scan.classes.entrySet()) out.append("model_class\t").append(e.getKey()).append('\t').append(e.getValue()).append('\n');
            Files.writeString(FMLPaths.GAMEDIR.get().resolve("wachiland-model-memory.txt"),out,StandardCharsets.UTF_8);
            return "Model census saved: vertex payload="+scan.payload+" bytes; truncated="+scan.truncated+".";
        } catch (Exception | LinkageError e) { return "Model census unavailable: "+e.getClass().getSimpleName(); }
    }
    private boolean limited() {
        if (models.size()>=65_536 || examined>=1_000_000 || System.nanoTime()>deadline) truncated=true;
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
            } else if(kind.equals("com.razz.decocraft.models.bbmodel.BlockbenchModel")) {
                customModels++; storedQuads(value,"bakedQuads");
            } else if(kind.equals("dev.wachipayox.bootoptim.optimization.client.DecocraftRotatedQuadReuse$DerivedBakedModel")
                    || kind.equals("dev.wachipayox.bootoptim.optimization.client.DirectGeneratedItemBaker$DirectModel")) {
                customModels++; storedQuads(value,"quads");
            }
        }
    }
    @SuppressWarnings("unchecked")
    private void storedQuads(Object model, String name) {
        try {
            String key=model.getClass().getName()+":"+name;
            java.lang.reflect.Field field=fields.get(key);
            if(field==null) { field=model.getClass().getDeclaredField(name); field.setAccessible(true); fields.put(key,field); }
            Object value=field.get(model);
            if(value instanceof List<?> list) quads((List<BakedQuad>)list);
        } catch(ReflectiveOperationException | RuntimeException e) { fieldFailures++; }
    }
    private void unbaked(Map<?,?> map, StringBuilder out, String label) {
        IdentityHashMap<Object,Boolean> seen=new IdentityHashMap<>();
        Map<String,Integer> counts=new LinkedHashMap<>();
        int entries=0; boolean cut=false;
        for(Object value:map.values()) {
            if(entries++>=500_000 || seen.size()>=65_536 || System.nanoTime()>deadline) { cut=true; break; }
            if(value==null || seen.put(value,Boolean.TRUE)!=null) continue;
            String name=value.getClass().getName();
            if(counts.containsKey(name) || counts.size()<256) counts.merge(name,1,Integer::sum);
        }
        out.append("unbaked_scope\t").append(label).append("\tentries_examined=").append(entries).append("\tunique=").append(seen.size()).append("\ttruncated=").append(cut).append('\n');
        for(var e:counts.entrySet()) out.append("unbaked_class\t").append(label).append('\t').append(e.getKey()).append('\t').append(e.getValue()).append('\n');
    }
    private void quads(List<BakedQuad> list) {
        if(list==null) return;
        for(BakedQuad quad:list) {
            if(verticesTruncated) return;
            if(arrays.size()>=400_000 || quadReferences>=2_000_000 || System.nanoTime()>deadline) { verticesTruncated=true; return; }
            quadReferences++;
            // Only stock exact quads: custom overrides could allocate or mutate in getVertices.
            if(quad==null || quad.getClass()!=BakedQuad.class) continue;
            int[] vertices=quad.getVertices();
            if(vertices!=null && arrays.put(vertices,Boolean.TRUE)==null) payload+=(long)vertices.length*Integer.BYTES;
        }
    }
}
