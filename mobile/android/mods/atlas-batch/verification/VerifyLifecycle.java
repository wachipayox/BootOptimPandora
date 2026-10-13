import java.nio.file.*;
import java.util.*;
import java.util.concurrent.*;
import net.wachiland.android.compat.LifecycleBridge;
/** Host-only atomic protocol test, not a Minecraft/Android lifecycle simulation. */
public class VerifyLifecycle {
 static void check(boolean value,String message){if(!value)throw new AssertionError(message);}
 public static void main(String[] args)throws Exception{
  Path root=Files.createTempDirectory("wachiland-lifecycle-");Path state=root.resolve("state"),ready=root.resolve("ready");
  System.setProperty("wachiland.android.lifecycle.state",state.toString());System.setProperty("wachiland.android.lifecycle.ready",ready.toString());
  String session=UUID.randomUUID().toString();
  check(!LifecycleBridge.hidden(),"missing must be visible");
  Files.writeString(state,session+" hidden");Thread.sleep(510);check(LifecycleBridge.hidden(),"hidden absent");
  LifecycleBridge.markReady();check(Files.readString(ready).equals(session),"wrong completion session");
  Files.writeString(state,session+" visible");Thread.sleep(510);check(!LifecycleBridge.hidden(),"visible absent");
  Files.writeString(state,"bad hidden");Thread.sleep(510);check(!LifecycleBridge.hidden(),"malformed must be visible");
  Files.writeString(state,"x".repeat(129));Thread.sleep(510);check(!LifecycleBridge.hidden(),"oversized must be visible");
  Files.delete(state);Thread.sleep(510);check(!LifecycleBridge.hidden(),"removed must be visible");
  System.out.println("Lifecycle protocol: missing, hidden/visible, UUID completion, malformed/oversized/removal fail-open passed.");
 }
}
