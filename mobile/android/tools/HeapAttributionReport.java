import java.nio.file.Path;
import java.time.Duration;
import java.util.Comparator;
import java.util.HashMap;
import java.util.Map;
import jdk.jfr.consumer.RecordedClass;
import jdk.jfr.consumer.RecordedEvent;
import jdk.jfr.consumer.RecordedFrame;
import jdk.jfr.consumer.RecordedObject;
import jdk.jfr.consumer.RecordedStackTrace;
import jdk.jfr.consumer.RecordingFile;

/** Offline PC reader: no device attachment, no game execution, no huge JSON expansion. */
public class HeapAttributionReport {
    private static class Group {
        long count, weight, maxAgeMillis;
        void allocation(long bytes) { count++; weight += bytes; }
    }
    public static void main(String[] args) throws Exception {
        if (args.length != 1) throw new IllegalArgumentException("Usage: java HeapAttributionReport.java snapshot.jfr");
        Map<String, Group> allocations = new HashMap<>(), oldObjects = new HashMap<>();
        long events = 0, collections = 0;
        try (RecordingFile recording = new RecordingFile(Path.of(args[0]))) {
            while (recording.hasMoreEvents()) {
                RecordedEvent event = recording.readEvent();
                events++;
                switch (event.getEventType().getName()) {
                    case "jdk.ObjectAllocationSample": {
                        RecordedClass type = event.getClass("objectClass");
                        String key = type.getName() + "\n" + stack(event.getStackTrace());
                        allocations.computeIfAbsent(key, k -> new Group()).allocation(event.getLong("weight"));
                        break;
                    }
                    case "jdk.OldObjectSample": {
                        RecordedObject object = event.getValue("object");
                        RecordedClass type = object.getValue("type");
                        String key = type.getName() + "\n" + stack(event.getStackTrace());
                        Group group = oldObjects.computeIfAbsent(key, k -> new Group());
                        group.count++;
                        group.maxAgeMillis = Math.max(group.maxAgeMillis, event.getDuration("objectAge").toMillis());
                        break;
                    }
                    case "jdk.GarbageCollection": collections++; break;
                    default: break;
                }
            }
        }
        System.out.println("Events=" + events + "; GC events=" + collections);
        System.out.println("Allocation weight estimates allocated traffic, NOT retained or resident memory.");
        System.out.println("Old-object counts can repeat the same sample across snapshots; not exact class totals.");
        System.out.println("No root traversal was requested. Allocation sites are candidate owners, not proven retaining roots.");
        System.out.println("\nTOP ALLOCATION SITES (estimated bytes):");
        allocations.entrySet().stream().sorted(Comparator.comparingLong((Map.Entry<String, Group> e) -> e.getValue().weight).reversed())
                .limit(40).forEach(e -> System.out.println("\nweight=" + e.getValue().weight + "; samples=" + e.getValue().count + "\n" + e.getKey()));
        System.out.println("\nOLD OBJECT SAMPLE SITES:");
        oldObjects.entrySet().stream().sorted(Comparator.comparingLong((Map.Entry<String, Group> e) -> e.getValue().maxAgeMillis).reversed())
                .limit(60).forEach(e -> System.out.println("\nsample_events=" + e.getValue().count + "; max_age_ms=" + e.getValue().maxAgeMillis + "\n" + e.getKey()));
    }
    private static String stack(RecordedStackTrace trace) {
        if (trace == null) return "  [stack unavailable]";
        StringBuilder text = new StringBuilder();
        int shown = 0;
        for (RecordedFrame frame : trace.getFrames()) {
            if (++shown > 16) break;
            text.append("  ").append(frame.getMethod().getType().getName()).append('.').append(frame.getMethod().getName())
                    .append(':').append(frame.getLineNumber()).append('\n');
        }
        if (trace.isTruncated()) text.append("  [truncated stack]\n");
        return text.toString();
    }
}
