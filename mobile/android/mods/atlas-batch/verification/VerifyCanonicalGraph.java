import java.nio.file.*;import java.io.*;import java.lang.reflect.*;
import net.wachiland.android.compat.EarlyModuleGraphAgent;
import jdk.internal.org.objectweb.asm.*;import jdk.internal.org.objectweb.asm.tree.*;import jdk.internal.org.objectweb.asm.util.*;
public class VerifyCanonicalGraph {
 public static void main(String[] a)throws Exception{
  byte[] original=Files.readAllBytes(Path.of(a[0]));Method signature=EarlyModuleGraphAgent.class.getDeclaredMethod("signature",byte[].class);signature.setAccessible(true);String expected=(String)signature.invoke(null,(Object)original);
  Method rewrite=EarlyModuleGraphAgent.class.getDeclaredMethod("rewriteAudited",byte[].class,String.class);rewrite.setAccessible(true);
  ClassWriter repack=new ClassWriter(0);new ClassReader(original).accept(repack,ClassReader.SKIP_DEBUG);byte[] different=repack.toByteArray();if(java.util.Arrays.equals(original,different))throw new AssertionError("test didn't repack");
  byte[] changed=(byte[])rewrite.invoke(null,different,expected);if(changed==null)throw new AssertionError("equivalent repack rejected");StringWriter errors=new StringWriter();CheckClassAdapter.verify(new ClassReader(changed),false,new PrintWriter(errors));if(!errors.toString().isBlank())throw new AssertionError(errors.toString());
  ClassNode node=new ClassNode(Opcodes.ASM8);new ClassReader(original).accept(node,0);boolean mutated=false;
  for(MethodNode m:node.methods)if(m.name.equals("makeGraph"))for(AbstractInsnNode i:m.instructions)if(i.getOpcode()==Opcodes.ICONST_0){m.instructions.set(i,new InsnNode(Opcodes.ICONST_1));mutated=true;break;}
  if(!mutated)throw new AssertionError("no mutation");ClassWriter bad=new ClassWriter(0);node.accept(bad);if(rewrite.invoke(null,bad.toByteArray(),expected)!=null)throw new AssertionError("changed instruction accepted");
  System.out.println("CP/debug repack accepted and verified; actual instruction mutation retains stock; signature="+expected);
 }
}
