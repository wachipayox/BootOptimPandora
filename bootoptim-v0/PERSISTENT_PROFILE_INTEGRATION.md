# Persistent profile layout — integration record

`agent/integration-current` is the integration authority for this Pandora
feature. It was created at commit
`bb942ec71fd0ecd964b55f346901989f9b1ae774` on 2026-09-17.

## Included, in order

1. PR #32 — persistent layout ownership/recovery architecture.
2. PR #36 — recoverable client flow: `Legacy/Unknown → Planning → Staging →
   Prepared → Publishing → Ready`.
3. PR #37 — fresh identity for cloned profiles and one cross-process lock per
   profile UUID.
4. PR #39 — native Windows/macOS validation plus the Windows directory-sync
   correction. Gate `35155936282` passed on both operating systems.

The source branch is intentionally the exact #39 head, not a reconstruction
from patch summaries. A smoke-only descendant may add packaging tooling, but
must never be treated as the integration or deploy branch.

## What this establishes

- durable identity, ownership/reconciliation, staging/recovery and lock
  semantics for a persistent local profile layout;
- cloned profiles cannot accidentally share their UUID lock namespace;
- the Windows lock path has native coverage for exclusivity, release after
  process death and reparse-point rejection.

## What it does not establish

- remote profiles, content download, signed revisions, automatic updates or
  server authentication;
- a production promotion to `master`;
- a release artifact or performance claim.

## Laptop smoke boundary (2026-09-20)

The portable executable used for the first visual laptop smoke test was built
from this layout-only integration branch. It deliberately contained neither
the USN asset-verification cache candidates nor the AppCDS launch-authority
candidate: their branches have an older, divergent base and were not composed
into this branch. The run therefore validates only that the persistent-layout
path can launch the existing instance; it is **not** an AppCDS/cache benchmark
and must not be compared with prior cache-enabled timings.

Its launcher log nevertheless recorded an actionable baseline for the next
composed build: 98 s to scan/display mod content, 58 s before launch setup,
then 229 s in Java/assets/libraries/log-configuration preparation before the
game process. A later composition must add phase-attribution probes and an
explicit cache decision record before claiming a pre-Java improvement.

The distribution-service repository owns the remote revision/CAS work. Any
future Pandora integration must reference a reviewed service protocol revision,
preserve local overlays and avoid a full `.minecraft` scan in the Start path.
