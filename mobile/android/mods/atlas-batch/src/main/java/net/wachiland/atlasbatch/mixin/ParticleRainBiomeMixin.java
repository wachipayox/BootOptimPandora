package net.wachiland.atlasbatch.mixin;

import com.llamalad7.mixinextras.injector.wrapmethod.WrapMethod;
import com.llamalad7.mixinextras.injector.wrapoperation.Operation;
import com.llamalad7.mixinextras.injector.wrapoperation.WrapOperation;
import net.minecraft.client.multiplayer.ClientLevel;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.util.RandomSource;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.block.state.BlockState;
import net.wachiland.atlasbatch.ParticleRainBiomeMemo;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Pseudo
@Mixin(targets="pigcart.particlerain.ParticleSpawner",remap=false)
abstract class ParticleRainBiomeMixin {
    @WrapMethod(method="tickBlockFX",require=0)
    private static void wachiland$scope(BlockPos.MutableBlockPos pos,BlockState state,RandomSource random,Operation<Void> original) {
        boolean active=ParticleRainBiomeMemo.begin();
        try { original.call(pos,state,random); }
        finally { if (active) ParticleRainBiomeMemo.end(); }
    }
    @WrapOperation(method="tickBlockFX",at=@At(value="INVOKE",target="Lnet/minecraft/client/multiplayer/ClientLevel;getBiome(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/core/Holder;"),require=0,remap=false)
    private static Holder<Biome> wachiland$biome(ClientLevel level,BlockPos pos,Operation<Holder<Biome>> original) {
        return ParticleRainBiomeMemo.read(level,pos,original);
    }
    // Keep only consecutive pure read/metadata iterations. Any geometry query, height
    // calculation or particle construction/insertion can call mod code: forget the read.
    @Inject(method="tickBlockFX",at={
        @At(value="INVOKE",target="Lnet/minecraft/client/multiplayer/ClientLevel;getBlockState(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/state/BlockState;"),
        @At(value="INVOKE",target="Lnet/minecraft/world/level/block/state/BlockState;getCollisionShape(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/phys/shapes/VoxelShape;"),
        @At(value="INVOKE",target="Lpigcart/particlerain/ParticleSpawner;getHeight(Lnet/minecraft/client/multiplayer/ClientLevel;II)I"),
        @At(value="INVOKE",target="Lnet/minecraft/client/particle/ParticleEngine;add(Lnet/minecraft/client/particle/Particle;)V"),
        @At(value="INVOKE",target="Lnet/minecraft/client/multiplayer/ClientLevel;addParticle(Lnet/minecraft/core/particles/ParticleOptions;DDDDDD)V")
    },require=0,remap=false)
    private static void wachiland$invalidate(BlockPos.MutableBlockPos pos,BlockState state,RandomSource random,CallbackInfo ci) {
        ParticleRainBiomeMemo.invalidate();
    }
}
