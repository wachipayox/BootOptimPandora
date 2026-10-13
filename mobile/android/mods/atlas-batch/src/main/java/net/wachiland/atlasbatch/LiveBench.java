package net.wachiland.atlasbatch;

import java.lang.reflect.Field;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Arrays;
import java.util.concurrent.atomic.AtomicLong;
import net.minecraft.client.Minecraft;
import net.minecraft.client.multiplayer.ClientLevel;
import net.minecraft.world.phys.Vec3;
import net.neoforged.neoforge.client.event.ClientTickEvent;

/** Explicit diagnostic only. Reuses the bootstrap agent; does not instrument game code. */
final class LiveBench {
    private static Class<?> counters;
    private static Field culling;
    private static Path base;
    private static String label;
    private static long start, epoch, pending, end;
    private static int ticks, duration;
    private static boolean running, oldFrustum, oldCulling, wantedFrustum, wantedCulling;
    private static boolean oldStaging, wantedStaging;
    private static Boolean requestedStaging;
    private static ClientLevel level;
    private static Vec3 position;
    private static float yaw, pitch, rain;
    private static String invalid;
    private static long[] initialCount, initialWall;

    static void tick(ClientTickEvent.Post ignored) {
        if (!Boolean.getBoolean("wachiland.performance.manual")) return;
        Minecraft mc = Minecraft.getInstance();
        try {
            if (counters == null) {
                counters = Class.forName("net.wachiland.performance.PerformanceCounters", false, null);
                if (counters.getClassLoader() != null) throw new IllegalStateException("Counters not bootstrap owned");
                culling = Class.forName("dev.tr7zw.entityculling.versionless.EntityCullingVersionlessBase").getField("enabled");
                base = Path.of(System.getProperty("wachiland.performance.base"));
            }
            long now = System.nanoTime();
            if (running) {
                if (mc.level != level || mc.player == null) invalid = "world_changed";
                else if (mc.screen != null) invalid = "screen_opened";
                else if (mc.player.position().distanceToSqr(position) > 0.0025
                    || Math.abs(mc.player.getYRot()-yaw) > 0.2 || Math.abs(mc.player.getXRot()-pitch) > 0.2) invalid = "camera_or_position_changed";
                else if (Math.abs(mc.level.getRainLevel(1)-rain) > 0.02) invalid = "weather_changed";
                if (now >= end || invalid != null) finish(now);
                return;
            }
            if (pending != 0) {
                if (mc.screen != null || mc.player == null || mc.level == null) { pending=now+5_000_000_000L; return; }
                if (now < pending) return;
                oldFrustum=AndroidSupportConfig.frustumCullingQueries; oldCulling=culling.getBoolean(null);
                oldStaging=AndroidSupportConfig.stagedUploads;
                wantedStaging=requestedStaging == null ? oldStaging : requestedStaging;
                AndroidSupportConfig.frustumCullingQueries=wantedFrustum; culling.setBoolean(null,wantedCulling);
                AndroidSupportConfig.stagedUploads=wantedStaging;
                // Allow culling state and frustum snapshots to settle, outside the measurement.
                pending=0; start=now+10_000_000_000L; return;
            }
            if (start != 0) {
                if (mc.player == null || mc.level == null || mc.screen != null) {
                    restore(); start=0; return;
                }
                if (now < start) return;
                level=mc.level; position=mc.player.position(); yaw=mc.player.getYRot(); pitch=mc.player.getXRot(); rain=mc.level.getRainLevel(1);
                counters.getField("active").setBoolean(null,false);
                counters.getField("traceCount").setInt(null,0); counters.getField("traceOverflow").setLong(null,0);
                initialCount=snapshot("COUNT"); initialWall=snapshot("NANOS");
                start=System.nanoTime(); epoch=System.currentTimeMillis(); end=start+duration*1_000_000_000L; invalid=null;
                counters.getField("captureOriginNanos").setLong(null,start);
                running=true; counters.getField("active").setBoolean(null,true);
                status("capture_start label="+label+" epoch_ms="+epoch+" frustum="+wantedFrustum+" culling="+wantedCulling+" staging="+wantedStaging+" duration_s="+duration+" position="+position+" yaw="+yaw+" pitch="+pitch+" rain="+rain);
                return;
            }
            if (++ticks % 20 != 0) return;
            Path request=mc.gameDirectory.toPath().resolve("wachiland-live-bench-request.txt");
            if (!Files.isRegularFile(request)) return;
            String[] fields=Files.readString(request,StandardCharsets.UTF_8).trim().split("\\s+");
            Files.delete(request);
            if ((fields.length!=4 && fields.length!=5) || !fields[0].matches("[a-zA-Z0-9_-]{1,48}") || !fields[2].matches("true|false") || !fields[3].matches("true|false")
                || (fields.length==5 && !fields[4].matches("true|false"))) throw new IllegalArgumentException("Expected label seconds frustum culling [staging]");
            int seconds=Integer.parseInt(fields[1]); if (seconds<30 || seconds>120) throw new IllegalArgumentException("Capture must be 30..120 seconds");
            label=fields[0]; duration=seconds; wantedFrustum=Boolean.parseBoolean(fields[2]); wantedCulling=Boolean.parseBoolean(fields[3]);
            requestedStaging=fields.length==5 ? Boolean.parseBoolean(fields[4]) : null;
            pending=now+5_000_000_000L;
            status("capture_queued label="+label);
        } catch (Throwable error) {
            try { if (counters!=null) counters.getField("active").setBoolean(null,false); } catch (Throwable ignoredError) { }
            if (running || start!=0) restore(); running=false; start=0; pending=0;
            status("capture_error "+error.getClass().getSimpleName()+":"+error.getMessage());
        }
    }

