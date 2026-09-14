# Agent 164 — pre-assets metadata audit

Base: `agent161/asset-verification-intent-20260914@635171f4241320c0de4507fbb67ed45f1cfabd17` (PR #10 head). PR #10 and the current PR #7 head diverge after common launch-probe commit `48b12855f054302d53a305f1eab9e768508fb3e2`; this branch deliberately touches no launch-probe, asset-object, USN-cache, scheduler, AppCDS or UI file.

This is a **docs-only result**. No performance saving is claimed. The root-valid laptop capture supplied to this investigation measured Start→Java `317.864 s` and `version_loader_resolution=13.104 s`; that whole 13.104 s span is a strict upper bound on anything this metadata front could possibly save in that capture, not an estimate of a candidate saving.

## Scope and source facts

The audited source is `crates/backend/src/metadata/manager.rs`, `crates/backend/src/metadata/items.rs`, `crates/backend/src/backend_handler.rs` and `crates/backend/src/launch/mod.rs`, together with PR #7–#10 and the repository README material.

`MetadataManager` already implements a process-local single-flight state per metadata key. A caller that finds `Pending` becomes the resolver; concurrent callers see `PendingOther` and wait for that same result. Therefore the normal metadata-manager path does **not** issue duplicate concurrent GETs for the same state key inside one launch.

For items with `expires() == true`, a successful in-memory result is kept alive for five minutes. A force reload or expiry starts a new load. For hash-pinned items (`data_hash() != None`), an on-disk cache hit is accepted only after reading the complete cached bytes, computing SHA-1 over those bytes, matching the expected 20-byte digest, and successfully deserializing the bytes. A valid hash-pinned cache hit returns before network access.

For cached items with no authoritative `data_hash`, the file is parsed first only as `file_fallback`; the manager still performs the network request on a reload. If the request fails and the cached file parsed successfully, the current behavior falls back to that parsed local file. This is intentionally different from treating the file as fresh.

## Exact NeoForge pre-assets chain

For `Loader::NeoForge`, `Launcher::create_launch_version` first runs these two metadata-manager fetches concurrently:

1. `MinecraftVersionManifestMetadataItem`
   - Request: Mojang version manifest URL (`MOJANG_VERSION_MANIFEST_URL`).
   - Disk cache: `version_manifest.json`.
   - Authoritative SHA-1: none.
   - Expiry: yes; successful process-local state is kept alive for five minutes.
   - Reload behavior: parse cached file as fallback, then GET. A successful `200 OK` is deserialized and written to the cache. A request/server failure may return the parsed file fallback. No mtime or size test is used.

2. `NeoforgeInstallerMavenMetadataItem`
   - Request: `https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml`.
   - Disk cache: `neoforge_installer_maven.xml`.
   - Authoritative SHA-1: none.
   - Expiry: yes; same five-minute process-local keepalive behavior.
   - Reload behavior: parse cached XML as fallback, then GET. A successful `200 OK` is parsed, versions are sorted by Pandora's existing version-fragment ordering and the body is written to cache. Request failure may use the parsed file fallback. No mtime or size test is used.

Pandora then selects the NeoForge loader version from that Maven manifest and enters `create_forgelike_launch_version`.

That function runs the following two operations concurrently:

3. `MinecraftVersionMetadataItem(version_link)`
   - Request: the exact version JSON URL from the Mojang version manifest.
   - Disk cache: `metadata/version_info/<manifest-provided-sha1>`.
   - Authoritative SHA-1: the SHA-1 carried by `MinecraftVersionLink` in the version manifest.
   - Expiry: no.
   - Cache hit: read complete cached bytes → SHA-1 → exact expected digest → deserialize. Only then return without network.
   - Cache miss/corruption: GET the version JSON, deserialize it, compute SHA-1 over the downloaded bytes, require the exact expected digest, then write the cache. A downloaded hash mismatch returns `MetaLoadError::InvalidHash`; it is not accepted as an update.

4. NeoForge installer `.sha1`
   - Request: `https://maven.neoforged.net/releases/net/neoforged/neoforge/<loader>/neoforge-<loader>-installer.jar.sha1`.
   - Implementation: direct `reqwest` GET in `Launcher::download_sha1`, outside `MetadataManager`.
   - Persistent cache: none.
   - In-launch deduplication: none is needed in the audited NeoForge path because this function is called once for the selected loader version.
   - Validation at this function: response body must be exactly 40 bytes and valid UTF-8. The returned string later reaches library loading, where Pandora requires it to decode as an exact 20-byte hexadecimal SHA-1 or returns `LoadLibrariesError::InvalidHash`.
   - Network/error behavior: send/read/format failure returns `None`; the existing downstream artifact then has no loader SHA-1. This audit does not change that stock behavior.

Only after the version/loader construction completes does `Launcher::launch` start the top-level Java/assets/libraries/log-configuration preparation group.

## Asset-index metadata immediately before asset objects

`Launcher::load_assets` derives the asset-index name from the resolved `MinecraftVersion`, then fetches `AssetsIndexMetadataItem` before entering `do_asset_objects_load`.

- Request: `version_info.asset_index.url`.
- Disk cache: `assets/indexes/<assets-name>.json`.
- Authoritative SHA-1: `version_info.asset_index.sha1` from the already hash-validated Minecraft version JSON.
- Expiry: no.
- Cache hit: read complete file → SHA-1 → exact expected digest → deserialize → return with no network.
- Missing/corrupt/mismatched cache: GET, deserialize downloaded bytes, compute SHA-1, require exact expected digest, then write. A remote body with the wrong hash is rejected.
- Cache invalidation therefore follows the authoritative expected SHA-1, not mtime or size. Reusing the same cache filename across a future metadata change is safe because the expected digest is checked before use.

`do_asset_objects_load` is outside this audit and is not modified by this branch.

## Cache invalidation and concurrency summary

The relevant cache identities are:

- version manifest: one fixed cache path; no hash authority; refresh policy is process-local five-minute expiry / force reload, with disk only as network-error fallback;
- NeoForge Maven manifest: same semantics as the version manifest;
- Minecraft version JSON: content-addressed cache filename derived from the manifest-provided SHA-1 plus full-byte SHA-1 verification;
- asset index: version-selected cache filename plus full-byte verification against the SHA-1 from the validated version JSON;
- NeoForge installer `.sha1`: no cache at all.

`MetadataManager`'s `Pending/PendingOther` state already deduplicates same-key concurrent loads. A second safe intra-launch metadata deduplication candidate was therefore not found.

## Candidate assessment

### Blind TTL / mtime / size cache

Rejected. Treating the unhashed Mojang or NeoForge manifests as fresh merely because a local age, mtime or size matches would change update semantics and can hide remote changes. Treating a versioned NeoForge `.sha1` URL as permanently immutable would likewise make a repository replacement invisible. None of these meet the task's update/repair contract.

### Persistent use of a cached loader `.sha1` when offline

Rejected. Current `download_sha1` returns `None` on network failure. Returning a previously cached digest instead would change stock failure behavior and could make an old remote digest authoritative without revalidation. The requested `no network` fallback is therefore the existing stock path, not stale-cache reuse.

### HTTP conditional requests (ETag / Last-Modified)

Semantically viable in principle, but not material enough from the available evidence to land default-on in this task.

A correct implementation could store response validators for the two unhashed persistent metadata bodies and/or the loader `.sha1`, then send `If-None-Match` / `If-Modified-Since` on the **same GET that stock already performs**. A `304 Not Modified` could reuse local bytes only after local cache validation; `200 OK` would parse/validate and replace cache state. Missing validators, proxies that strip validators, server changes, or malformed validator metadata would fall back to an unconditional stock GET.

However, conditional GET does not remove the network round-trip. The specifically observed `loader_sha1` body is only 40 bytes, so converting its `200` to `304` cannot plausibly remove a 13-second request wait; it only avoids transferring those 40 response bytes. The two unhashed manifest bodies are larger, but this investigation has no per-request timing/body-size evidence showing that body transfer or second parse—rather than DNS/TLS/server/RTT—materially contributes to the measured `13.104 s` version/loader span.

Because the requested optimization target is wall time rather than bandwidth, adding validator sidecars and new cache state without evidence that body transfer is material would add failure modes without a demonstrated launch benefit. No code is therefore landed.

## Safe conditional-cache contract if reopened

If later evidence justifies implementation, the minimum fail-open contract should be:

1. Keep the existing network request. Never use a local TTL to skip it.
2. Persist validator state only beside an already successfully parsed `200 OK` body. Store at least ETag and/or Last-Modified plus a locally computed SHA-1 of the exact cached body. This local SHA-1 is corruption detection, not remote authority.
3. Before sending a conditional request, read the cached body, require its stored local SHA-1 to match, and require normal deserialization to succeed. If any part is missing/corrupt, ignore the validator and make the normal unconditional GET.
4. On `304`, reuse only that already validated local body. If `304` arrives without a valid local body/validator pair, retry once with an unconditional GET; never manufacture freshness from the unexpected `304`.
5. On `200`, deserialize first, apply any existing authoritative hash rule, compute the local body digest, and publish body + validator metadata atomically enough that a torn sidecar cannot authorize a body. A torn/corrupt sidecar must cause unconditional network behavior.
6. On proxy anomalies, validator parsing failure or absent validators, behave exactly like stock GET.
7. On no network, preserve the existing behavior of the individual item: unhashed manager items may use their existing parsed `file_fallback`; direct loader `.sha1` must continue returning `None` rather than newly trusting stale cache state.
8. Do not compare local clock time to Last-Modified. Echo validators only; clock correctness must be irrelevant.
9. Preserve `MetadataManager` single-flight behavior so concurrent callers share one conditional/unconditional fetch rather than racing cache publication.
10. A future explicit full verification/repair path must be allowed to bypass validator reuse if its policy requires a complete `200` body, but this docs-only branch does not invent such a policy.

Required tests for that future implementation are: valid local body + validator + `304`; remote modification + `200`; corrupt body or digest mismatch forcing unconditional `200`; missing validator forcing stock GET; unexpected `304` with absent/corrupt cache forcing unconditional retry; no-network preservation of each current fallback; and concurrent same-key callers causing one network fetch/publication.

## Cost ceiling and reopening criterion

For the supplied root-valid laptop capture, the **absolute ceiling** for this entire front is the observed `version_loader_resolution=13.104 s`. No metadata-only candidate can save more than that span, and the span also contains work that a conditional GET would not remove.

Reopen an implementation only when PR #7-compatible measurement (or a deliberately narrower diagnostic that does not alter the root contract) demonstrates at least one of the following on a repaired/no-download launch:

- the same manifest or loader-hash URL is actually requested more than once within one launch despite the current manager state machine; or
- for the version manifest / NeoForge Maven manifest, at least `1.0 s` median wall time across repeated comparable runs is attributable to response-body transfer plus reparse/write after the response starts, and the origin supplies a stable ETag or Last-Modified validator; or
- an A/B prototype of conditional GET, still performing one network round-trip and preserving all fallbacks above, produces a repeatable material reduction in `version_loader_resolution` without changing selected Minecraft version, selected NeoForge version, installer SHA-1, asset-index SHA-1, repair outcome, or error behavior.

Any performance statement must come from that physical A/B. This audit makes none.