# Startup verification harnesses

These are standalone host semantic checks, outside the distributable. Compile
against `build/classes/java/earlyCompat` after `gradlew assemble`. JDK21 internal
ASM requires exports of `java.base/jdk.internal.org.objectweb.asm`, `.tree`, and
`.util` to `ALL-UNNAMED` for compilation/execution. Supply actual runtime ASM
9.7.1 jars on the host classpath for the opcode golden test; no new dependency
is bundled in the mod.

- GraphAlgorithmTest:280random read/transitive graph pairs plus dense500nodes,
  cycles/partial parent edges. Compares stock fixed-point algorithm to the
  packaged ModuleGraphClosure implementation. Synthetic timing is not phone
  performance evidence.
- VerifyGraphHook: arguments are exact JRE Resolver.class and output class path.
  Rewritten bytecode verifies; changed runtime bytes rejected.
- VerifyCanonicalGraph: exact Resolver.class argument. VM-style pool/debug
  repack accepted; instruction mutation rejected; rewritten bytecode verifies.
- VerifyOpcodeAgent: exact Mixin jar argument. Stock and transformed wrapper
  results match for8209values, extrema and all runtime ASM integer constants;
  changed class rejected.

Phone runtime, graphics and comparable performance gates remain separate.
Do not infer Android behavior or startup speed from a green host harness.

`VerifyLifecycle` compiles directly with LifecycleBridge. It checks bounded
atomic-file visibility, current UUID completion and missing/malformed/oversized
state falling back to visible. It is a protocol test, not a simulation of
Android process priority, SurfaceTexture ownership, Doze or Minecraft ticks.
