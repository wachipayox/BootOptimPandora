package net.wachiland.atlasbatch;

import java.util.List;
import java.util.Set;
import java.nio.file.Files;
import java.util.Properties;
import net.neoforged.fml.loading.FMLPaths;
import org.objectweb.asm.tree.ClassNode;
import org.spongepowered.asm.mixin.extensibility.IMixinConfigPlugin;
import org.spongepowered.asm.mixin.extensibility.IMixinInfo;

/** An optional failed injection must never leave only part of the batching guards active. */
public final class AtlasBatchMixinPlugin implements IMixinConfigPlugin {
    private static volatile int applied;
    private static volatile boolean rainHookInstalled;
    private static final List<String> REQUIRED = List.of(
        "MinecraftMixin", "GameRendererMixin", "TextureAtlasMixin", "SpriteContentsMixin",
        "SpriteUploadInvoker", "GlStateManagerMixin", "NativeImageMixin");

    public static boolean ready() { return applied == (1 << REQUIRED.size()) - 1; }
    public static boolean rainHookInstalled() { return rainHookInstalled; }

    private static boolean requestRainHook() {
        // Do not leave a WrapMethod/Object[] dispatch on this very hot path when
        // the experiment is off. Read without loading Minecraft or writing config.
        try {
            Properties p=new Properties();
            try (var reader=Files.newBufferedReader(FMLPaths.CONFIGDIR.get().resolve("wachiland-android-support.properties"))) { p.load(reader); }
            if (!"true".equalsIgnoreCase(p.getProperty("enabled","true").trim())) return false;
            return "true".equalsIgnoreCase(p.getProperty("experimental.rainBiomeMemo","false").trim())
                || ("true".equalsIgnoreCase(p.getProperty("diagnostics","false").trim())
                    && Files.isRegularFile(FMLPaths.GAMEDIR.get().resolve("wachiland-rain-biome-probe.txt")));
        } catch (Exception e) { return false; }
    }

    @Override public void onLoad(String mixinPackage) { }
    @Override public String getRefMapperConfig() { return null; }
    @Override public boolean shouldApplyMixin(String targetClassName, String mixinClassName) {
        return AndroidSupportConfig.ANDROID
            && (!mixinClassName.endsWith(".ParticleRainBiomeMixin") || requestRainHook());
    }
    @Override public void acceptTargets(Set<String> myTargets, Set<String> otherTargets) { }
    @Override public List<String> getMixins() { return null; }
    @Override public void preApply(String targetClassName, ClassNode targetClass, String mixinClassName, IMixinInfo mixinInfo) { }
    @Override public synchronized void postApply(String targetClassName, ClassNode targetClass, String mixinClassName, IMixinInfo mixinInfo) {
        String name = mixinClassName.substring(mixinClassName.lastIndexOf('.') + 1);
        if (name.equals("ParticleRainBiomeMixin")) rainHookInstalled=true;
        int index = REQUIRED.indexOf(name);
        if (index >= 0) applied |= 1 << index;
    }
}
