package net.wachiland.atlasbatch;

import com.mojang.blaze3d.platform.NativeImage;
import com.mojang.blaze3d.systems.RenderSystem;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.Map;
import net.minecraft.client.Minecraft;
import net.minecraft.client.renderer.texture.DynamicTexture;
import net.minecraft.client.renderer.texture.TextureAtlas;
import net.wachiland.atlasbatch.mixin.NativeImageAccess;
import net.wachiland.atlasbatch.mixin.TextureManagerAccess;
import net.neoforged.fml.loading.FMLPaths;

/** Explicit bounded native resource census. No GC, pixel reads, GL calls or resource mutation. */
final class ResourceMemoryCensus {
    private record Row(String owner, long bytes, int width, int height) {}
    static String dump() {
        if (!AndroidSupportConfig.ANDROID || !RenderSystem.isOnRenderThread()) return "Resource census requires Android render thread.";
        Minecraft mc=Minecraft.getInstance();
        if (mc.getOverlay()!=null) return "Wait until resource reload has finished.";
        if (!(mc.getTextureManager() instanceof TextureManagerAccess access)) return "Resource census accessor unavailable.";
        long start=System.nanoTime();
        HashSet<Long> addresses=new HashSet<>();
        ArrayList<Row> largest=new ArrayList<>();
        Map<String,Long> owners=new LinkedHashMap<>();
        Map<String,Integer> textureClasses=new LinkedHashMap<>();
        long total=0; int examined=0, closed=0; boolean truncated=false;
        try {
            outer: for (var entry: access.wachiland$textures().entrySet()) {
                if (++examined>100_000) { truncated=true; break; }
                String kind=entry.getValue().getClass().getName();
                if (textureClasses.containsKey(kind) || textureClasses.size()<256) textureClasses.merge(kind,1,Integer::sum);
                if (entry.getValue() instanceof TextureAtlas atlas) {
                    for (var sprite:atlas.getTextures().values()) {
                        var contents=sprite.contents();
                        for (int mip=0;mip<contents.byMipLevel.length;mip++) {
                            if (++examined>100_000 || addresses.size()>=65_536) { truncated=true; break outer; }
                            NativeImage image=contents.byMipLevel[mip];
                            if (!((Object)image instanceof NativeImageAccess nativeAccess)) continue;
                            long address=nativeAccess.wachiland$address();
                            if (address==0) { closed++; continue; }
                            if (!addresses.add(address)) continue;
                            long bytes=nativeAccess.wachiland$bytes(); total+=bytes;
                            String category="atlas:"+atlas.location()+":mip"+(mip==0?"0":"higher");
                            if (owners.containsKey(category) || owners.size()<256) owners.merge(category,bytes,Long::sum);
                            retainLargest(largest,new Row("sprite:"+contents.name()+":mip"+mip,bytes,image.getWidth(),image.getHeight()));
                        }
                    }
                } else if (entry.getValue() instanceof DynamicTexture dynamic) {
                    NativeImage image=dynamic.getPixels();
                    if (!((Object)image instanceof NativeImageAccess nativeAccess)) continue;
                    long address=nativeAccess.wachiland$address();
                    if (address==0) { closed++; continue; }
                    if (addresses.size()>=65_536) { truncated=true; break; }
                    if (!addresses.add(address)) continue;
                    long bytes=nativeAccess.wachiland$bytes(); total+=bytes;
                    String category="dynamic:"+entry.getKey().getNamespace();
                    if (owners.containsKey(category) || owners.size()<256) owners.merge(category,bytes,Long::sum);
                    retainLargest(largest,new Row("dynamic:"+entry.getKey(),bytes,image.getWidth(),image.getHeight()));
                }
            }
            largest.sort(Comparator.comparingLong(Row::bytes).reversed());
            StringBuilder text=new StringBuilder(16_384);
            text.append("schema=1\nepoch_ms=").append(System.currentTimeMillis())
                .append("\nunique_live_native_image_bytes=").append(total)
                .append("\nunique_addresses=").append(addresses.size())
                .append("\nexamined_entries=").append(examined).append("\nclosed_images=").append(closed)
                .append("\ntruncated=").append(truncated)
                .append("\nscan_ns=").append(System.nanoTime()-start)
                .append("\nScope: registered atlas mip images and DynamicTexture pixels only. Lower bound, not total native/RSS or GPU allocation.\n");
            for (var e:owners.entrySet()) text.append("owner_bytes\t").append(e.getKey()).append('\t').append(e.getValue()).append('\n');
            for (Row row:largest) text.append("largest\t").append(row.owner()).append('\t').append(row.bytes()).append('\t').append(row.width()).append('x').append(row.height()).append('\n');
            for (var e:textureClasses.entrySet()) text.append("texture_class\t").append(e.getKey()).append('\t').append(e.getValue()).append('\n');
            Files.writeString(FMLPaths.GAMEDIR.get().resolve("wachiland-resource-memory.txt"),text,StandardCharsets.UTF_8);
            return "Resource census saved: "+total+" bytes in "+addresses.size()+" registered native images; truncated="+truncated+". No GC requested.";
        } catch (Exception | LinkageError e) { return "Resource census unavailable: "+e.getClass().getSimpleName(); }
    }
    private static void retainLargest(ArrayList<Row> rows, Row row) {
        if (rows.size()<40) { rows.add(row); return; }
        int smallest=0;
        for (int i=1;i<rows.size();i++) if (rows.get(i).bytes()<rows.get(smallest).bytes()) smallest=i;
        if (row.bytes()>rows.get(smallest).bytes()) rows.set(smallest,row);
    }
}
