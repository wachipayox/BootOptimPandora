package net.wachiland.atlasbatch;

import com.mojang.blaze3d.platform.GlStateManager;
import com.mojang.blaze3d.systems.RenderSystem;
import com.mojang.logging.LogUtils;
import java.nio.ByteBuffer;
import net.minecraft.client.Minecraft;
import net.minecraft.client.renderer.texture.SpriteContents;
import net.minecraft.client.renderer.texture.TextureAtlas;
import net.neoforged.fml.ModList;
import org.lwjgl.opengl.GL;
import org.lwjgl.opengl.GL11C;
import org.lwjgl.opengl.GL15C;
import org.lwjgl.opengl.GL21C;
import org.lwjgl.opengl.GL30C;
import org.lwjgl.system.MemoryUtil;

/** Experimental immediate GPU copy, never a deferred pointer or a second full atlas. */
public final class StagedAtlasUpload {
    private static final ThreadLocal<Boolean> SCOPE=ThreadLocal.withInitial(() -> false);
    private static boolean tested, supported, failed;
    private static long uploads, fallbacks;
    private static int verifiedMips;
    private static final int[] STORES={3317,3314,3315,3316,3333,3330,3331,3332};

    public static boolean enter(SpriteContents sprite,TextureAtlas atlas) {
        boolean previous=SCOPE.get();
        SCOPE.set(false);
        if (!AndroidSupportConfig.enabled || !AndroidSupportConfig.ANDROID || !AndroidSupportConfig.stagedUploads
            || failed || !RenderSystem.isOnRenderThread() || !AtlasBatchMixinPlugin.ready()) return previous;
        Minecraft mc=Minecraft.getInstance();
        // Unknown custom sprite/atlas implementations and Kerria's upload ownership stay stock.
        if (sprite.getClass()!=SpriteContents.class || atlas==null || atlas.getClass()!=TextureAtlas.class
            || mc.level==null || mc.screen!=null || mc.getOverlay()!=null || ModList.get().isLoaded("kerria")) return previous;
        int unit=GlStateManager._getActiveTexture()-33984;
        SCOPE.set(unit>=0 && unit<GlStateManager.TEXTURES.length && GlStateManager.TEXTURES[unit].binding==atlas.getId());
        return previous;
    }
    public static void leave(boolean previous) { SCOPE.set(previous); }
    public static String status() { return "staged_uploads="+AndroidSupportConfig.stagedUploads+" self_test="+tested+" supported="+supported+" failed="+failed+" verified_mips="+verifiedMips+" uploads="+uploads+" fallbacks="+fallbacks; }
    public static void invalidatePixels() { if (RenderSystem.isOnRenderThread()) verifiedMips=0; }

