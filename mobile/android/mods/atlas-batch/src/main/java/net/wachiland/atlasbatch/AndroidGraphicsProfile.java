package net.wachiland.atlasbatch;

import com.mojang.blaze3d.systems.RenderSystem;
import com.mojang.logging.LogUtils;
import java.util.Locale;
import org.lwjgl.opengl.GL;
import org.lwjgl.opengl.GL11C;

/** Reports capabilities; a vendor name alone never enables a workaround. */
final class AndroidGraphicsProfile {
    private static boolean checked;
    private static String summary="pending render context";
    static void inspect() {
        if (checked || !AndroidSupportConfig.enabled || !AndroidSupportConfig.ANDROID || !RenderSystem.isOnRenderThread()) return;
        checked=true;
        try {
            String vendor=GL11C.glGetString(GL11C.GL_VENDOR);
            String renderer=GL11C.glGetString(GL11C.GL_RENDERER);
            String version=GL11C.glGetString(GL11C.GL_VERSION);
            var caps=GL.getCapabilities();
            String combined=(vendor+" "+renderer).toLowerCase(Locale.ROOT);
            String family=combined.contains("adreno") ? "Adreno" :
                combined.contains("mali") || combined.contains("immortalis") ? "Mali/Immortalis" :
                combined.contains("powervr") ? "PowerVR" : combined.contains("xclipse") ? "Xclipse" : "unknown";
            summary="family="+family+" vendor="+vendor+" renderer="+renderer+" version="+version+
                " framebuffer="+(caps.glBindFramebuffer!=0 && caps.glFramebufferTexture2D!=0)+
                " copy_tex_sub_image="+(caps.glCopyTexSubImage2D!=0)+
                " immutable_storage="+(caps.glTexStorage2D!=0)+" image_copy="+(caps.glCopyImageSubData!=0)+
                " sync="+(caps.glFenceSync!=0 && caps.glClientWaitSync!=0)+
                " max_texture_size="+GL11C.glGetInteger(GL11C.GL_MAX_TEXTURE_SIZE);
            LogUtils.getLogger().info("[Wachiland Android support] {}",summary);
        } catch (RuntimeException | LinkageError e) {
            summary="capabilities unavailable; stock behavior";
            LogUtils.getLogger().warn("[Wachiland Android support] {}",summary,e);
        }
    }
    static String status() { return summary; }
}
