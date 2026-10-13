package net.wachiland.android.compat;

import java.lang.instrument.ClassFileTransformer;
import java.lang.instrument.Instrumentation;
import java.security.ProtectionDomain;
import java.security.MessageDigest;
import java.util.HexFormat;
import java.util.Map;
import java.util.Set;
import java.net.URLClassLoader;
import java.lang.invoke.MethodHandles;
import java.lang.invoke.MethodHandle;
import jdk.internal.org.objectweb.asm.*;

/** Android-only addon: adapts GL calls in the audited external Drippy provider. */
public final class EarlyCompatAgent {
    private static final java.util.concurrent.atomic.AtomicBoolean STARTED=new java.util.concurrent.atomic.AtomicBoolean();
    private static final String TARGET="de/keksuccino/drippyloadingscreen/earlywindow/window/DrippyEarlyWindowProvider";
    private static final String PIN="12bc661aed5dbce06c2907eaba9709714298bcde7d8edb67a580b12740d6edf3";
    private static final String BRIDGE="net/wachiland/android/compat/EarlyBridge";
    private static final Map<String,String> CALLS=Map.ofEntries(
        Map.entry("glMatrixMode","(I)V"),Map.entry("glLoadIdentity","()V"),Map.entry("glOrtho","(DDDDDD)V"),
        Map.entry("glPushMatrix","()V"),Map.entry("glPopMatrix","()V"),Map.entry("glScalef","(FFF)V"),Map.entry("glTranslatef","(FFF)V"),
        Map.entry("glBegin","(I)V"),Map.entry("glEnd","()V"),Map.entry("glVertex2f","(FF)V"),Map.entry("glTexCoord2f","(FF)V"),
        Map.entry("glColor4f","(FFFF)V"),Map.entry("glVertexPointer","(IIILjava/nio/ByteBuffer;)V"),Map.entry("glDrawArrays","(III)V"),
        Map.entry("glEnable","(I)V"),Map.entry("glDisable","(I)V"),Map.entry("glIsEnabled","(I)Z"),
        Map.entry("glEnableClientState","(I)V"),Map.entry("glDisableClientState","(I)V"));
    public static void initialize(Instrumentation instrumentation) {
        if (!STARTED.compareAndSet(false,true)) return;
        EarlyModuleGraphAgent.initialize(instrumentation);
        EarlyOpcodeNamesAgent.initialize(instrumentation);
        if (!Boolean.getBoolean("wachiland.android.early.compat")) return;
        instrumentation.addTransformer(new ClassFileTransformer() {
            @Override public byte[] transform(Module module,ClassLoader loader,String name,Class<?> redefining,ProtectionDomain domain,byte[] bytes) {
                if (!TARGET.equals(name)) return null;
                try {
                    if (!PIN.equals(HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(bytes)))) {
                        System.out.println("[Wachiland early compatibility] Provider changed; stock retained"); return null;
                    }
                    Class<?> facade=Class.forName("net.wachiland.android.compat.EarlyBridge",true,null);
                    if(facade.getClassLoader()!=null)throw new IllegalStateException("Early facade not bootstrap owned");
                    // Use Drippy's exact LWJGL classes/capabilities through its parent.
                    Class<?> renderer;
                    try(URLClassLoader child=new URLClassLoader(new java.net.URL[]{EarlyCompatAgent.class.getProtectionDomain().getCodeSource().getLocation()},loader)) {
                        renderer=Class.forName("net.wachiland.android.compat.EarlyGl",true,child);
                    }
                    java.lang.reflect.Method[] methods=facade.getDeclaredMethods();
                    java.util.HashMap<String,MethodHandle> delegates=new java.util.HashMap<>();
                    for(java.lang.reflect.Method method:methods) {
                        if(method.getName().startsWith("gl") || Set.of("beginFrame","endFrame","close","fitUiScale").contains(method.getName()))
                            delegates.put(method.getName(),MethodHandles.publicLookup().unreflect(renderer.getMethod(method.getName(),method.getParameterTypes())));
                    }
                    facade.getMethod("install",Map.class).invoke(null,delegates);
                    if (module!=null && module.isNamed()) instrumentation.redefineModule(module,Set.of(facade.getModule()),Map.of(),Map.of(),Set.of(),Map.of());
                    ClassReader reader=new ClassReader(bytes); ClassWriter writer=new ClassWriter(reader,ClassWriter.COMPUTE_MAXS);
                    int[] hooks={0};
                    reader.accept(new ClassVisitor(Opcodes.ASM8,writer) {
                        @Override public MethodVisitor visitMethod(int access,String method,String desc,String signature,String[] exceptions) {
                            MethodVisitor mv=super.visitMethod(access,method,desc,signature,exceptions);
                            return new MethodVisitor(Opcodes.ASM8,mv) {
                                @Override public void visitCode() {
                                    super.visitCode();
                                    if (method.equals("drawFrame") && desc.equals("()V")) {
                                        Label draw=new Label();super.visitMethodInsn(Opcodes.INVOKESTATIC,"net/wachiland/android/compat/LifecycleBridge","renderEarly","()Z",false);
                                        super.visitJumpInsn(Opcodes.IFNE,draw);super.visitInsn(Opcodes.RETURN);super.visitLabel(draw);super.visitFrame(Opcodes.F_SAME,0,null,0,null);
                                    }
                                    if (method.equals("drawFrame") && desc.equals("()V")) super.visitMethodInsn(Opcodes.INVOKESTATIC,BRIDGE,"beginFrame","()V",false);
                                }
                                @Override public void visitMethodInsn(int op,String owner,String called,String descriptor,boolean itf) {
                                    if (op==Opcodes.INVOKESTATIC && owner.equals("org/lwjgl/opengl/GL11") && descriptor.equals(CALLS.get(called))) {
                                        hooks[0]++; owner=BRIDGE;
                                    }
                                    super.visitMethodInsn(op,owner,called,descriptor,itf);
                                }
                                @Override public void visitInsn(int op) {
                                    if(op==Opcodes.FRETURN && method.equals("computeUiScale") && desc.equals("()F")) {
                                        super.visitVarInsn(Opcodes.ALOAD,0);
                                        super.visitFieldInsn(Opcodes.GETFIELD,TARGET,"windowWidth","I");
                                        super.visitVarInsn(Opcodes.ALOAD,0);
                                        super.visitFieldInsn(Opcodes.GETFIELD,TARGET,"options","Lde/keksuccino/drippyloadingscreen/earlywindow/window/config/EarlyLoadingOptions;");
                                        super.visitMethodInsn(Opcodes.INVOKEVIRTUAL,"de/keksuccino/drippyloadingscreen/earlywindow/window/config/EarlyLoadingOptions","logoWidth","()I",false);
                                        super.visitVarInsn(Opcodes.ALOAD,0);
                                        super.visitFieldInsn(Opcodes.GETFIELD,TARGET,"options","Lde/keksuccino/drippyloadingscreen/earlywindow/window/config/EarlyLoadingOptions;");
                                        super.visitMethodInsn(Opcodes.INVOKEVIRTUAL,"de/keksuccino/drippyloadingscreen/earlywindow/window/config/EarlyLoadingOptions","logoOffsetX","()I",false);
                                        super.visitVarInsn(Opcodes.ALOAD,0);
                                        super.visitFieldInsn(Opcodes.GETFIELD,TARGET,"logoTexture","Lde/keksuccino/drippyloadingscreen/earlywindow/window/texture/EarlyWindowTextureLoader$LoadedTexture;");
                                        super.visitMethodInsn(Opcodes.INVOKESTATIC,BRIDGE,"fitUiScale","(FIIILjava/lang/Object;)F",false);
                                    }
                                    if (op==Opcodes.RETURN && desc.equals("()V")) {
                                        if (method.equals("drawFrame")) super.visitMethodInsn(Opcodes.INVOKESTATIC,BRIDGE,"endFrame","()V",false);
                                        if (method.equals("cleanupTextures")) super.visitMethodInsn(Opcodes.INVOKESTATIC,BRIDGE,"close","()V",false);
                                    }
                                    super.visitInsn(op);
                                }
                            };
                        }
                    },0);
                    if (hooks[0]!=52) { System.out.println("[Wachiland early compatibility] Unexpected GL coverage="+hooks[0]+"; stock retained"); return null; }
                    System.out.println("[Wachiland early compatibility] Audited Drippy adapter installed; GL calls="+hooks[0]);
                    return writer.toByteArray();
                } catch(Throwable error) { System.out.println("[Wachiland early compatibility] Adapter skipped: "+error); return null; }
            }
        },false);
    }
}