    public static boolean upload(int target,int level,int x,int y,int width,int height,int format,int type,long pointer) {
        if (!SCOPE.get() || !RenderSystem.isOnRenderThread() || failed || target!=GL11C.GL_TEXTURE_2D || level<0 || level>15 || width<=0 || height<=0
            || width>256 || height>256 || x<0 || y<0 || format!=GL11C.GL_RGBA || type!=GL11C.GL_UNSIGNED_BYTE || pointer==0) return false;
        if (!tested) selfTest();
        if (!supported || failed) return false;
        // Client memory only. With a PBO the pointer is an offset, not an address.
        if (GL11C.glGetInteger(GL21C.GL_PIXEL_UNPACK_BUFFER_BINDING)!=0) return false;
        int internal=GL11C.glGetTexLevelParameteri(target,level,GL11C.GL_TEXTURE_INTERNAL_FORMAT);
        int atlasWidth=GL11C.glGetTexLevelParameteri(target,level,GL11C.GL_TEXTURE_WIDTH);
        int atlasHeight=GL11C.glGetTexLevelParameteri(target,level,GL11C.GL_TEXTURE_HEIGHT);
        if (internal!=GL11C.GL_RGBA8 || width>atlasWidth || height>atlasHeight || x>atlasWidth-width || y>atlasHeight-height) return false;
        // This prototype deliberately checks errors at the native boundary. A pre-existing
        // error disables it; it never treats that error as evidence that the copy failed.
        if (GL11C.glGetError()!=GL11C.GL_NO_ERROR) { disable("pre-existing GL error"); return false; }
        State saved=new State();
        int texture=0, framebuffer=0;
        boolean complete=false;
        try {
            texture=GL11C.glGenTextures(); framebuffer=GL30C.glGenFramebuffers();
            GL11C.glBindTexture(target,texture);
            allocate(width,height);
            GL30C.glBindFramebuffer(GL30C.GL_READ_FRAMEBUFFER,framebuffer);
            GL30C.glFramebufferTexture2D(GL30C.GL_READ_FRAMEBUFFER,GL30C.GL_COLOR_ATTACHMENT0,target,texture,0);
            if (GL30C.glCheckFramebufferStatus(GL30C.GL_READ_FRAMEBUFFER)!=GL30C.GL_FRAMEBUFFER_COMPLETE) { disable("staging framebuffer incomplete"); return false; }
            GL11C.glReadBuffer(GL30C.GL_COLOR_ATTACHMENT0);
            // Pixel store was set by NativeImage and remains intact, including frame offsets.
            GL11C.glTexSubImage2D(target,0,0,0,width,height,format,type,pointer);
            GL11C.glBindTexture(target,saved.texture);
            GL11C.glCopyTexSubImage2D(target,level,x,y,0,0,width,height);
            complete=GL11C.glGetError()==GL11C.GL_NO_ERROR;
            if (!complete) disable("upload/copy GL error");
            else if ((verifiedMips & (1<<level))==0) complete=verifyPixels(saved.texture,level,x,y,width,height,pointer);
        } catch (RuntimeException | LinkageError e) {
            disable("native route unavailable: "+e.getClass().getSimpleName());
        } finally {
            saved.restore();
            if (framebuffer!=0) GL30C.glDeleteFramebuffers(framebuffer);
            if (texture!=0) GL11C.glDeleteTextures(texture);
        }
        if (complete) uploads++; else fallbacks++;
        return complete; // Caller repeats original CPU upload if copying failed.
    }

    private static void allocate(int width,int height) {
        GL11C.glTexParameteri(GL11C.GL_TEXTURE_2D,GL11C.GL_TEXTURE_MIN_FILTER,GL11C.GL_NEAREST);
        GL11C.glTexParameteri(GL11C.GL_TEXTURE_2D,GL11C.GL_TEXTURE_MAG_FILTER,GL11C.GL_NEAREST);
        GL11C.glTexImage2D(GL11C.GL_TEXTURE_2D,0,GL11C.GL_RGBA8,width,height,0,GL11C.GL_RGBA,GL11C.GL_UNSIGNED_BYTE,0L);
    }
    private static void disable(String reason) {
        failed=true; supported=false;
        LogUtils.getLogger().warn("[Wachiland Android support] Staging disabled; stock upload: {}",reason);
    }

