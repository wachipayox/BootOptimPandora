package net.wachiland.android.compat;
import java.lang.instrument.*;
import java.security.*;
import java.util.*;
import jdk.internal.org.objectweb.asm.*;
import jdk.internal.org.objectweb.asm.tree.*;
/** Launcher-requested closure hook; exact JRE resource and instruction guards. */
public final class EarlyModuleGraphAgent {
 private static final String TARGET="java/lang/module/Resolver";
 private static final String HELPER="net/wachiland/android/compat/ModuleGraphClosure";
 private static final String PIN="164d37585babeb3e3c5cdeb13591f0bc6ae932b53da29f550331e425e1f676e2";
 public static byte[] rewrite(byte[] bytes) throws Exception {
  if(!PIN.equals(HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(bytes))))return null;
  return rewriteAudited(bytes,signature(bytes));
 }
 private static String signature(byte[] bytes) throws Exception {
  ClassNode type=new ClassNode(Opcodes.ASM8);new ClassReader(bytes).accept(type,0);
  MethodNode method=type.methods.stream().filter(m->m.name.equals("makeGraph")&&m.desc.equals("(Ljava/lang/module/Configuration;)Ljava/util/Map;")).findFirst().orElseThrow();
  if(!method.tryCatchBlocks.isEmpty())throw new IllegalStateException("Unexpected handlers");
  Map<LabelNode,Integer> labels=new IdentityHashMap<>();int at=0;
  for(AbstractInsnNode insn:method.instructions){if(insn instanceof LabelNode label)labels.put(label,at);if(insn.getOpcode()>=0)at++;}
  StringBuilder result=new StringBuilder(method.name+method.desc+method.access);
  for(AbstractInsnNode insn:method.instructions){if(insn.getOpcode()<0)continue;result.append(';').append(insn.getOpcode()).append(':');
   if(insn instanceof VarInsnNode n)result.append(n.var);
   else if(insn instanceof FieldInsnNode n)result.append(n.owner).append('|').append(n.name).append('|').append(n.desc);
   else if(insn instanceof MethodInsnNode n)result.append(n.owner).append('|').append(n.name).append('|').append(n.desc).append('|').append(n.itf);
   else if(insn instanceof TypeInsnNode n)result.append(n.desc);
   else if(insn instanceof IntInsnNode n)result.append(n.operand);
   else if(insn instanceof IincInsnNode n)result.append(n.var).append('|').append(n.incr);
   else if(insn instanceof LdcInsnNode n)result.append(n.cst.getClass().getName()).append('|').append(n.cst);
   else if(insn instanceof JumpInsnNode n)result.append(labels.get(n.label));
   else if(insn instanceof InvokeDynamicInsnNode n)result.append(n.name).append('|').append(n.desc).append('|').append(n.bsm).append('|').append(Arrays.deepToString(n.bsmArgs));
   else if(insn instanceof TableSwitchInsnNode n)result.append(n.min).append('|').append(n.max).append('|').append(labels.get(n.dflt)).append('|').append(n.labels.stream().map(labels::get).toList());
   else if(insn instanceof LookupSwitchInsnNode n)result.append(n.keys).append('|').append(labels.get(n.dflt)).append('|').append(n.labels.stream().map(labels::get).toList());
   else if(insn instanceof MultiANewArrayInsnNode n)result.append(n.desc).append('|').append(n.dims);
  }
  return HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(result.toString().getBytes(java.nio.charset.StandardCharsets.UTF_8)));
 }
 private static byte[] rewriteAudited(byte[] bytes,String expected) throws Exception {
  // Retransformation may reorder the constant pool and strip debug attributes.
  // Compare exact instructions/operands/branch targets, not those VM rewrites.
  if(!expected.equals(signature(bytes)))return null;
  ClassNode node=new ClassNode(Opcodes.ASM8);new ClassReader(bytes).accept(node,ClassReader.EXPAND_FRAMES);
  MethodNode graph=node.methods.stream().filter(m->m.name.equals("makeGraph")&&m.desc.equals("(Ljava/lang/module/Configuration;)Ljava/util/Map;")).findFirst().orElseThrow();
  if(!graph.tryCatchBlocks.isEmpty())return null;
  AbstractInsnNode tail=null,ret=null;int arrays=0,returns=0;
  for(AbstractInsnNode i:graph.instructions){if(i instanceof TypeInsnNode t && i.getOpcode()==Opcodes.NEW && t.desc.equals("java/util/ArrayList")){tail=i;arrays++;}if(i.getOpcode()==Opcodes.ARETURN){ret=i;returns++;}}
  if(arrays!=1||returns!=1||!(ret.getPrevious() instanceof VarInsnNode v)||v.getOpcode()!=Opcodes.ALOAD||v.var!=3)return null;
  InsnList call=new InsnList();LabelNode stock=new LabelNode();
  call.add(new VarInsnNode(Opcodes.ALOAD,3));call.add(new VarInsnNode(Opcodes.ALOAD,4));
  call.add(new MethodInsnNode(Opcodes.INVOKESTATIC,HELPER,"tryCandidate","(Ljava/util/Map;Ljava/util/Map;)Z",false));
  call.add(new JumpInsnNode(Opcodes.IFEQ,stock));call.add(new VarInsnNode(Opcodes.ALOAD,3));call.add(new InsnNode(Opcodes.ARETURN));call.add(stock);
  graph.instructions.insertBefore(tail,call);
  graph.instructions.insertBefore(ret,new MethodInsnNode(Opcodes.INVOKESTATIC,HELPER,"finishControl","()V",false));
  ClassWriter writer=new ClassWriter(ClassWriter.COMPUTE_FRAMES|ClassWriter.COMPUTE_MAXS);node.accept(writer);return writer.toByteArray();
 }
 public static void initialize(Instrumentation instrumentation) {
  String route=System.getProperty("wachiland.android.startup.moduleGraph","");if(!Set.of("candidate","control").contains(route))return;
  try {
   Class<?> helper=Class.forName(HELPER.replace('/','.'),true,null);
   Class<?> resolver=Class.forName(TARGET.replace('/','.'),false,null);
   byte[] packaged;
   try(var input=resolver.getResourceAsStream("/java/lang/module/Resolver.class")){if(input==null)throw new IllegalStateException("Runtime class resource unavailable");packaged=input.readNBytes(262145);}
   if(packaged.length>262144 || !PIN.equals(HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(packaged)))){System.out.println("[Wachiland module graph] unknown runtime resource; stock retained");return;}
   String expected=signature(packaged);
   if(!instrumentation.isRetransformClassesSupported()||!instrumentation.isModifiableClass(resolver)){System.out.println("[Wachiland module graph] unavailable; stock retained");return;}
   instrumentation.redefineModule(resolver.getModule(),Set.of(helper.getModule()),Map.of(),Map.of(),Set.of(),Map.of());
   ClassFileTransformer transformer=new ClassFileTransformer(){@Override public byte[] transform(Module module,ClassLoader loader,String name,Class<?> type,ProtectionDomain domain,byte[] bytes){if(!TARGET.equals(name))return null;try{byte[] result=rewriteAudited(bytes,expected);System.out.println("[Wachiland module graph] audited JRE21 hook="+(result!=null)+" route="+route);return result;}catch(Throwable error){System.out.println("[Wachiland module graph] skipped: "+error);return null;}}};
   instrumentation.addTransformer(transformer,true);instrumentation.retransformClasses(resolver);
  }catch(Throwable error){System.out.println("[Wachiland module graph] initialization unavailable: "+error);}
 }
}
