package net.wachiland.atlasbatch.mixin;
import com.llamalad7.mixinextras.injector.wrapmethod.WrapMethod;
import com.llamalad7.mixinextras.injector.wrapoperation.*;
import com.mojang.blaze3d.pipeline.RenderTarget;
import com.mojang.blaze3d.systems.RenderSystem;
import net.minecraft.client.Minecraft;
import net.minecraft.client.renderer.texture.TextureManager;
import net.wachiland.atlasbatch.BackgroundMode;
import org.spongepowered.asm.mixin.*;
import org.spongepowered.asm.mixin.injection.*;
import org.spongepowered.asm.mixin.injection.callback.*;
import java.util.concurrent.locks.LockSupport;

/** Keep stock scheduled work/ticks/reload uploads, suppress invisible presentation. */
@Mixin(Minecraft.class)
abstract class BackgroundMinecraftMixin {
 @Shadow public boolean noRender;
 @Unique private boolean wachiland$hiddenFrame;
 @Unique private long wachiland$nextLoadingFrame;
 @WrapMethod(method="runTick")
 private void wachiland$backgroundFrame(boolean render,Operation<Void> original){
  boolean hidden=BackgroundMode.hidden(),saved=noRender,savedFrame=wachiland$hiddenFrame;long start=System.nanoTime();
  // LoadingOverlay advances its completion callback from render(). Keep a 1Hz
  // offscreen loading heartbeat; never stop the reload completion machinery.
  Minecraft client=(Minecraft)(Object)this;
  boolean loadingFrame=hidden&&(!client.isGameLoadFinished()||client.getOverlay()!=null)&&start>=wachiland$nextLoadingFrame;
  if(loadingFrame)wachiland$nextLoadingFrame=start+1_000_000_000L;
  wachiland$hiddenFrame=hidden&&!loadingFrame;
  if(wachiland$hiddenFrame)noRender=true;
  // false also marks intentional no-tick frames during disconnect/level teardown.
  // Preserve that caller contract; hidden visibility must never force a tick.
  boolean suppressed=wachiland$hiddenFrame;
  try{original.call(render);}finally{if(suppressed)noRender=saved;wachiland$hiddenFrame=savedFrame;if(hidden&&render){long left=50_000_000L-(System.nanoTime()-start);if(left>0)LockSupport.parkNanos(left);}}
 }
 @Inject(method="getFramerateLimit",at=@At("HEAD"),cancellable=true)
 private void wachiland$logicRate(CallbackInfoReturnable<Integer> result){if(BackgroundMode.hidden())result.setReturnValue(20);}
 @Inject(method="onGameLoadFinished",at=@At("RETURN"))
 private void wachiland$ready(CallbackInfo callback){BackgroundMode.ready();}
 @WrapOperation(method="runTick",at=@At(value="INVOKE",target="Lcom/mojang/blaze3d/systems/RenderSystem;clear(IZ)V"))
 private void wachiland$clear(int mask,boolean mac,Operation<Void> original){if(!wachiland$hiddenFrame)original.call(mask,mac);}
 @WrapOperation(method="runTick",at=@At(value="INVOKE",target="Lcom/mojang/blaze3d/pipeline/RenderTarget;bindWrite(Z)V"))
 private void wachiland$bind(RenderTarget target,boolean viewport,Operation<Void> original){if(!wachiland$hiddenFrame)original.call(target,viewport);}
 @WrapOperation(method="runTick",at=@At(value="INVOKE",target="Lcom/mojang/blaze3d/pipeline/RenderTarget;unbindWrite()V"))
 private void wachiland$unbind(RenderTarget target,Operation<Void> original){if(!wachiland$hiddenFrame)original.call(target);}
 @WrapOperation(method="runTick",at=@At(value="INVOKE",target="Lcom/mojang/blaze3d/pipeline/RenderTarget;blitToScreen(II)V"))
 private void wachiland$blit(RenderTarget target,int width,int height,Operation<Void> original){if(!wachiland$hiddenFrame)original.call(target,width,height);}
 @WrapOperation(method="tick",at=@At(value="INVOKE",target="Lnet/minecraft/client/renderer/texture/TextureManager;tick()V"))
 private void wachiland$textures(TextureManager manager,Operation<Void> original){if(!BackgroundMode.hidden())original.call(manager);}
}
