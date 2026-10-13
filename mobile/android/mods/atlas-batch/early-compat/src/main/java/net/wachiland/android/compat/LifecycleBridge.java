package net.wachiland.android.compat;
import java.nio.file.*;
import java.nio.charset.StandardCharsets;
import java.util.concurrent.locks.LockSupport;
/** Bounded atomic-file protocol. Never retains game/Activity objects or runs GL off-thread. */
public final class LifecycleBridge {
 private static volatile boolean hidden;
 private static long nextPoll;
 private static String session;
 private static boolean ready;
 public static synchronized boolean hidden(){
  long now=System.nanoTime();if(now<nextPoll)return hidden;nextPoll=now+500_000_000L;
  String source=System.getProperty("wachiland.android.lifecycle.state","");
  if(source.isEmpty()){hidden=false;return false;}
  try{Path file=Path.of(source);if(!Files.isRegularFile(file)||Files.size(file)>128){hidden=false;return false;}
   String[] fields=Files.readString(file,StandardCharsets.UTF_8).trim().split(" ");
   if(fields.length!=2||!fields[0].matches("[0-9a-f-]{36}")||!(fields[1].equals("hidden")||fields[1].equals("visible"))){hidden=false;return false;}
   session=fields[0];hidden=fields[1].equals("hidden");
  }catch(Exception unknown){hidden=false;}
  return hidden;
 }
 public static boolean renderEarly(){if(!hidden())return true;LockSupport.parkNanos(1_000_000_000L);return false;}
 public static synchronized void markReady(){
  if(ready)return;hidden();if(session==null)return;String target=System.getProperty("wachiland.android.lifecycle.ready","");if(target.isEmpty())return;
  try{Path file=Path.of(target),temp=file.resolveSibling(file.getFileName()+".tmp");Files.writeString(temp,session,StandardCharsets.UTF_8);Files.move(temp,file,StandardCopyOption.REPLACE_EXISTING,StandardCopyOption.ATOMIC_MOVE);ready=true;System.out.println("[Wachiland lifecycle] game_ready");}
  catch(Exception failure){System.out.println("[Wachiland lifecycle] ready signal unavailable: "+failure.getClass().getSimpleName());}
 }
}
