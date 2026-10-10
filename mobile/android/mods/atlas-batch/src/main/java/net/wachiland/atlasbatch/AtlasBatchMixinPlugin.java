package net.wachiland.atlasbatch;

import java.util.List;
import java.util.Set;
import org.objectweb.asm.tree.ClassNode;
import org.spongepowered.asm.mixin.extensibility.IMixinConfigPlugin;
import org.spongepowered.asm.mixin.extensibility.IMixinInfo;

/** An optional failed injection must never leave only part of the batching guards active. */
public final class AtlasBatchMixinPlugin implements IMixinConfigPlugin {
    private static volatile int applied;
    private static final List<String> REQUIRED = List.of(
        "MinecraftMixin", "GameRendererMixin", "TextureAtlasMixin", "SpriteContentsMixin",
        "SpriteUploadInvoker", "GlStateManagerMixin", "NativeImageMixin");

    public static boolean ready() { return applied == (1 << REQUIRED.size()) - 1; }

    @Override public void onLoad(String mixinPackage) { }
    @Override public String getRefMapperConfig() { return null; }
    @Override public boolean shouldApplyMixin(String targetClassName, String mixinClassName) { return AndroidSupportConfig.ANDROID; }
    @Override public void acceptTargets(Set<String> myTargets, Set<String> otherTargets) { }
    @Override public List<String> getMixins() { return null; }
    @Override public void preApply(String targetClassName, ClassNode targetClass, String mixinClassName, IMixinInfo mixinInfo) { }
    @Override public synchronized void postApply(String targetClassName, ClassNode targetClass, String mixinClassName, IMixinInfo mixinInfo) {
        String name = mixinClassName.substring(mixinClassName.lastIndexOf('.') + 1);
        int index = REQUIRED.indexOf(name);
        if (index >= 0) applied |= 1 << index;
    }
}
