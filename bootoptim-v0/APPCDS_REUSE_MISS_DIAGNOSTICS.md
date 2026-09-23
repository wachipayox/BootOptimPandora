# AppCDS identity-cache miss diagnosis

Status: opt-in diagnostic only; it changes neither identity acceptance nor AppCDS
state-machine decisions. It is intended to explain physical-run results such as
`reused=959 strong=204`, not to claim a performance improvement.

## What the laptop result currently proves

The 959 reused records are files whose digest was accepted from the prior manifest
after the direct NTFS/USN checks passed. The other 204 were conservatively read and
SHA-256 hashed again because the current evidence did not authorize reuse, or there
was no usable old record. `unverifiable=none` only means evidence queries/final
identity checks did not report an error; it does **not** explain ordinary cache
misses. File counts also do not tell us how many bytes/time were saved.

The same run's `launch-plan.match=FIRST_OR_MISMATCH` and
`state=STALE reason=identity-mismatch` are a separate issue: the existing READY
archive did not match the newly rebuilt canonical plan and therefore was not
consumed. The prior plan JSON had already been overwritten, so the exact field
responsible for that historical mismatch cannot be reconstructed from the retained
files. Do not describe that launch as an AppCDS-hit timing result.

## Enabling diagnostics

Set `BOOTOPTIM_APPCDS_IDENTITY_DIAGNOSTICS=1` for one normal Pandora GUI launch.
The existing summary line adds:

- reused and strongly hashed bytes, not just file counts;
- the reason for each miss grouped by input role;
- whether a strong rehash produced the same digest as the cached one or different
  content;
- at most 16 opaque correlation tokens (`SHA256(role + NUL + encoded path)`, first
  16 hex digits), never raw paths, content, or digest values.
- on a plan mismatch, `BOOTOPTIM_APPCDS_PLAN_DIFF fields=...` lists only canonical
  top-level field names whose values changed. It never prints field values.

Reason counters include absent old records, failed evidence queries, volume/FileId
replacement, journal reset/regression/window expiry, changed per-file USN and
same-handle identity change. Strong rehashes still occur for every miss. Diagnostics
are disabled by default and collected only under the opt-in environment variable.

## Decision gate / possible follow-up

First collect one run with the same packaged launcher/helper and unchanged instance.
If a large share of misses have `digest_same` and are concentrated in one role, map
the opaque tokens locally to the plan and find which process rewrites those files.
If `digest_changed` is nonzero, identify the writer and establish whether those bytes
are truly launch-semantic before considering any narrower identity policy. Do not
replace USN evidence with timestamps, omit inputs globally, or weaken READY identity
to force a match. Only after the miss cause and byte distribution are known should a
behavioral cache-policy change be proposed.

For the AppCDS READY mismatch, the next diagnostic launch compares the old and new
canonical plan by field name before replacing it. A field-level mismatch still means
stock fallback; it is diagnostic, not permission to consume a mismatching archive.