    /** Check the first real upload of each mip against the caller's CPU rectangle. */
    private static boolean verifyPixels(int destination,int level,int x,int y,int width,int height,long pointer) {
        int rowLength=GL11C.glGetInteger(GL11C.GL_UNPACK_ROW_LENGTH);
        int skipRows=GL11C.glGetInteger(GL11C.GL_UNPACK_SKIP_ROWS);
        int skipPixels=GL11C.glGetInteger(GL11C.GL_UNPACK_SKIP_PIXELS);
        int alignment=GL11C.glGetInteger(GL11C.GL_UNPACK_ALIGNMENT);
        if (rowLength<0 || skipRows<0 || skipPixels<0 || !(alignment==1 || alignment==2 || alignment==4 || alignment==8)) {
            disable("unknown unpack layout"); return false;
        }
        long rowBytes=(long)(rowLength==0 ? width : rowLength)*4;
        long stride=(rowBytes+alignment-1)/alignment*alignment;
        long first=(long)skipRows*stride+(long)skipPixels*4;
        int packBuffer=GL11C.glGetInteger(GL21C.GL_PIXEL_PACK_BUFFER_BINDING);
        int[] packNames={GL11C.GL_PACK_ALIGNMENT,GL11C.GL_PACK_ROW_LENGTH,GL11C.GL_PACK_SKIP_ROWS,GL11C.GL_PACK_SKIP_PIXELS};
        int[] packValues=new int[4];
        for(int i=0;i<4;i++) packValues[i]=GL11C.glGetInteger(packNames[i]);
        ByteBuffer actual=MemoryUtil.memAlloc(width*height*4);
        boolean equal=false;
        try {
            GL15C.glBindBuffer(GL21C.GL_PIXEL_PACK_BUFFER,0);
            for(int i=0;i<4;i++) GL11C.glPixelStorei(packNames[i],i==0 ? 1 : 0);
            GL30C.glFramebufferTexture2D(GL30C.GL_READ_FRAMEBUFFER,GL30C.GL_COLOR_ATTACHMENT0,GL11C.GL_TEXTURE_2D,destination,level);
            if (GL30C.glCheckFramebufferStatus(GL30C.GL_READ_FRAMEBUFFER)!=GL30C.GL_FRAMEBUFFER_COMPLETE) return false;
            GL11C.glReadPixels(x,y,width,height,GL11C.GL_RGBA,GL11C.GL_UNSIGNED_BYTE,actual);
            equal=GL11C.glGetError()==GL11C.GL_NO_ERROR;
            for(int row=0;equal && row<height;row++) for(int col=0;col<width*4;col++) {
                if (actual.get(row*width*4+col)!=MemoryUtil.memGetByte(pointer+first+row*stride+col)) { equal=false; break; }
            }
            if (equal) {
                verifiedMips |= 1<<level;
                LogUtils.getLogger().info("[Wachiland Android support] Real mip pixels passed level={} size={}x{} row_length={} skip_rows={} skip_pixels={}",level,width,height,rowLength,skipRows,skipPixels);
            }
        } finally {
            GL15C.glBindBuffer(GL21C.GL_PIXEL_PACK_BUFFER,packBuffer);
            for(int i=0;i<4;i++) GL11C.glPixelStorei(packNames[i],packValues[i]);
            MemoryUtil.memFree(actual);
            if (!equal) disable("real upload pixel verification failed");
        }
        return equal;
    }

