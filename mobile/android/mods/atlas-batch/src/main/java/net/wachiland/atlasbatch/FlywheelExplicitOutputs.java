package net.wachiland.atlasbatch;

import com.mojang.blaze3d.systems.RenderSystem;
import com.mojang.logging.LogUtils;
import org.lwjgl.opengl.GL;

/** Opt-in compatibility trial for Flywheel 1.0.6's exact OIT output declaration. */
public final class FlywheelExplicitOutputs {
    private static int patched;
    private static final String ORIGINAL = "out vec4 _flw_coeffs0;\nout vec4 _flw_coeffs1;\nout vec4 _flw_coeffs2;\nout vec4 _flw_coeffs3;";
    private static final String EXPLICIT = "layout(location = 0) out vec4 _flw_coeffs0;\nlayout(location = 1) out vec4 _flw_coeffs1;\nlayout(location = 2) out vec4 _flw_coeffs2;\nlayout(location = 3) out vec4 _flw_coeffs3;";

    public static CharSequence rewrite(CharSequence input) {
        if (!AndroidSupportConfig.ANDROID || !AndroidSupportConfig.enabled
            || !AndroidSupportConfig.flywheelExplicitOutputs || !RenderSystem.isOnRenderThread()) return input;
        try {
            if (!GL.getCapabilities().OpenGL33) return input;
            String source=input.toString();
            // Exact, single declaration block only: unknown versions/rewriters keep stock source.
            if (!source.contains("#ifdef _FLW_COLLECT_COEFFS") || !source.contains("_FLW_OIT")
                || !source.contains(ORIGINAL) || source.indexOf(ORIGINAL)!=source.lastIndexOf(ORIGINAL)
                || source.contains("layout(location") || source.contains("layout (location")) return input;
            String result=source.replace(ORIGINAL,EXPLICIT);
            if (++patched<=8) LogUtils.getLogger().info("[Wachiland Android support] Flywheel explicit OIT outputs 0..3 applied; shader={}",patched);
            return result;
        } catch (RuntimeException | LinkageError error) {
            return input;
        }
    }
}
