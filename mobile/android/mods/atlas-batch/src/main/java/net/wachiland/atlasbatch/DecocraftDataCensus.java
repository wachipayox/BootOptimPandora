package net.wachiland.atlasbatch;

import com.mojang.blaze3d.systems.RenderSystem;
import java.lang.reflect.Field;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.IdentityHashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import net.minecraft.client.Minecraft;
import net.neoforged.fml.ModList;
import net.neoforged.fml.loading.FMLPaths;

/** Pinned stored fields from Decocraft 3.0.11. Counts only, never removes resources. */
final class DecocraftDataCensus {
    private final Map<String, Field> fields=new LinkedHashMap<>();
    private final IdentityHashMap<Object, Boolean> models=new IdentityHashMap<>(), strings=new IdentityHashMap<>();
    private long sourceChars, elements, animations, animators, keyframes, imageEntries;
    private int errors, textures, examined;
    private boolean truncated;
    private final long deadline=System.nanoTime()+500_000_000L;
    static String dump() {
        if(!AndroidSupportConfig.ANDROID || !RenderSystem.isOnRenderThread() || Minecraft.getInstance().getOverlay()!=null)
            return "Wait for Android resource reload to finish.";
        if(!ModList.get().isLoaded("decocraft")) return "Decocraft not loaded.";
        long start=System.nanoTime();
        try {
            DecocraftDataCensus scan=new DecocraftDataCensus();
            Class<?> module=Class.forName("com.razz.decocraft.common.ModuleBlocks",false,DecocraftDataCensus.class.getClassLoader());
            Object root=module.getField("MAT_TO_BB_MODEL").get(null);
            if(!(root instanceof Map<?,?> map)) return "Decocraft stored model map unavailable.";
            for(Object model:map.values()) {
                if(scan.limited()) break;
                scan.examined++;
                if(model==null || !model.getClass().getName().equals("com.razz.decocraft.models.bbmodel.BBModel") || scan.models.put(model,Boolean.TRUE)!=null) continue;
                scan.model(model);
            }
            String text="schema=1\nepoch_ms="+System.currentTimeMillis()+"\nmap_entries="+map.size()+"\nunique_bbmodels="+scan.models.size()
                +"\ntexture_source_unique_chars="+scan.sourceChars+"\ntexture_entries="+scan.textures+"\ntexture_image_entries="+scan.imageEntries
                +"\nelement_references="+scan.elements+"\nanimation_references="+scan.animations+"\nanimator_references="+scan.animators+"\nkeyframe_references="+scan.keyframes
                +"\nfield_failures="+scan.errors+"\ntruncated="+scan.truncated+"\nscan_ns="+(System.nanoTime()-start)
                +"\nScope: stored Decocraft ModuleBlocks BBModel data, unique models/source Strings by identity, other counts are references. Chars are not heap bytes. No transitive retained-size measurement, mod callbacks, raster pixel reads, full GC or resource mutation. Caps 4096 models/Strings, 100000 visited entries and 500 ms.\n";
            Files.writeString(FMLPaths.GAMEDIR.get().resolve("wachiland-decocraft-memory.txt"),text,StandardCharsets.UTF_8);
            return "Decocraft data census saved; models="+scan.models.size()+"; truncated="+scan.truncated+".";
        } catch(Exception | LinkageError e) { return "Decocraft census unavailable: "+e.getClass().getSimpleName(); }
    }
    private boolean limited() {
        if(models.size()>=4096 || strings.size()>=4096 || examined>=100000 || System.nanoTime()>deadline) truncated=true;
        return truncated;
    }
    private Object field(Object target,String name) {
        try {
            String key=target.getClass().getName()+":"+name;
            Field f=fields.get(key);
            if(f==null) { f=target.getClass().getField(name); fields.put(key,f); }
            return f.get(target);
        } catch(ReflectiveOperationException | RuntimeException e) { errors++; return null; }
    }
    private long size(Object value) { return value instanceof List<?> list ? list.size() : value instanceof Map<?,?> map ? map.size() : 0; }
    private void model(Object model) {
        elements+=size(field(model,"elements"));
        imageEntries+=size(field(model,"textureImages"));
        Object tex=field(model,"textures");
        if(tex instanceof List<?> list) for(Object texture:list) {
            if(limited()) return;
            examined++; textures++;
            if(texture==null || !texture.getClass().getName().equals("com.razz.decocraft.models.bbmodel.BBModelParts$Texture")) continue;
            Object source=field(texture,"source");
            if(source instanceof String str && strings.put(str,Boolean.TRUE)==null) sourceChars+=str.length();
        }
        Object ani=field(model,"animations");
        animations+=size(ani);
        if(ani instanceof List<?> list) for(Object animation:list) {
            if(limited()) return;
            examined++;
            if(animation==null || !animation.getClass().getName().equals("com.razz.decocraft.models.bbmodel.BBModelParts$Animation")) continue;
            Object values=field(animation,"animators");
            animators+=size(values);
            if(values instanceof Map<?,?> map) for(Object animator:map.values()) {
                if(limited()) return;
                examined++;
                if(animator!=null && animator.getClass().getName().equals("com.razz.decocraft.models.bbmodel.BBModelParts$Animator")) keyframes+=size(field(animator,"keyframes"));
            }
        }
    }
}