    private static void selfTest() {
        tested=true;
        var caps=GL.getCapabilities();
        if (!caps.OpenGL30 || caps.glCopyTexSubImage2D==0 || caps.glReadPixels==0 || caps.glFramebufferTexture2D==0) return;
        if (GL11C.glGetError()!=GL11C.GL_NO_ERROR) { disable("pre-existing GL error before self-test"); return; }
        State saved=new State();
        int[] stores=new int[STORES.length];
        for (int i=0;i<stores.length;i++) stores[i]=GL11C.glGetInteger(STORES[i]);
        int unpackBuffer=GL11C.glGetInteger(GL21C.GL_PIXEL_UNPACK_BUFFER_BINDING);
        int packBuffer=GL11C.glGetInteger(GL21C.GL_PIXEL_PACK_BUFFER_BINDING);
        int source=0,destination=0,framebuffer=0;
        ByteBuffer expected=MemoryUtil.memAlloc(64),actual=MemoryUtil.memAlloc(16);
        try {
            for(int i=0;i<64;i++) expected.put(i,(byte)(17+i*13));
            GL15C.glBindBuffer(GL21C.GL_PIXEL_UNPACK_BUFFER,0);
            GL15C.glBindBuffer(GL21C.GL_PIXEL_PACK_BUFFER,0);
            for(int i=0;i<STORES.length;i++) GL11C.glPixelStorei(STORES[i],i==0 || i==4 ? 1 : 0);
            source=GL11C.glGenTextures(); destination=GL11C.glGenTextures(); framebuffer=GL30C.glGenFramebuffers();
            GL11C.glBindTexture(GL11C.GL_TEXTURE_2D,source); allocate(2,2);
            GL11C.glPixelStorei(GL11C.GL_UNPACK_ROW_LENGTH,4);
            GL11C.glPixelStorei(GL11C.GL_UNPACK_SKIP_ROWS,1);
            GL11C.glPixelStorei(GL11C.GL_UNPACK_SKIP_PIXELS,1);
            GL11C.glTexSubImage2D(GL11C.GL_TEXTURE_2D,0,0,0,2,2,GL11C.GL_RGBA,GL11C.GL_UNSIGNED_BYTE,expected);
            GL30C.glBindFramebuffer(GL30C.GL_READ_FRAMEBUFFER,framebuffer);
            GL30C.glFramebufferTexture2D(GL30C.GL_READ_FRAMEBUFFER,GL30C.GL_COLOR_ATTACHMENT0,GL11C.GL_TEXTURE_2D,source,0);
            if (GL30C.glCheckFramebufferStatus(GL30C.GL_READ_FRAMEBUFFER)!=GL30C.GL_FRAMEBUFFER_COMPLETE) return;
            GL11C.glReadBuffer(GL30C.GL_COLOR_ATTACHMENT0);
            GL11C.glBindTexture(GL11C.GL_TEXTURE_2D,destination); allocate(8,8);
            GL11C.glTexImage2D(GL11C.GL_TEXTURE_2D,1,GL11C.GL_RGBA8,4,4,0,GL11C.GL_RGBA,GL11C.GL_UNSIGNED_BYTE,0L);
            GL11C.glCopyTexSubImage2D(GL11C.GL_TEXTURE_2D,1,1,1,0,0,2,2);
            GL30C.glFramebufferTexture2D(GL30C.GL_READ_FRAMEBUFFER,GL30C.GL_COLOR_ATTACHMENT0,GL11C.GL_TEXTURE_2D,destination,1);
            if (GL30C.glCheckFramebufferStatus(GL30C.GL_READ_FRAMEBUFFER)!=GL30C.GL_FRAMEBUFFER_COMPLETE) return;
            GL11C.glReadPixels(1,1,2,2,GL11C.GL_RGBA,GL11C.GL_UNSIGNED_BYTE,actual);
            supported=GL11C.glGetError()==GL11C.GL_NO_ERROR;
            for(int row=0;row<2;row++) for(int col=0;col<8;col++) supported &= expected.get((row+1)*16+4+col)==actual.get(row*8+col);
        } catch (RuntimeException | LinkageError e) {
            disable("pixel self-test unavailable: "+e.getClass().getSimpleName());
        } finally {
            saved.restore();
            GL15C.glBindBuffer(GL21C.GL_PIXEL_UNPACK_BUFFER,unpackBuffer);
            GL15C.glBindBuffer(GL21C.GL_PIXEL_PACK_BUFFER,packBuffer);
            for(int i=0;i<STORES.length;i++) GL11C.glPixelStorei(STORES[i],stores[i]);
            if(framebuffer!=0) GL30C.glDeleteFramebuffers(framebuffer);
            if(source!=0) GL11C.glDeleteTextures(source);
            if(destination!=0) GL11C.glDeleteTextures(destination);
            MemoryUtil.memFree(expected); MemoryUtil.memFree(actual);
            LogUtils.getLogger().info("[Wachiland Android support] Staging pixel self-test passed={}",supported);
        }
    }
    private static final class State {
        final int texture=GL11C.glGetInteger(GL11C.GL_TEXTURE_BINDING_2D);
        final int readFramebuffer=GL11C.glGetInteger(GL30C.GL_READ_FRAMEBUFFER_BINDING);
        void restore() {
            GL11C.glBindTexture(GL11C.GL_TEXTURE_2D,texture);
            GL30C.glBindFramebuffer(GL30C.GL_READ_FRAMEBUFFER,readFramebuffer);
        }
    }
}
