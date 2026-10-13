package net.wachiland.android.compat;
import java.lang.invoke.MethodHandle;
import java.util.Map;
import java.nio.ByteBuffer;

/** Startup bootstrap facade: JDK types only, no GL, ASM or game dependencies. */
public final class EarlyBridge {
    private static volatile Map<String,MethodHandle> handles=Map.of();
    private static boolean reported;
    public static void install(Map<String,MethodHandle> value){handles=Map.copyOf(value);}
    private static MethodHandle h(String name){return handles.get(name);}
    private static void failure(Throwable error){if(!reported){reported=true;System.err.println("[Wachiland early compatibility] Bridge failure: "+error);}}
    public static void glMatrixMode(int value){try{h("glMatrixMode").invokeExact(value);}catch(Throwable e){failure(e);}}
    public static void glLoadIdentity(){try{h("glLoadIdentity").invokeExact();}catch(Throwable e){failure(e);}}
    public static void glOrtho(double l,double r,double b,double t,double near,double far){try{h("glOrtho").invokeExact(l,r,b,t,near,far);}catch(Throwable e){failure(e);}}
    public static void glPushMatrix(){try{h("glPushMatrix").invokeExact();}catch(Throwable e){failure(e);}}
    public static void glPopMatrix(){try{h("glPopMatrix").invokeExact();}catch(Throwable e){failure(e);}}
    public static void glScalef(float x,float y,float z){try{h("glScalef").invokeExact(x,y,z);}catch(Throwable e){failure(e);}}
    public static void glTranslatef(float x,float y,float z){try{h("glTranslatef").invokeExact(x,y,z);}catch(Throwable e){failure(e);}}
    public static void glColor4f(float r,float g,float b,float a){try{h("glColor4f").invokeExact(r,g,b,a);}catch(Throwable e){failure(e);}}
    public static void glBegin(int value){try{h("glBegin").invokeExact(value);}catch(Throwable e){failure(e);}}
    public static void glEnd(){try{h("glEnd").invokeExact();}catch(Throwable e){failure(e);}}
    public static void glVertex2f(float x,float y){try{h("glVertex2f").invokeExact(x,y);}catch(Throwable e){failure(e);}}
    public static void glTexCoord2f(float x,float y){try{h("glTexCoord2f").invokeExact(x,y);}catch(Throwable e){failure(e);}}
    public static void glEnable(int cap){try{h("glEnable").invokeExact(cap);}catch(Throwable e){failure(e);}}
    public static void glDisable(int cap){try{h("glDisable").invokeExact(cap);}catch(Throwable e){failure(e);}}
    public static boolean glIsEnabled(int cap){try{return (boolean)h("glIsEnabled").invokeExact(cap);}catch(Throwable e){failure(e);return false;}}
    public static void glEnableClientState(int cap){try{h("glEnableClientState").invokeExact(cap);}catch(Throwable e){failure(e);}}
    public static void glDisableClientState(int cap){try{h("glDisableClientState").invokeExact(cap);}catch(Throwable e){failure(e);}}
    public static void glVertexPointer(int size,int type,int stride,ByteBuffer data){try{h("glVertexPointer").invokeExact(size,type,stride,data);}catch(Throwable e){failure(e);}}
    public static void glDrawArrays(int mode,int first,int count){try{h("glDrawArrays").invokeExact(mode,first,count);}catch(Throwable e){failure(e);}}
    public static void beginFrame(){try{h("beginFrame").invokeExact();}catch(Throwable e){failure(e);}}
    public static void endFrame(){try{h("endFrame").invokeExact();}catch(Throwable e){failure(e);}}
    public static void close(){try{h("close").invokeExact();}catch(Throwable e){failure(e);}finally{handles=Map.of();}}
}
