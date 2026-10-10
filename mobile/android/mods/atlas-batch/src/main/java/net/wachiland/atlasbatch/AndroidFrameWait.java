package net.wachiland.atlasbatch;

import com.mojang.blaze3d.systems.RenderSystem;
import java.io.InputStream;
import java.util.concurrent.locks.LockSupport;
import org.lwjgl.glfw.GLFW;
import org.objectweb.asm.ClassReader;
import org.objectweb.asm.Opcodes;
import org.objectweb.asm.tree.ClassNode;

/** Replace only a verified empty Android wait. Vanilla still owns the deadline. */
public final class AndroidFrameWait {
    private static Boolean emptyWait;
    private static long waits;
    private static String gate="not_checked";
    public static String status() { return "frame_wait="+AndroidSupportConfig.frameWait+" wait_gate="+gate+" waits="+waits; }
    public static void waitFor(double seconds) {
        if (!AndroidSupportConfig.ANDROID || !AndroidSupportConfig.enabled || !AndroidSupportConfig.frameWait
            || !RenderSystem.isOnRenderThread() || !Double.isFinite(seconds) || seconds<=0
            || Thread.currentThread().isInterrupted()) return;
        if (emptyWait==null) emptyWait=verifyEmptyWait();
        if (!emptyWait) return;
        // Short bounded parks retain responsiveness. Early/spurious wakeups are
        // harmless: vanilla rechecks its existing monotonic deadline each time.
        long nanos=(long)Math.min(seconds*1_000_000_000.0,2_000_000.0);
        if (nanos>0) { waits++; LockSupport.parkNanos(nanos); }
    }
    private static boolean verifyEmptyWait() {
        try (InputStream in=GLFW.class.getResourceAsStream("GLFW.class")) {
            if (in==null) { gate="missing_class_resource"; return false; }
            byte[] bytes=in.readNBytes(2_000_001);
            if (bytes.length>2_000_000) { gate="unexpected_class_size"; return false; }
            ClassNode node=new ClassNode();
            new ClassReader(bytes).accept(node,ClassReader.SKIP_DEBUG|ClassReader.SKIP_FRAMES);
            for (var method:node.methods) {
                if (!method.name.equals("glfwWaitEventsTimeout") || !method.desc.equals("(D)V")) continue;
                if ((method.access & Opcodes.ACC_STATIC)==0) break;
                int instructions=0;
                for (var instruction:method.instructions) {
                    if (instruction.getOpcode()<0) continue;
                    if (instruction.getOpcode()!=Opcodes.RETURN) { gate="wait_has_implementation"; return false; }
                    instructions++;
                }
                if (instructions==1) { gate="verified_empty_android_wait"; return true; }
                break;
            }
            gate="unknown_wait";
        } catch (Exception | LinkageError failure) { gate="verification_failed"; }
        return false;
    }
}
