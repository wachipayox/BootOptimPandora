package net.wachiland.atlasbatch;

import com.google.gson.JsonParser;
import java.io.File;
import java.io.RandomAccessFile;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.StandardCopyOption;
import java.util.concurrent.atomic.AtomicBoolean;
import net.minecraft.client.Minecraft;

/** On-demand bridge to the launcher's existing guarded histogram, not another profiler. */
final class GuardedHeapRequest {
    private static final AtomicBoolean ATTEMPTED=new AtomicBoolean();
    static String request() {
        if(!AndroidSupportConfig.ANDROID || Minecraft.getInstance().getOverlay()!=null) return "Wait for resource reload to finish.";
        if(System.getProperty("wachiland.performance.agent")!=null) return "Heap census cannot run alongside the frame profiler.";
        String path=System.getProperty("wachiland.memory.file");
        if(path==null || !path.endsWith("-jvm.csv")) return "Existing launcher memory sampler is required.";
        if(!ATTEMPTED.compareAndSet(false,true)) return "Heap request already attempted in this process; see heap-request.txt.";
        File csv=new File(path);
        Thread worker=new Thread(() -> run(csv),"Wachiland-existing-heap-census-request");
        worker.setDaemon(true); worker.setPriority(Thread.MIN_PRIORITY); worker.start();
        return "Guarded heap census armed for up to 120 seconds. May pause if headroom passes; see heap-request.txt.";
    }
    private static void run(File csv) {
        File status=sibling(csv,"heap-request.txt");
        try {
            Files.writeString(status.toPath(),"epoch_ms="+System.currentTimeMillis()+"\nDiagnostic request: reuse existing HeapAttribution.histogram and hasHeadroom. Live phase requests one full GC and changes timing. Fresh actual Android sampler row, matching current PID, >=768 MiB available and lowMemory=false required before each phase. No heap dump or cache mutation.\n",StandardCharsets.UTF_8);
            Class<?> type=Class.forName("net.wachiland.diagnostics.HeapAttribution",false,ClassLoader.getSystemClassLoader());
            var constructor=type.getDeclaredConstructor(File.class); constructor.setAccessible(true);
            Object probe=constructor.newInstance(csv);
            Method guard=type.getDeclaredMethod("hasHeadroom"); guard.setAccessible(true);
            Method histogram=type.getDeclaredMethod("histogram",String.class,String[].class); histogram.setAccessible(true);
            long deadline=System.nanoTime()+120_000_000_000L;
            boolean ready=false;
            do {
                if(refreshGuard(csv,status) && Boolean.TRUE.equals(guard.invoke(probe))) { ready=true; break; }
                Thread.sleep(10000);
            } while(System.nanoTime()<deadline);
            if(!ready) { note(status,"Occupied skipped: no qualifying fresh headroom within 120 seconds."); return; }
            histogram.invoke(probe,"occupied",new String[]{"-all","-parallel=1"});
            if(!refreshGuard(csv,status) || !Boolean.TRUE.equals(guard.invoke(probe))) { note(status,"Live skipped: fresh headroom guard failed after occupied census."); return; }
            histogram.invoke(probe,"live",new String[]{"-parallel=1"});
            note(status,"Existing helper invoked; read heap-status.txt and histogram files for actual success/duration. No success inferred from this request.");
        } catch(Exception | LinkageError e) { try { note(status,"Optional request unavailable: "+e.getClass().getName()); } catch(Exception ignored) {} }
    }
    private static boolean refreshGuard(File csv,File status) throws Exception {
        File source=sibling(csv,"android.jsonl");
        byte[] bytes;
        try(RandomAccessFile in=new RandomAccessFile(source,"r")) {
            int size=(int)Math.min(32768,in.length()); bytes=new byte[size]; in.seek(in.length()-size); in.readFully(bytes);
        }
        String[] lines=new String(bytes,StandardCharsets.UTF_8).split("\n");
        com.google.gson.JsonObject row=null;
        for(int i=lines.length-1;i>=0;i--) {
            try { row=JsonParser.parseString(lines[i]).getAsJsonObject(); break; } catch(RuntimeException ignored) {}
        }
        if(row==null) return false;
        long epoch=row.get("epoch_ms").getAsLong(), available=row.get("system_available_bytes").getAsLong();
        long age=System.currentTimeMillis()-epoch;
        boolean low=row.get("system_low_memory").getAsBoolean();
        long pid=row.get("pid").getAsLong();
        boolean valid=pid==ProcessHandle.current().pid() && age>=0 && age<=15000 && available>=768L*1024*1024 && !low;
        note(status,"Android guard row: epoch="+epoch+" age_ms="+age+" pid="+pid+" current_pid="+ProcessHandle.current().pid()+" available_bytes="+available+" lowMemory="+low+" passes="+valid);
        if(!valid) return false;
        // Publish the measured timestamp, never the current time: old data cannot be made fresh.
        File staged=sibling(csv,"headroom.tmp"), target=sibling(csv,"headroom.guard");
        Files.writeString(staged.toPath(),epoch+","+available+","+low,StandardCharsets.UTF_8);
        try { Files.move(staged.toPath(),target.toPath(),StandardCopyOption.ATOMIC_MOVE,StandardCopyOption.REPLACE_EXISTING); }
        catch(java.nio.file.AtomicMoveNotSupportedException e) { Files.move(staged.toPath(),target.toPath(),StandardCopyOption.REPLACE_EXISTING); }
        return true;
    }
    private static File sibling(File csv,String suffix) { return new File(csv.getParentFile(),csv.getName().replace("jvm.csv",suffix)); }
    private static void note(File file,String text) throws Exception {
        Files.writeString(file.toPath(),System.currentTimeMillis()+" "+text+"\n",StandardCharsets.UTF_8,java.nio.file.StandardOpenOption.APPEND);
    }
}
