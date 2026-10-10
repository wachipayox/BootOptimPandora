package net.wachiland.atlasbatch;

import java.lang.management.ManagementFactory;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import javax.management.ObjectName;
import net.neoforged.fml.loading.FMLPaths;

/** Explicit, one-shot compiled-method attribution; never part of timed capture. */
final class JavaCodeMap {
    static String dump() {
        try {
            var server=ManagementFactory.getPlatformMBeanServer();
            var name=new ObjectName("com.sun.management:type=DiagnosticCommand");
            String code=(String)server.invoke(name,"compilerCodelist",new Object[]{new String[0]},new String[]{"[Ljava.lang.String;"});
            var path=FMLPaths.GAMEDIR.get().resolve("wachiland-jit-codelist.txt");
            Files.writeString(path,code,StandardCharsets.UTF_8);
            return "Compiled-method map written to wachiland-jit-codelist.txt; no GC requested.";
        } catch (Exception | LinkageError e) {
            return "Compiled-method mapping unavailable: "+e.getClass().getSimpleName();
        }
    }
}
