import java.nio.file.*;import java.util.*;import java.util.jar.*;import java.lang.reflect.*;
import net.wachiland.android.compat.EarlyOpcodeNamesAgent;
import jdk.internal.org.objectweb.asm.*;import jdk.internal.org.objectweb.asm.tree.*;
public class VerifyOpcodeAgent extends ClassLoader {
 VerifyOpcodeAgent(){super(VerifyOpcodeAgent.class.getClassLoader());}
 Class<?> define(String name,byte[] source){ClassNode node=new ClassNode(Opcodes.ASM8);new ClassReader(source).accept(node,0);ClassWriter writer=new ClassWriter(0);writer.visit(Opcodes.V17,Opcodes.ACC_PUBLIC,name,null,"java/lang/Object",null);
  for(MethodNode m:node.methods)if(m.name.equals("getOpcodeName")&&(m.desc.equals("(I)Ljava/lang/String;")||m.desc.equals("(ILjava/lang/String;I)Ljava/lang/String;"))){for(AbstractInsnNode i:m.instructions)if(i instanceof MethodInsnNode call &&call.owner.equals(node.name))call.owner=name;m.accept(writer);}writer.visitEnd();byte[] code=writer.toByteArray();return defineClass(name.replace('/','.'),code,0,code.length);}
 public static void main(String[] args)throws Exception{byte[] original;try(JarFile jar=new JarFile(args[0])){original=jar.getInputStream(jar.getJarEntry("org/spongepowered/asm/util/Bytecode.class")).readAllBytes();}
  byte[] candidate=EarlyOpcodeNamesAgent.rewrite(original,VerifyOpcodeAgent.class.getClassLoader());if(candidate==null)throw new AssertionError("runtime guard rejected");
  VerifyOpcodeAgent loader=new VerifyOpcodeAgent();Method stock=loader.define("test/Stock",original).getMethod("getOpcodeName",int.class);Method fast=loader.define("test/Candidate",candidate).getMethod("getOpcodeName",int.class);
  Set<Integer> values=new HashSet<>();for(int i=-4096;i<=4096;i++)values.add(i);values.add(Integer.MIN_VALUE);values.add(Integer.MAX_VALUE);for(Field f:Class.forName("org.objectweb.asm.Opcodes").getDeclaredFields())if(f.getType()==int.class)values.add(f.getInt(null));
  for(int value:values){Object a=stock.invoke(null,value),b=fast.invoke(null,value);if(!Objects.equals(a,b))throw new AssertionError(value+" "+a+" != "+b);}
  original[original.length-1]^=1;if(EarlyOpcodeNamesAgent.rewrite(original,VerifyOpcodeAgent.class.getClassLoader())!=null)throw new AssertionError("changed class accepted");
  System.out.println("opcode equivalence values="+values.size()+" including integer bounds/runtime constants; changed class keeps stock");
 }
}
