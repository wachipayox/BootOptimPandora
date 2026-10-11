# Experimental Android ZIP backing storage

Not production. Isolated opt-in memory candidate for the embedded OpenJDK
21.0.1 runtime, source commit `060c4f7589e7`, used by FCL on the physical Vivo.
The unmodified upstream ByteArrayChannel source is retained for audit;
COPYING contains the OpenJDK license and Classpath exception.

The read-only channel returned while a nested ZipFileSystem constructor is on
the stack normally retains a complete decompressed byte array. This candidate
stores that already-read snapshot in a private temporary file for arrays at
least 256 KiB. It keeps fresh filesystems/readers and loader callbacks. It does
not deduplicate mods, change resolution, close live ZIP filesystems, or remove
resources. Writable channels and other call sites remain stock.

The full initial array still exists during creation: this candidate targets
retained backing bytes, not the original decompression peak. File pages can be
reclaimed, but the actual system headroom and startup cost must be measured.
Snapshot byte totals are not measured RSS or end-to-end memory savings.

Successful Linux snapshots are immediately unlinked while their file
descriptor remains open. Closing the stock channel closes that descriptor;
process exit also cleans it. A temp-file failure falls back to the original
array. Original clamped seek, zero-capacity/EOF read, locking and read-only
write/truncate checks are retained. Positional file reads avoid modifying the
observable channel position during `toByteArray`.

This changes private backing storage and introduces possible disk I/O failures
after construction; it is not a universal equivalence claim. Generic mods
reflecting private ByteArrayChannel.buf are outside the approved domain.
Physical startup, resource data, world, resource reload and disk failure gates
remain required before any promotion. The module replacement itself is pinned
and must not be automatically installed for an unknown JDK.

Build with JDK 21:

```powershell
javac --patch-module jdk.zipfs=src -d build/classes src/jdk/nio/zipfs/ByteArrayChannel.java
jar --create --file build/wachiland-zipfs-spill.jar -C build/classes .
```

Only the isolated phone trial uses `--patch-module=jdk.zipfs=<private jar>`
and `-Dwachiland.zipfs.spill=true`. Remove both arguments to restore the
unmodified runtime; the JRE files are never overwritten. No APK rebuild and
no BootOptim source change is needed for this test.

## 2026-10-11 depth correction (still experimental)

The actual phone JFR last old-object sample batch revealed JarJar PathFileSystem
wrappers placing the nested ZipFileSystem constructor at frame index 17. The
first implementation examined only 16 StackWalker frames and missed that path.
The candidate now examines at most 32 frames; all other eligibility conditions
and I/O behavior are unchanged. Source attribution and physical trial results
belong in ANDROID-SUPPORT-MODEL-OWNERS-2026-10-11.md. Counter bytes remain
cumulative copied snapshots, never a claim of actual RAM savings. No production
promotion follows solely from increasing coverage.
