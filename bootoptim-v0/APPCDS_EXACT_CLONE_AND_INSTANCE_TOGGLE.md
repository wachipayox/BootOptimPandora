# AppCDS exact local clone and per-instance preference

Status: Agent 201 continuation candidate. This branch was created from
`agent/integration-current@3410b4c47eec0f47012d3c33bbd5e1d58b669e14` and then
composes PR #48 as an explicit merge parent. PR #48 remains untouched and is a
prerequisite for this continuation.

`AGENTS.md` is absent at the required integration snapshot; repository README,
BootOptim v0 identity/roadmap documentation, and the requested AppCDS/profile-lock
PR lineage remain the authority.

## Product decisions

### AppCDS enabled per instance

`InstanceConfiguration.appcds_enabled` is persistent, instance-local launcher
configuration. Its compatibility default is `true`; the true value is omitted
from JSON, so reading an old profile or pressing Start does not trigger an eager
migration/write across instances. An explicit false value is serialized with that
instance and follows an ordinary local clone as normal instance payload.

Instance Settings shows one compact **AppCDS enabled** checkbox.

When false, Pandora returns to the stock Java launch path before spawning the
AppCDS preflight helper. It therefore neither consumes READY nor requests TRAIN
and does not touch or delete the existing AppCDS cache. Re-enabling only restores
the normal AppCDS eligibility path: PR #48 profile binding plus the effective
launch plan and the existing READY/TRAIN state machine remain authoritative.

The setting is not global and does not make CLI, legacy or unknown launch sources
eligible for any identity-cache optimization.

### Explicit exact local clone

Normal **Duplicate instance** remains isolated: it remints the persistent
`profile_uuid` and skips active, quarantined and exact-clone AppCDS state.

The duplicate dialog now has a separate, default-off **Exact local clone (reuse
validated AppCDS)** choice. It applies only to this local duplication operation;
it is not used for remote profiles, derived revisions or updates.

Exact clone deliberately **still remints the destination profile UUID**. Reusing
the source UUID would make two live roots claim the same persistent identity and
would undermine the per-profile lock/cache ownership contract.

While the source persistent-profile lock is held, exact clone requires:

- committed Ready persistent profile state with no recovery/conflict transaction;
- AppCDS `profile.namespace` matching the source UUID;
- no pending/invalid TRAIN state or AppCDS staging files;
- `launch-plan.json` matching its strong SHA-256 publication;
- READY metadata matching that plan;
- a regular contained `ready.jsa` whose full SHA-256 and size match metadata.

The archive is **not** copied into the destination active AppCDS namespace.
Instead, after the ordinary profile clone has finished and proved/reminted its
Ready layout, the archive is copied to a completed sibling candidate directory.
The source archive is rehashed after copying and the completion marker is written
last. Interrupted/partial copies therefore cannot be mistaken for READY.

The candidate records an expected destination plan SHA-256. That expectation is
derived from the already proven source plan by changing only two clone-ownership
facts: the persistent profile UUID and serialized artifact paths that are
demonstrably inside the relocated instance root. JVM argv fingerprints/safe
literals, Java, loader, classpath/module-path inputs, helper/launcher identity,
mods/config content hashes and every other plan field are not rewritten.

On the destination's first eligible auto launch, PR #48 first obtains the
destination profile lease, binds its cache UUID, reconstructs the complete
effective launch plan with the normal strong identity code and takes the AppCDS
cache lock. The inherited archive can then be promoted only when that rebuilt
plan hash is exactly the candidate's expected hash and the copied archive still
matches its recorded SHA-256/size. Promotion uses the existing AppCDS promotion
primitive, which writes fresh READY metadata for the destination's effective
plan.

Any mod/config/Java/loader/argv/component/input difference, wrong UUID, corrupt
metadata/archive, reparse/path escape, existing local training/READY state,
recovery ambiguity, lock contention or persistence error fails to STOCK. A
mismatching/corrupt completed candidate is quarantined; it is never converted to
READY.

## Concurrency and recovery

No launcher-global lock, launch IPC gate or restriction on running two game
copies is added. This continuation composes only with PR #48's existing
per-profile layout lease and AppCDS cache lock. Contention/uncertainty affects
AppCDS activation only and falls back to stock launch.

Normal clone holds the source profile snapshot lock as before. Exact clone uses
that same lease while validating/copying source READY evidence. Destination
candidate publication happens only after the reminted persistent clone commits.
A crash before the final candidate rename leaves only staging state, which the
runtime never adopts.

## Composition order

This PR is intentionally stacked without editing PR #48:

1. merge PR #48 (profile namespace/lease) into `agent/integration-current`;
2. rebase/drop this branch's explicit #48 merge parent so the remaining delta is
   the exact-clone/preference continuation;
3. compose PR #47 independently. Exact-candidate adoption belongs after the
   destination's full plan reconstruction/eligibility checks and before normal
   READY/TRAIN handling. It does not relax PR #47's later independent identity
   confirmation for newly trained archives;
4. compose the real incremental identity work only after its own authority/gates.

The preference false gate remains outside the helper, so it suppresses both
consumption and training regardless of whether PR #47 is present.

## Measurement boundaries and physical gate

This work makes no TTMM claim.

- **Click -> Java:** disabling AppCDS removes AppCDS preflight from that instance;
  exact-clone adoption still performs full strong destination identity and
  archive validation. Any change here must be measured only as Click -> Java.
- **Java -> menu:** unchanged by this work and must be reported separately.
- **Exit/training:** TRAIN/archive generation remains exit-side work. A successful
  exact clone can avoid a new training campaign, but that is not a Java -> menu
  saving and is not counted as normal Start latency.

No laptop request is needed for this source candidate. Physical validation is
meaningful only after composition with PR #48, PR #47 and the real incremental
identity path, using matching built artifacts and separate timing boundaries.
