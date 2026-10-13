package net.wachiland.android.compat;

import java.nio.ByteBuffer;
import java.nio.FloatBuffer;
import java.util.concurrent.locks.LockSupport;
import org.lwjgl.opengl.GL11;
import org.lwjgl.opengl.GL20;
import org.lwjgl.opengl.GL30;
import org.lwjgl.system.MemoryUtil;

/** Restricted legacy-call adapter, used only by the pinned external Drippy class.
 * No game code/assets copied; original provider owns layout, textures and progress.
 */
public final class EarlyGl {
    private static boolean attempted, core, disabled;
    private static Thread owner;
    private static int program,vao,vbo,oldProgram,oldVao,oldVbo;
    private static int colorLocation,textureLocation;
    private static FloatBuffer stream;
    private static final float[] color={1,1,1,1},quad=new float[64],matrix={1,1,0,0};
    private static final float[][] stack=new float[16][4];
    private static int depth,mode=5888,primitive,vertices,stride,pointerStart;
    private static double left=-1,right=1,bottom=-1,top=1;
    private static float u,v;
    private static boolean textured;
    private static ByteBuffer pointer;
    private static long frameStart,frames;
    private EarlyGl() { }
    private static boolean use() {
        if (disabled) return false;
        if (attempted) return core && Thread.currentThread()==owner;
        attempted=true;
        try {
            String renderer=GL11.glGetString(GL11.GL_VERSION);
            if (renderer==null || !renderer.contains("MobileGlues")) return false;
            owner=Thread.currentThread();
            oldProgram=GL11.glGetInteger(GL20.GL_CURRENT_PROGRAM);oldVao=GL11.glGetInteger(GL30.GL_VERTEX_ARRAY_BINDING);oldVbo=GL11.glGetInteger(34964);
            int vertex=shader(GL20.GL_VERTEX_SHADER,"#version 150\nin vec2 Position; in vec2 UV; out vec2 texCoord; void main(){gl_Position=vec4(Position,0.0,1.0);texCoord=UV;}");
            int fragment=shader(GL20.GL_FRAGMENT_SHADER,"#version 150\nin vec2 texCoord; out vec4 outColor; uniform vec4 Tint; uniform sampler2D Image; uniform int Textured; void main(){outColor=Tint*(Textured!=0?texture(Image,texCoord):vec4(1.0));}");
            program=GL20.glCreateProgram();GL20.glAttachShader(program,vertex);GL20.glAttachShader(program,fragment);
            GL20.glBindAttribLocation(program,0,"Position");GL20.glBindAttribLocation(program,1,"UV");GL30.glBindFragDataLocation(program,0,"outColor");GL20.glLinkProgram(program);
            GL20.glDeleteShader(vertex);GL20.glDeleteShader(fragment);
            if(GL20.glGetProgrami(program,GL20.GL_LINK_STATUS)==0) throw new IllegalStateException(GL20.glGetProgramInfoLog(program));
            colorLocation=GL20.glGetUniformLocation(program,"Tint");textureLocation=GL20.glGetUniformLocation(program,"Textured");
            vao=GL30.glGenVertexArrays();vbo=org.lwjgl.opengl.GL15.glGenBuffers();stream=MemoryUtil.memAllocFloat(24576);
            core=true;System.out.println("[Wachiland early compatibility] MobileGlues core drawing ready; loading cap=20 FPS");
        } catch(Throwable failure) { disabled=true;core=false;System.out.println("[Wachiland early compatibility] Core drawing failed; original fallback: "+failure);cleanup(); }
        return core;
    }
    private static int shader(int type,String source) {
        int s=GL20.glCreateShader(type);GL20.glShaderSource(s,source);GL20.glCompileShader(s);
        if(GL20.glGetShaderi(s,GL20.GL_COMPILE_STATUS)==0) {String log=GL20.glGetShaderInfoLog(s);GL20.glDeleteShader(s);throw new IllegalStateException(log);}
        return s;
    }
    public static void beginFrame() {frameStart=System.nanoTime();}
    public static void endFrame() {
        if (!core || Thread.currentThread()!=owner) return;
        long remaining=50_000_000L-(System.nanoTime()-frameStart);
        if(remaining>0) LockSupport.parkNanos(remaining);
        frames++;
    }
    public static void glMatrixMode(int value) {if(!use()){GL11.glMatrixMode(value);return;}mode=value;}
    public static void glLoadIdentity() {
        if(!use()){GL11.glLoadIdentity();return;}
        if(mode==5888){matrix[0]=matrix[1]=1;matrix[2]=matrix[3]=0;}
        else {left=bottom=-1;right=top=1;}
    }
    public static void glOrtho(double l,double r,double b,double t,double near,double far) {
        if(!use()){GL11.glOrtho(l,r,b,t,near,far);return;}left=l;right=r;bottom=b;top=t;
    }
    public static void glPushMatrix(){if(!use()){GL11.glPushMatrix();return;}if(depth>=stack.length)throw new IllegalStateException("Unexpected matrix depth");System.arraycopy(matrix,0,stack[depth++],0,4);}
    public static void glPopMatrix(){if(!use()){GL11.glPopMatrix();return;}if(depth==0)throw new IllegalStateException("Unexpected matrix underflow");System.arraycopy(stack[--depth],0,matrix,0,4);}
    public static void glScalef(float x,float y,float z){if(!use()){GL11.glScalef(x,y,z);return;}matrix[0]*=x;matrix[1]*=y;}
    public static void glTranslatef(float x,float y,float z){if(!use()){GL11.glTranslatef(x,y,z);return;}matrix[2]+=matrix[0]*x;matrix[3]+=matrix[1]*y;}
    public static void glColor4f(float r,float g,float b,float a){if(!use()){GL11.glColor4f(r,g,b,a);return;}color[0]=r;color[1]=g;color[2]=b;color[3]=a;}
    public static void glEnable(int cap){if(use() && cap==3553)textured=true;else GL11.glEnable(cap);}
    public static void glDisable(int cap){if(use() && cap==3553)textured=false;else GL11.glDisable(cap);}
    public static boolean glIsEnabled(int cap){return use() && cap==3553?textured:GL11.glIsEnabled(cap);}
    public static void glEnableClientState(int cap){if(!use())GL11.glEnableClientState(cap);}
    public static void glDisableClientState(int cap){if(!use())GL11.glDisableClientState(cap);}
    public static void glBegin(int value){if(!use()){GL11.glBegin(value);return;}primitive=value;vertices=0;}
    public static void glTexCoord2f(float x,float y){if(!use()){GL11.glTexCoord2f(x,y);return;}u=x;v=y;}
    public static void glVertex2f(float x,float y){
        if(!use()){GL11.glVertex2f(x,y);return;}
        if(vertices>=quad.length/4)throw new IllegalStateException("Unexpected primitive size");
        int i=vertices++*4;quad[i]=x;quad[i+1]=y;quad[i+2]=u;quad[i+3]=v;
    }
    public static void glEnd(){
        if(!use()){GL11.glEnd();return;}stream.clear();
        if(primitive==7){for(int i=0;i+3<vertices;i+=4){put(quad,i);put(quad,i+1);put(quad,i+2);put(quad,i);put(quad,i+2);put(quad,i+3);}emit(4);}
        else {for(int i=0;i<vertices;i++)put(quad,i);emit(primitive);}
    }
    private static void put(float[] a,int index){int i=index*4;put(a[i],a[i+1],a[i+2],a[i+3]);}
    private static void put(float x,float y,float tu,float tv){
        stream.put((float)((2*(matrix[0]*x+matrix[2])-left-right)/(right-left)));
        stream.put((float)((2*(matrix[1]*y+matrix[3])-bottom-top)/(top-bottom)));
        stream.put(tu).put(tv);
    }
    public static void glVertexPointer(int size,int type,int byteStride,ByteBuffer data){
        if(!use()){GL11.glVertexPointer(size,type,byteStride,data);return;}
        if(size!=2 || type!=5126 || byteStride!=16)throw new IllegalStateException("Unexpected text vertex layout");
        pointer=data;stride=byteStride;pointerStart=data.position();
    }
    public static void glDrawArrays(int drawMode,int first,int count){
        if(!use()){GL11.glDrawArrays(drawMode,first,count);return;}
        if(drawMode!=7 || pointer==null || count%4!=0)throw new IllegalStateException("Unexpected text primitive");
        stream.clear();
        for(int i=first;i<first+count;i+=4){
            if(stream.remaining()<24){emit(4);stream.clear();}
            putPointer(i);putPointer(i+1);putPointer(i+2);putPointer(i);putPointer(i+2);putPointer(i+3);
        }
        emit(4);pointer=null;
    }
    private static void putPointer(int i){int at=pointerStart+i*stride;put(pointer.getFloat(at),pointer.getFloat(at+4),0,0);}
    private static void emit(int drawMode){
        if(stream.position()==0)return;
        int previousProgram=GL11.glGetInteger(GL20.GL_CURRENT_PROGRAM),previousVao=GL11.glGetInteger(GL30.GL_VERTEX_ARRAY_BINDING),previousVbo=GL11.glGetInteger(34964);
        try {
            GL20.glUseProgram(program);GL30.glBindVertexArray(vao);org.lwjgl.opengl.GL15.glBindBuffer(34962,vbo);
            stream.flip();org.lwjgl.opengl.GL15.glBufferData(34962,stream,35040);
            GL20.glEnableVertexAttribArray(0);GL20.glEnableVertexAttribArray(1);
            GL20.glVertexAttribPointer(0,2,5126,false,16,0L);GL20.glVertexAttribPointer(1,2,5126,false,16,8L);
            GL20.glUniform4f(colorLocation,color[0],color[1],color[2],color[3]);GL20.glUniform1i(textureLocation,textured?1:0);GL20.glUniform1i(GL20.glGetUniformLocation(program,"Image"),0);
            GL11.glDrawArrays(drawMode,0,stream.remaining()/4);
        } finally {GL30.glBindVertexArray(previousVao);org.lwjgl.opengl.GL15.glBindBuffer(34962,previousVbo);GL20.glUseProgram(previousProgram);}
    }
    public static void close(){if(Thread.currentThread()==owner){System.out.println("[Wachiland early compatibility] Handoff complete; frames="+frames);cleanup();disabled=true;core=false;}}
    private static void cleanup(){
        if(Thread.currentThread()!=owner)return;
        try{if(program!=0)GL20.glDeleteProgram(program);if(vao!=0)GL30.glDeleteVertexArrays(vao);if(vbo!=0)org.lwjgl.opengl.GL15.glDeleteBuffers(vbo);if(stream!=null)MemoryUtil.memFree(stream);}catch(Throwable ignored){}
        program=vao=vbo=0;stream=null;pointer=null;
    }
}
