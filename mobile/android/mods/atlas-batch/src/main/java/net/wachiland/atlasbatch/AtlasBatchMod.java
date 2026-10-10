package net.wachiland.atlasbatch;

import com.mojang.brigadier.Command;
import net.minecraft.commands.Commands;
import net.minecraft.network.chat.Component;
import net.neoforged.api.distmarker.Dist;
import net.neoforged.fml.common.Mod;
import net.neoforged.neoforge.common.NeoForge;
import net.neoforged.neoforge.client.event.RegisterClientCommandsEvent;

@Mod(value = "wachiland_android_support", dist = Dist.CLIENT)
public final class AtlasBatchMod {
    public AtlasBatchMod() { AndroidSupportConfig.load(); NeoForge.EVENT_BUS.addListener(AtlasBatchMod::commands); }
    private static void commands(RegisterClientCommandsEvent event) {
        event.getDispatcher().register(Commands.literal("wachilandandroid")
            .then(Commands.literal("status").executes(c -> {
                c.getSource().sendSuccess(() -> Component.literal("Android="+AndroidSupportConfig.ANDROID+" enabled="+AndroidSupportConfig.enabled+" diagnostics="+AndroidSupportConfig.diagnostics+" "+AndroidGraphicsProfile.status()+" "+StagedAtlasUpload.status()),false);
                return Command.SINGLE_SUCCESS;
            }))
            .then(Commands.literal("reloadconfig").executes(c -> {
                AtlasBatch.setEnabled(false);
                AndroidSupportConfig.load();
                c.getSource().sendSuccess(() -> Component.literal("Android support configuration reloaded; experimental batching disabled."),false);
                return Command.SINGLE_SUCCESS;
            })));
        event.getDispatcher().register(Commands.literal("wachilandatlas")
            .then(Commands.literal("on").executes(c -> { AtlasBatch.setEnabled(true); c.getSource().sendSuccess(() -> Component.literal(AtlasBatch.status()), false); return Command.SINGLE_SUCCESS; }))
            .then(Commands.literal("off").executes(c -> { AtlasBatch.setEnabled(false); c.getSource().sendSuccess(() -> Component.literal(AtlasBatch.status()), false); return Command.SINGLE_SUCCESS; }))
            .then(Commands.literal("status").executes(c -> { c.getSource().sendSuccess(() -> Component.literal(AtlasBatch.status()), false); return Command.SINGLE_SUCCESS; })));
    }
}
