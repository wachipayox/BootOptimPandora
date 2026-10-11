/*
 * Copyright (c) 2018, 2020, Oracle and/or its affiliates. All rights reserved.
 * DO NOT ALTER OR REMOVE COPYRIGHT NOTICES OR THIS FILE HEADER.
 *
 * This code is free software; you can redistribute it and/or modify it
 * under the terms of the GNU General Public License version 2 only, as
 * published by the Free Software Foundation.  Oracle designates this
 * particular file as subject to the "Classpath" exception as provided
 * by Oracle in the LICENSE file that accompanied this code.
 *
 * This code is distributed in the hope that it will be useful, but WITHOUT
 * ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or
 * FITNESS FOR A PARTICULAR PURPOSE.  See the GNU General Public License
 * version 2 for more details (a copy is included in the LICENSE file that
 * accompanied this code).
 *
 * You should have received a copy of the GNU General Public License version
 * 2 along with this work; if not, write to the Free Software Foundation,
 * Inc., 51 Franklin St, Fifth Floor, Boston, MA 02110-1301 USA.
 *
 * Please contact Oracle, 500 Oracle Parkway, Redwood Shores, CA 94065 USA
 * or visit www.oracle.com if you need additional information or have any
 * questions.
 */

package jdk.nio.zipfs;

import java.io.IOException;
import java.io.UncheckedIOException;
import java.nio.channels.FileChannel;
import java.nio.ReadOnlyBufferException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardOpenOption;
import java.util.concurrent.atomic.AtomicLong;
import java.nio.ByteBuffer;
import java.nio.channels.ClosedChannelException;
import java.nio.channels.NonWritableChannelException;
import java.nio.channels.SeekableByteChannel;
import java.util.Arrays;
import java.util.concurrent.locks.ReadWriteLock;
import java.util.concurrent.locks.ReentrantReadWriteLock;

public class ByteArrayChannel implements SeekableByteChannel {

    private final ReadWriteLock rwlock = new ReentrantReadWriteLock();
    private byte buf[];
    // Android experimental disk snapshot for large nested ZIP backing channels.
    private FileChannel spill;
    private static final AtomicLong SPILLED = new AtomicLong();
    private static final AtomicLong CHANNELS = new AtomicLong();

    /*
     * The current position of this channel.
     */
    private int pos;

    /*
     * The index that is one greater than the last valid byte in the channel.
     */
    private int last;

    private boolean closed;
    private boolean readonly;

    /*
     * Creates a {@code ByteArrayChannel} with size {@code sz}.
     */
    ByteArrayChannel(int sz, boolean readonly) {
        this.buf = new byte[sz];
        this.pos = this.last = 0;
        this.readonly = readonly;
    }

    /*
     * Creates a ByteArrayChannel with its 'pos' at 0 and its 'last' at buf's end.
     * Note: no defensive copy of the 'buf', used directly.
     */
    ByteArrayChannel(byte[] buf, boolean readonly) {
        this.buf = buf;
        this.pos = 0;
        this.last = buf.length;
        this.readonly = readonly;
        if (readonly && buf.length >= 262144 && eligibleNestedZip()) trySpill(buf);
    }

    private static boolean eligibleNestedZip() {
        if (!Boolean.getBoolean("wachiland.zipfs.spill")) return false;
        boolean desktopTest=Boolean.getBoolean("wachiland.zipfs.desktopTest");
        if (!desktopTest && !("Linux".equals(System.getProperty("os.name"))
                && "aarch64".equals(System.getProperty("os.arch"))
                && System.getProperty("java.version", "").startsWith("21.0.1"))) return false;
        return StackWalker.getInstance().walk(frames -> frames.limit(32).anyMatch(frame ->
                frame.getClassName().equals("jdk.nio.zipfs.ZipFileSystem") && frame.getMethodName().equals("<init>")));
    }

    private void trySpill(byte[] bytes) {
        FileChannel candidate=null;
        Path path=null;
        try {
            path=Files.createTempFile("wachiland-zipfs-", ".bin");
            candidate=FileChannel.open(path,StandardOpenOption.READ,StandardOpenOption.WRITE);
            ByteBuffer source=ByteBuffer.wrap(bytes);
            while (source.hasRemaining()) candidate.write(source);
            // Linux unlinks the private snapshot immediately: process death cannot leave cache files behind.
            Files.delete(path);
            path=null;
            spill=candidate;
            buf=null;
            long count=CHANNELS.incrementAndGet();
            long total=SPILLED.addAndGet(bytes.length);
            if (count<=4 || count%32==0) System.err.println("[Wachiland ZIP spill] guardDepth=32 channels="+count+" snapshot_bytes="+total);
        } catch (IOException | SecurityException failure) {
            // Any temp filesystem failure retains the original authoritative array.
            if (candidate!=null) try { candidate.close(); } catch (IOException ignored) { }
            if (path!=null) try { Files.deleteIfExists(path); } catch (IOException | SecurityException ignored) { }
        }
    }

    @Override
    public boolean isOpen() {
        return !closed;
    }