    private static void finish(long now) throws Exception {
        counters.getField("active").setBoolean(null,false);
        int count=counters.getField("traceCount").getInt(null);
        long overflow=counters.getField("traceOverflow").getLong(null);
        String[] names=(String[]) counters.getField("NAMES").get(null);
        long[] finalCount=snapshot("COUNT"), finalWall=snapshot("NANOS");
        long[][] trace=new long[5][];
        String[] arrays={"FRAME_END","FRAME_DURATION","FRAME_TICKS","FRAME_TICK_WALL","FRAME_WORLD_WALL"};
        for(int i=0;i<5;i++) trace[i]=Arrays.copyOf((long[])counters.getField(arrays[i]).get(null),count);
        String phase=label, failure=invalid; long origin=start, originEpoch=epoch, elapsed=now-start;
        long[] firstCount=initialCount,firstWall=initialWall;
        String metadata="label="+phase+"\nvalid="+(failure==null)+"\ninvalid_reason="+failure+"\norigin_epoch_ms="+originEpoch+"\nelapsed_ns="+elapsed+"\nfrustum="+wantedFrustum+"\nculling="+wantedCulling+"\nstaging="+wantedStaging+"\nposition="+position+"\nyaw="+yaw+"\npitch="+pitch+"\nrain="+rain+"\ntrace_overflow="+overflow+"\n";
        restore(); running=false; start=0; level=null; position=null;
        // Bounded copy is complete; disk IO runs after capture, never in a timed frame.
        Thread writer=new Thread(() -> {
            try {
                StringBuilder csv=new StringBuilder("end_epoch_ms,end_elapsed_ns,duration_ns,client_ticks,client_tick_wall_ns,world_render_wall_ns\n");
                for(int i=0;i<count;i++) {
                    long rel=trace[0][i]-origin;
                    csv.append(originEpoch+rel/1_000_000).append(',').append(rel).append(',').append(trace[1][i]).append(',').append(trace[2][i]).append(',').append(trace[3][i]).append(',').append(trace[4][i]).append('\n');
                }
                Files.writeString(Path.of(base+"-"+phase+"-frames.csv"),csv,StandardCharsets.UTF_8);
                StringBuilder metrics=new StringBuilder("metric,calls,wall_ns\n");
                for(int i=0;i<names.length;i++) metrics.append(names[i]).append(',').append(finalCount[i]-firstCount[i]).append(',').append(finalWall[i]-firstWall[i]).append('\n');
                Files.writeString(Path.of(base+"-"+phase+"-metrics.csv"),metrics,StandardCharsets.UTF_8);
                Files.writeString(Path.of(base+"-"+phase+"-metadata.txt"),metadata,StandardCharsets.UTF_8);
                status("capture_complete label="+phase+" frames="+count+" valid="+(failure==null));
            } catch(Exception error) { status("capture_write_error "+error); }
        },"Wachiland-bench-export"); writer.setDaemon(true); writer.setPriority(Thread.MIN_PRIORITY); writer.start();
    }
    private static long[] snapshot(String field) throws Exception {
        AtomicLong[] values=(AtomicLong[])counters.getField(field).get(null); long[] result=new long[values.length];
        for(int i=0;i<values.length;i++) result[i]=values[i].get(); return result;
    }
    private static void restore() {
        AndroidSupportConfig.frustumCullingQueries=oldFrustum;
        AndroidSupportConfig.stagedUploads=oldStaging;
        try { if(culling!=null) culling.setBoolean(null,oldCulling); } catch(Throwable ignored) { }
    }
    private static synchronized void status(String text) {
        System.out.println("[Wachiland live bench] "+text);
        try { if(base!=null) Files.writeString(Path.of(base+"-live-status.txt"),System.currentTimeMillis()+" "+text+"\n",StandardCharsets.UTF_8,java.nio.file.StandardOpenOption.CREATE,java.nio.file.StandardOpenOption.APPEND); } catch(Exception ignored) { }
    }
}
