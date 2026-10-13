import java.nio.file.*;
import java.io.*;
import net.wachiland.android.compat.EarlyModuleGraphAgent;
import jdk.internal.org.objectweb.asm.*;
import jdk.internal.org.objectweb.asm.util.*;
public class VerifyGraphHook {
 public static void main(String[] args)throws Exception{
  byte[] original=Files.readAllBytes(Path.of(args[0]));byte[] changed=EarlyModuleGraphAgent.rewrite(original);
  if(changed==null)throw new AssertionError("audited class rejected");
  StringWriter output=new StringWriter();CheckClassAdapter.verify(new ClassReader(changed),false,new PrintWriter(output));if(!output.toString().isBlank())throw new AssertionError(output.toString());
  byte[] mutated=original.clone();mutated[mutated.length-1]^=1;if(EarlyModuleGraphAgent.rewrite(mutated)!=null)throw new AssertionError("changed JRE accepted");
  Files.write(Path.of(args[1]),changed);System.out.println("Exact phone-JRE rewrite verified; mutated bytes retain stock");
 }
}
