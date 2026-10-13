package net.wachiland.atlasbatch;
import java.lang.invoke.*;
/** Optional bootstrap bridge, no hard native or launcher dependency. */
public final class BackgroundMode {
 private static MethodHandle hidden,ready;private static boolean checked;
 private static synchronized void setup(){if(checked)return;checked=true;
  try{Class<?> type=Class.forName("net.wachiland.android.compat.LifecycleBridge",false,null);hidden=MethodHandles.publicLookup().findStatic(type,"hidden",MethodType.methodType(boolean.class));ready=MethodHandles.publicLookup().findStatic(type,"markReady",MethodType.methodType(void.class));}catch(ReflectiveOperationException unknown){}
 }
 public static boolean hidden(){if(!AndroidSupportConfig.ANDROID||!AndroidSupportConfig.enabled)return false;setup();try{return hidden!=null&&(boolean)hidden.invokeExact();}catch(Throwable unavailable){return false;}}
 public static void ready(){setup();try{if(ready!=null)ready.invokeExact();}catch(Throwable unavailable){}}
}