    @Override
    public long position() throws IOException {
        beginRead();
        try {
            ensureOpen();
            return pos;
        } finally {
            endRead();
        }
    }

    @Override
    public SeekableByteChannel position(long pos) throws IOException {
        beginWrite();
        try {
            ensureOpen();
            if (pos < 0 || pos >= Integer.MAX_VALUE)
                throw new IllegalArgumentException("Illegal position " + pos);
            this.pos = Math.min((int)pos, last);
            return this;
        } finally {
            endWrite();
        }
    }

    @Override
    public int read(ByteBuffer dst) throws IOException {
        beginWrite();
        try {
            ensureOpen();
            if (pos == last)
                return -1;
            int n = Math.min(dst.remaining(), last - pos);
            if (spill==null) dst.put(buf, pos, n);
            else {
                if (dst.isReadOnly()) throw new ReadOnlyBufferException();
                int previousLimit=dst.limit();
                dst.limit(dst.position()+n);
                try {
                    int copied=0;
                    while (copied<n) {
                        int got=spill.read(dst,(long)pos+copied);
                        if (got<=0) throw new IOException("Incomplete private ZIP snapshot");
                        copied+=got;
                    }
                } finally { dst.limit(previousLimit); }
            }
            pos += n;
            return n;
        } finally {
            endWrite();
        }
    }

    @Override
    public SeekableByteChannel truncate(long size) throws IOException {
        if (readonly)
            throw new NonWritableChannelException();
        ensureOpen();
        throw new UnsupportedOperationException();
    }

    @Override
    public int write(ByteBuffer src) throws IOException {
        if (readonly)
            throw new NonWritableChannelException();
        beginWrite();
        try {
            ensureOpen();
            int n = src.remaining();
            ensureCapacity(pos + n);
            src.get(buf, pos, n);
            pos += n;
            if (pos > last) {
                last = pos;
            }
            return n;
        } finally {
            endWrite();
        }
    }

    @Override
    public long size() throws IOException {
        beginRead();
        try {
            ensureOpen();
            return last;
        } finally {
            endRead();
        }
    }

    @Override
    public void close() throws IOException {
        if (closed)
            return;
        beginWrite();
        try {
            closed = true;
            buf = null;
            pos = 0;
            last = 0;
            if (spill!=null) {
                FileChannel channel=spill;
                spill=null;
                channel.close();
            }
        } finally {
            endWrite();
        }
    }

    /**
     * Creates a newly allocated byte array. Its size is the current
     * size of this channel and the valid contents of the buffer
     * have been copied into it.
     *
     * @return the current contents of this channel, as a byte array.
     */
    public byte[] toByteArray() {
        beginRead();
        try {
            // avoid copy if last == bytes.length?
            if (spill==null) return Arrays.copyOf(buf, last);
            byte[] copy=new byte[last];
            ByteBuffer destination=ByteBuffer.wrap(copy);
            try {
                while (destination.hasRemaining()) {
                    int got=spill.read(destination,destination.position());
                    if (got<=0) throw new IOException("Incomplete private ZIP snapshot");
                }
            } catch (IOException failure) { throw new UncheckedIOException(failure); }
            return copy;
        } finally {
            endRead();
        }
    }

    private void ensureOpen() throws IOException {
        if (closed)
            throw new ClosedChannelException();
    }

    final void beginWrite() {
        rwlock.writeLock().lock();
    }

    final void endWrite() {
        rwlock.writeLock().unlock();
    }

    private final void beginRead() {
        rwlock.readLock().lock();
    }

    private final void endRead() {
        rwlock.readLock().unlock();
    }

    private void ensureCapacity(int minCapacity) {
        // overflow-conscious code
        if (minCapacity - buf.length > 0) {
            grow(minCapacity);
        }
    }

    /**
     * The maximum size of array to allocate.
     * Some VMs reserve some header words in an array.
     * Attempts to allocate larger arrays may result in
     * OutOfMemoryError: Requested array size exceeds VM limit
     */
    private static final int MAX_ARRAY_SIZE = Integer.MAX_VALUE - 8;

    /**
     * Increases the capacity to ensure that it can hold at least the
     * number of elements specified by the minimum capacity argument.
     *
     * @param minCapacity the desired minimum capacity
     */
    private void grow(int minCapacity) {
        // overflow-conscious code
        int oldCapacity = buf.length;
        int newCapacity = oldCapacity << 1;
        if (newCapacity - minCapacity < 0)
            newCapacity = minCapacity;
        if (newCapacity - MAX_ARRAY_SIZE > 0)
            newCapacity = hugeCapacity(minCapacity);
        buf = Arrays.copyOf(buf, newCapacity);
    }

    private static int hugeCapacity(int minCapacity) {
        if (minCapacity < 0) // overflow
            throw new OutOfMemoryError("Required length exceeds implementation limit");
        return (minCapacity > MAX_ARRAY_SIZE) ?
            Integer.MAX_VALUE :
            MAX_ARRAY_SIZE;
    }
}
