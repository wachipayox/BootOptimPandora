package net.wachiland.android.compat;
import java.lang.instrument.*;import java.security.*;import java.lang.reflect.Field;import java.util.*;
import jdk.internal.org.objectweb.asm.*;import jdk.internal.org.objectweb.asm.tree.*;
/** Uses own BootOptim #213 semantic proof through the already-required Android agent.
 * No BootOptim source changed. No transformed-byte cache, callback bypass or classloader replacement. */
public final class EarlyOpcodeNamesAgent {
 private static final String TARGET="org/spongepowered/asm/util/Bytecode";
 private static final String TARGET_METHOD="getOpcodeName",TARGET_DESC="(I)Ljava/lang/String;",STOCK_HELPER_DESC="(ILjava/lang/String;I)Ljava/lang/String;";
 private static final String PIN="f739f2b50dc1deb178df4cd16a977808795d965a0fe0118a5af233f5db8abe37";
    private static final int[] STOCK_DECIMAL_OPCODES = {
        19, 20,
        26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45,
        59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78,
        196
    };

 public static boolean semantics(ClassLoader loader)throws Exception {
  Class<?> opcodeClass=Class.forName("org.objectweb.asm.Opcodes",true,loader);
  String[] table=(String[])Class.forName("org.objectweb.asm.util.Printer",true,loader).getField("OPCODES").get(null);
  Map<Integer,String> stock=new HashMap<>();boolean found=false;
  for(Field field:opcodeClass.getDeclaredFields()){if(!found&&!field.getName().equals("UNINITIALIZED_THIS"))continue;found=true;if(field.getType()!=Integer.TYPE)continue;int value=field.getInt(null);if(value>=1)stock.putIfAbsent(value,field.getName());}
  if(!found)return false;
  for(int value=0;value<table.length;value++)if(!Objects.equals(stock.getOrDefault(value,String.valueOf(value)),direct(value,table)))return false;
  for(var entry:stock.entrySet())if(!entry.getValue().equals(direct(entry.getKey(),table)))return false;
  return true;
 }
 public static String direct(int value,String[] table){if(value<0)return "UNKNOWN";if(value==0||Arrays.binarySearch(STOCK_DECIMAL_OPCODES,value)>=0)return String.valueOf(value);if(value<table.length&&table[value]!=null)return table[value];return String.valueOf(value);}
 public static byte[] rewrite(byte[] bytes,ClassLoader loader)throws Exception{
  if(!PIN.equals(HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(bytes))))return null;
  ClassNode node=new ClassNode(Opcodes.ASM8);new ClassReader(bytes).accept(node,0);
  MethodNode method=node.methods.stream().filter(m->m.name.equals(TARGET_METHOD)&&m.desc.equals(TARGET_DESC)).findFirst().orElse(null);
  if(method==null||!matchesMixin087Wrapper(method)||!semantics(loader))return null;
  replaceBody(method);ClassWriter writer=new ClassWriter(0);node.accept(writer);return writer.toByteArray();
 }
 public static void initialize(Instrumentation instrumentation){String route=System.getProperty("wachiland.android.startup.opcodeNames","");if(!Set.of("candidate","control").contains(route))return;
  for(Class<?> loaded:instrumentation.getAllLoadedClasses())if(loaded.getName().equals(TARGET.replace('/','.'))){System.out.println("[Wachiland opcode names] already loaded; stock retained");return;}
  instrumentation.addTransformer(new ClassFileTransformer(){@Override public byte[] transform(Module module,ClassLoader loader,String name,Class<?> type,ProtectionDomain domain,byte[] bytes){if(!TARGET.equals(name))return null;try{byte[] changed=rewrite(bytes,loader);System.out.println("[Wachiland opcode names] guarded="+(changed!=null)+" route="+route);return route.equals("candidate")?changed:null;}catch(Throwable error){System.out.println("[Wachiland opcode names] stock retained: "+error);return null;}}},false);
 }
    static boolean matchesMixin087Wrapper(MethodNode method) {
        List<AbstractInsnNode> real = new ArrayList<>();
        for (AbstractInsnNode insn = method.instructions.getFirst(); insn != null; insn = insn.getNext()) {
            if (insn.getOpcode() >= 0) {
                real.add(insn);
            }
        }
        if (real.size() != 5
                || real.get(0).getOpcode() != Opcodes.ILOAD
                || real.get(1).getOpcode() != Opcodes.LDC
                || real.get(2).getOpcode() != Opcodes.ICONST_1
                || real.get(3).getOpcode() != Opcodes.INVOKESTATIC
                || real.get(4).getOpcode() != Opcodes.ARETURN) {
            return false;
        }
        if (!(real.get(0) instanceof VarInsnNode load) || load.var != 0) {
            return false;
        }
        if (!(real.get(1) instanceof LdcInsnNode ldc) || !"UNINITIALIZED_THIS".equals(ldc.cst)) {
            return false;
        }
        if (!(real.get(3) instanceof MethodInsnNode call)) {
            return false;
        }
        return TARGET.equals(call.owner)
                && TARGET_METHOD.equals(call.name)
                && STOCK_HELPER_DESC.equals(call.desc)
                && !call.itf;
    }

    private static void replaceBody(MethodNode method) {
        InsnList instructions = new InsnList();
        LabelNode table = new LabelNode();
        LabelNode decimal = new LabelNode();
        LabelNode unknown = new LabelNode();
        LabelNode[] decimalTargets = new LabelNode[STOCK_DECIMAL_OPCODES.length];
        Arrays.fill(decimalTargets, decimal);

        instructions.add(new VarInsnNode(Opcodes.ILOAD, 0));
        instructions.add(new JumpInsnNode(Opcodes.IFLT, unknown));
        instructions.add(new VarInsnNode(Opcodes.ILOAD, 0));
        instructions.add(new JumpInsnNode(Opcodes.IFEQ, decimal));
        instructions.add(new VarInsnNode(Opcodes.ILOAD, 0));
        instructions.add(new LookupSwitchInsnNode(table, STOCK_DECIMAL_OPCODES, decimalTargets));

        instructions.add(table);
        instructions.add(new FrameNode(Opcodes.F_SAME, 0, null, 0, null));
        instructions.add(new VarInsnNode(Opcodes.ILOAD, 0));
        instructions.add(new FieldInsnNode(
                Opcodes.GETSTATIC,
                "org/objectweb/asm/util/Printer",
                "OPCODES",
                "[Ljava/lang/String;"));
        instructions.add(new InsnNode(Opcodes.ARRAYLENGTH));
        instructions.add(new JumpInsnNode(Opcodes.IF_ICMPGE, decimal));
        instructions.add(new FieldInsnNode(
                Opcodes.GETSTATIC,
                "org/objectweb/asm/util/Printer",
                "OPCODES",
                "[Ljava/lang/String;"));
        instructions.add(new VarInsnNode(Opcodes.ILOAD, 0));
        instructions.add(new InsnNode(Opcodes.AALOAD));
        instructions.add(new InsnNode(Opcodes.ARETURN));

        instructions.add(decimal);
        instructions.add(new FrameNode(Opcodes.F_SAME, 0, null, 0, null));
        instructions.add(new VarInsnNode(Opcodes.ILOAD, 0));
        instructions.add(new MethodInsnNode(
                Opcodes.INVOKESTATIC,
                "java/lang/String",
                "valueOf",
                "(I)Ljava/lang/String;",
                false));
        instructions.add(new InsnNode(Opcodes.ARETURN));

        instructions.add(unknown);
        instructions.add(new FrameNode(Opcodes.F_SAME, 0, null, 0, null));
        instructions.add(new LdcInsnNode("UNKNOWN"));
        instructions.add(new InsnNode(Opcodes.ARETURN));

        method.instructions.clear();
        method.instructions.add(instructions);
        method.tryCatchBlocks.clear();
        method.localVariables = null;
        method.maxLocals = 1;
        method.maxStack = 2;
    }

}
