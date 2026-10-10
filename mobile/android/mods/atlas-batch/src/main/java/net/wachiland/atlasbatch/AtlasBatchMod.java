package net.wachiland.atlasbatch;

import com.mojang.brigadier.Command;
import net.minecraft.commands.Commands;
import net.minecraft.network.chat.Component;
import net.neoforged.api.distmarker.Dist;
import net.neoforged.fml.common.Mod;
import net.neoforged.neoforge.common.NeoForge;
import net.neoforged.neoforge.client.event.RegisterClientCommandsEvent;

@Mod(value = "wachiland_atlas_batch", dist = Dist.CLIENT)
public final class AtlasBatchMod {
    public AtlasBatchMod() { NeoForge.EVENT_BUS.addListener(AtlasBatchMod::commands); }
    private static void commands(RegisterClientCommandsEvent event) {
        event.getDispatcher().register(Commands.literal("wachilandatlas")
            .then(Commands.literal("on").executes(c -> { AtlasBatch.setEnabled(true); c.getSource().sendSuccess(() -> Component.literal(AtlasBatch.status()), false); return Command.SINGLE_SUCCESS; }))
            .then(Commands.literal("off").executes(c -> { AtlasBatch.setEnabled(false); c.getSource().sendSuccess(() -> Component.literal(AtlasBatch.status()), false); return Command.SINGLE_SUCCESS; }))
            .then(Commands.literal("status").executes(c -> { c.getSource().sendSuccess(() -> Component.literal(AtlasBatch.status()), false); return Command.SINGLE_SUCCESS; })));
    }
}
