package net.wachiland.atlasbatch;

import com.mojang.blaze3d.systems.RenderSystem;
import net.minecraft.client.Minecraft;
import net.neoforged.fml.ModList;
import org.lwjgl.opengl.GL;
import org.lwjgl.opengl.GL11C;
import org.lwjgl.opengl.GL32C;
import org.lwjgl.system.MemoryUtil;

/** Opt-in experiment: preserve Sodium draw order without a translation-layer command buffer. */
public final class DirectTerrainDraw {
    private static long batches, draws;
    private static String gate="not_checked";
    public static String status() { return "direct_terrain="+AndroidSupportConfig.directTerrainDraws+" gate="+gate+" batches="+batches+" draws="+draws; }
    public static boolean draw(int mode,long counts,int type,long indices,int size,long baseVertices) {
        if (!AndroidSupportConfig.ANDROID || !AndroidSupportConfig.enabled || !AndroidSupportConfig.directTerrainDraws
            || !RenderSystem.isOnRenderThread()) return false;
        var mc=Minecraft.getInstance();
        if (mc.level==null || mc.screen!=null || mc.getOverlay()!=null) return false;
        var mod=ModList.get().getModContainerById("sodium");
        if (mod.isEmpty() || !mod.get().getModInfo().getVersion().toString().equals("0.8.12-beta.1+mc1.21.1")) { gate="unknown_sodium_version"; return false; }
        var caps=GL.getCapabilities();
        String version=GL11C.glGetString(GL11C.GL_VERSION);
        // DrawID would change in a loop of single draws. Do not opt into a context
        // exposing either core or extension shader draw parameters, even if stock
        // Sodium shaders currently do not use them. Unknown renderers stay stock.
        if (version==null || !version.contains("MobileGlues") || caps.glDrawElementsBaseVertex==0
            || caps.OpenGL46 || caps.GL_ARB_shader_draw_parameters) { gate="renderer_or_drawid_capability"; return false; }
        if (ModList.get().isLoaded("iris") || ModList.get().isLoaded("oculus")) { gate="shader_pack_owner"; return false; }
        if (mode!=GL11C.GL_TRIANGLES || type!=GL11C.GL_UNSIGNED_INT || size<0 || size>16384
            || counts==0 || indices==0 || baseVertices==0) { gate="unknown_batch_shape"; return false; }
        // Validate the entire trusted Sodium native batch before submitting anything.
        // Never replay stock after partially submitting a batch (especially translucent terrain).
        for(int i=0;i<size;i++) if (MemoryUtil.memGetInt(counts+(long)i*4)<0 || MemoryUtil.memGetAddress(indices+(long)i*org.lwjgl.system.Pointer.POINTER_SIZE)<0) { gate="invalid_batch"; return false; }
        gate="eligible";
        for(int i=0;i<size;i++) {
            int count=MemoryUtil.memGetInt(counts+(long)i*4);
            if (count==0) continue;
            GL32C.nglDrawElementsBaseVertex(mode,count,type,MemoryUtil.memGetAddress(indices+(long)i*org.lwjgl.system.Pointer.POINTER_SIZE),MemoryUtil.memGetInt(baseVertices+(long)i*4));
            draws++;
        }
        batches++;
        return true;
    }
}

