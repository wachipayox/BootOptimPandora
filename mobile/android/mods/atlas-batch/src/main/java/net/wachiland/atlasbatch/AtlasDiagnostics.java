package net.wachiland.atlasbatch;

import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.Map;
import net.minecraft.client.Minecraft;
import net.minecraft.client.renderer.texture.SpriteContents;
import net.minecraft.client.renderer.texture.TextureAtlas;

/** Bounded experimental attribution; never owns images or changes GL state. */
final class AtlasDiagnostics {
    private static final long WINDOW_NS = 180_000_000_000L;
    private static final int MAX_KEYS = 2048;
    private static final HashSet<Key> SEEN = new HashSet<>();
    private static final Map<String, Long> REASONS = new LinkedHashMap<>();
    private static long started, frames, uploads, discrete, interpolated, repeats, overflow, visits;
    private static boolean active;
    private static Thread owner;
    private static String settings = "pending";
    private record Key(SpriteContents sprite, TextureAtlas atlas, int x, int y) {}

    static void begin(Minecraft mc, boolean renderLevel) {
        SEEN.clear();
        owner=Thread.currentThread();
        boolean world=AndroidSupportConfig.observe() && renderLevel && mc.level!=null && mc.screen==null && mc.getOverlay()==null;
        long now=System.nanoTime();
        if (world && started==0) { started=now; settings=readSettings(); }
        active=world && started!=0 && now-started<WINDOW_NS && !RenderAheadProbe.isRequested();
        if (active) frames++;
    }
    static void visit() { if (active && Thread.currentThread()==owner) visits++; }
    static void upload(SpriteContents sprite, TextureAtlas atlas, int x, int y, boolean originalImages) {
        if (!active || Thread.currentThread()!=owner) return;
        uploads++;
        if (originalImages) discrete++; else interpolated++;
        Key key=new Key(sprite,atlas,x,y);
        if (SEEN.contains(key)) repeats++;
        else if (SEEN.size()<MAX_KEYS) SEEN.add(key); else overflow++;
    }
    static void reason(String reason) {
        if (active && Thread.currentThread()==owner) REASONS.merge(reason,1L,Long::sum);
    }
    static String summary() {
        return "window_active="+active+" observed_frames="+frames+" atlas_visits="+visits+
            " uploads="+uploads+" discrete="+discrete+" interpolated="+interpolated+
            " repeat_keys="+repeats+" observation_overflow="+overflow+" reasons="+REASONS+" settings="+settings;
    }
    private static String readSettings() {
        String sodium="unavailable", async="unavailable";
        try {
            Object options=Class.forName("net.caffeinemc.mods.sodium.client.SodiumClientMod").getMethod("options").invoke(null);
            Object performance=options.getClass().getField("performance").get(options);
            sodium=String.valueOf(performance.getClass().getField("animateOnlyVisibleTextures").get(performance));
        } catch (ReflectiveOperationException | LinkageError ignored) { }
        for (String prefix : new String[]{"neoforge.",""}) {
            try {
                Class<?> helper=Class.forName(prefix+"fun.qu_an.minecraft.asyncparticles.client.config.ConfigHelper");
                async="tick_async="+helper.getMethod("isTickAsync").invoke(null)+", deferred="+helper.getMethod("isDeferredTextureTick").invoke(null);
                break;
            } catch (ReflectiveOperationException | LinkageError ignored) { }
        }
        return "sodium_visible_only="+sodium+", asyncparticles={"+async+"}";
    }
}
