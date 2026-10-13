package net.wachiland.android.compat;

import java.lang.instrument.Instrumentation;

/** Minimal JVM premain. GL/ASM initialization happens later in the launch wrapper. */
public final class EarlyCompatBootstrap {
    private static volatile Instrumentation instrumentation;
    public static void premain(String ignored, Instrumentation value) { instrumentation=value; }
    public static Object getInstrumentation() { return instrumentation; }
}
