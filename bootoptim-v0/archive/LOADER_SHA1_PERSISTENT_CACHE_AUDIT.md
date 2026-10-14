# Agent 174 — persistent Forge/NeoForge installer SHA-1 cache audit

## Provenance and scope

Research branch: `agent174/loader-sha1-cache-research-20260915`, created from the final valid Start-to-Java probe authority `agent155/launch-prejava-probe-20260914@55f05135396e9682a5d0f76a24e62f21850fbc03` (PR #7).

This front is deliberately independent of the asset USN cache and AppCDS work. It does not change launch-source eligibility, asset verification, AppCDS, loader selection, repair behavior, Java launch arguments, UI, authentication or privileged helpers.

The prior durable audit is PR #12 / `bootoptim-v0/METADATA_PRE_ASSETS_AUDIT.md`. That audit rejected a blind persistent `.sha1` reuse because it would hide a same-version repository replacement, and noted that a conditional HTTP cache preserves freshness only by retaining the network round trip. Agent 174 reopens only the performance premise because two new physical HDD launches observed `network_request source=loader_sha1` inside `version_loader_resolution`, with the whole span at 4.34 s and 4.80 s, a fixed loader version, and no repair.

No TTMM or end-to-end saving is claimed. The physical values are attribution evidence only and phases must not be summed.

## Source-confirmed criticality

For both Forge and NeoForge, `Launcher::create_launch_version` resolves the loader version and calls `create_forgelike_launch_version`. That function constructs the canonical installer `.sha1` URL and waits on a `futures::future::join` of:

1. `MinecraftVersionMetadataItem(version_link)`, and
2. `Launcher::download_sha1(http_client, installer_hash_url)`.

Only after both futures finish does Pandora construct the installer `GameLibraryArtifact` with that returned SHA-1 and call `load_libraries`. Therefore the `.sha1` request is a real barrier participant in `version_loader_resolution`: if it is slower than the concurrently fetched version metadata, it directly holds the phase open. The supplied physical probe proves the request occurs in both new-launcher runs; it does not prove that the request owns every millisecond of the 4.34/4.80 s span, because other version/loader work overlaps it.

`download_sha1` currently accepts only a 40-byte UTF-8 body. The downstream library loader is stricter: when the SHA-1 is present it must decode as exactly 20 bytes of hexadecimal or launch fails with `LoadLibrariesError::InvalidHash`.

When the expected installer SHA-1 is present, `do_libraries_load` fully SHA-1 hashes an existing local installer before accepting it. If the file is absent or mismatched, Pandora downloads the installer and full-hashes the downloaded bytes against the same expected digest before writing it. Only after this validation does `create_forgelike_launch_version` open the installer ZIP and consume `install_profile.json` and the embedded version/profile data.

If the `.sha1` request fails or is not a 40-byte UTF-8 body, stock `download_sha1` returns `None`. In that stock fallback, an existing installer is accepted by existence rather than content hash, and a newly downloaded installer has no expected SHA-1 to compare. This is existing stock behavior; this research does not broaden it and a new cache must not turn locally persisted state into stronger-but-stale remote authority.

## Loader-version and manifest identity

`InstanceConfiguration::determine_neoforge_loader_version` immediately returns `preferred_loader_version` when configured. Otherwise it chooses the newest matching version from the freshly resolved NeoForge Maven manifest. Forge has the analogous preferred-version behavior.

The NeoForge metadata item downloads `maven-metadata.xml`, but Pandora's parsed `NeoforgeMavenManifest` retains the sorted version list, not an authoritative installer-artifact digest. With a fixed/preferred loader version, the chosen loader version is independent of the current version list once the fetch has completed.

A safe cache key could mechanically bind at least:

- cache schema version;
- loader kind (`forge` / `neoforge`);
- exact selected loader version;
- exact canonical installer `.sha1` URL;
- exact canonical installer JAR URL;
- optionally a digest of the fetched Maven manifest bytes if the implementation preserved those bytes.

Those fields prevent cross-loader, cross-version and cross-URL confusion. They do **not** prove that a cached digest is still the origin's current digest for that same identity.

## Why a network-skipping persistent hit is not safe

Consider two launch worlds with identical local observations at process start:

- same loader kind and pinned loader version;
- same canonical installer and `.sha1` URLs;
- same cached record containing `H_old`;
- same local installer bytes that previously validated against `H_old`;
- same locally cached/parsing-equivalent Maven version manifest.

World A: the origin still serves `.sha1 = H_old` and the old installer.

World B: the repository has replaced that **same version** at the same URLs and now serves `.sha1 = H_new` plus corresponding installer bytes. The retained Maven version list may remain identical because the version coordinate did not change.

Any persistent cache algorithm that skips contacting the `.sha1` origin sees exactly the same local inputs in A and B, so it must make the same decision in both worlds. If it returns `H_old`, it is correct in A and stale in B. Stock Pandora contacts the origin and can observe `H_new` in B. Therefore loader kind + version + URLs + local installer identity + retained version-manifest identity are insufficient to preserve stock same-version update visibility while removing the network request.

This is an information boundary, not a serialization-format problem. Schema versioning, strict SHA-1 syntax, atomic writes, checksums over the cache file, file permissions and stronger local hashing can reject malformed or torn cache state, but none can manufacture remote freshness.

A local TTL only limits the stale window; it does not remove the counterexample and changes stock update semantics. This task explicitly excludes a blind TTL.

## Why deriving SHA-1 from an already validated installer is insufficient

After a successful stock launch with remote digest `H_old`, computing SHA-1 over the installer again can prove that the **local bytes** still equal `H_old`. It cannot prove that the origin still designates `H_old` as current for that version/URL.

Using that local digest to authorize future network skipping would make the previously accepted installer self-authorizing. In World B above it would continue accepting the old local JAR and would hide the origin's same-version replacement. The derived digest is useful only as local corruption evidence; it is not update/freshness authority.

## Required bad cases

### First installation

There is no trusted cache record or validated installer. Stock network resolution is required. A cache may be populated only after a syntactically valid remote SHA-1 has actually governed full installer validation, but that still does not authorize a future network-skipping hit.

### Installer absent or corrupt

A cached old digest is not enough to decide what should be downloaded because the origin may now publish a different same-version digest. To preserve stock update semantics, resolve the remote `.sha1` first, then run the existing full installer validation/download path.

### Loader version update

A changed loader kind/version or canonical URL must invalidate any record. This is necessary but not sufficient because same-version replacement remains possible.

### Remote `.sha1` changed at the same version/URL

This is the decisive fail-closed case. A network-skipping cache cannot distinguish it from an unchanged origin using current local inputs. It must therefore fall back to the network if exact stock update visibility is required.

### Offline

Stock direct `.sha1` resolution returns `None` on send/read/format failure. Reusing a previously cached remote digest offline would change stock fallback semantics and make stale persisted state authoritative. This audit retains the stock offline behavior: no network means no newly trusted cached `.sha1`.

### Corrupt, partial or injected cache

A future sidecar can and should reject unknown schemas, duplicate fields, invalid loader kind/version/URL identity, non-canonical URLs, non-lowercase/non-40-hex SHA-1, partial files and checksum mismatches, and should publish via temp-file + sync + atomic replace. Those controls are still insufficient to solve stale remote authority, so implementing them alone would add complexity without creating a safe hit state.

## Conditional HTTP remains safe but does not remove this wait

PR #12 already specifies a sound ETag/Last-Modified design: validate local body + sidecar, send a conditional request, accept `304` only against that validated pair, accept and atomically replace on validated `200`, and retry an unexpected `304` unconditionally. Missing/corrupt validators or proxy anomalies fall back to the stock GET; offline behavior remains stock.

That design preserves remote revalidation because it still contacts the origin. For the installer `.sha1`, the response body is only 40 bytes, so converting a normal `200` to `304` cannot remove DNS/TLS/server/RTT wait and is not an evidence-backed mechanism for eliminating the observed 4–5 s `version_loader_resolution` span.

## Decision

**NO-GO for a persistent network-skipping Forge/NeoForge installer `.sha1` cache under the requested update/integrity contract.**

The new physical evidence confirms that `loader_sha1` is a meaningful critical-path participant and therefore makes this front performance-interesting. It does not provide the missing remote freshness authority required to skip the request safely.

No production cache is implemented. In particular this branch does not add a TTL, does not use a validated local installer as remote freshness proof, does not trust a cached digest offline, and does not use the Maven version list as an artifact-hash authority.

## Reopen criterion and physical promotion gate

Reopen implementation only if a materially new authority is available, for example an origin-authenticated/signed metadata path that binds the selected Forge/NeoForge version to the installer digest and has defined update semantics, or another verifiable origin contract that makes same-version replacement detectable without the `.sha1` GET. Merely observing the same version/URL/JAR again is not a new premise.

If such an authority appears, the candidate cache must be versioned and atomically published, bind exact loader kind + version + canonical URLs + authoritative freshness/digest input, strictly validate the SHA-1 syntax, and fall back to the existing network path on every unknown/corrupt/mismatch state. Tests must cover first install, hit, loader update, same-version remote digest change, absent/corrupt installer, offline, corrupt/truncated/injected cache and publication interruption.

Only after those semantic gates pass should the target HDD run an alternating stock/candidate A/B of **`version_loader_resolution`** using the valid PR #7 root, the exact fixed loader version and repaired/no-download state. Record cache hit/miss/network-fallback separately. Do not infer TTMM and do not add assets-USN or AppCDS savings to this phase.
