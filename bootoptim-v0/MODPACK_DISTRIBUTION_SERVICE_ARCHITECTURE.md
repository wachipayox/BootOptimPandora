# Private modpack distribution service architecture

Status: architecture / executable protocol design only.

This change is stacked on PR #32 head
`agent184/profile-persistent-layout-20260916@c25b3ddf21d314c9cf65beb39bb529253322b3f9`.
It intentionally stays independent of the two parallel follow-ups: PR #34 owns
persistent profile UUID/state/journal/publication and PR #33 owns the
managed/local ownership planner. A future client integration must combine those
correctness boundaries before it applies content from this service.

This document makes **no performance, Start-to-Java, Java-to-menu or TTMM
claim**. It does not implement a server, deploy infrastructure, change launcher
runtime, implement Automodpack, or add a Start-time filesystem scan.

The companion protocol files are:

- `distribution-service/openapi.yaml`
- `distribution-service/revision-manifest.schema.json`

## Decision

Build a small private **publish-only** Linux service whose authority is limited
to authenticated discovery and delivery of:

1. official profile/channel metadata;
2. immutable, externally signed revision manifests;
3. pinned inheritance chains and deterministic derived projections; and
4. immutable SHA-256-addressed objects.

The service does not decide what is currently installed on a player's machine.
It has no endpoint that accepts a game directory, no remote file inventory API,
and no launcher hook that scans player files at Start. Future client application
must go through the persistent-layout staging/ownership/recovery transaction,
not through this server.

## Product invariants

Three official profiles exist from bootstrap and are never deletable:

| Stable id | Display name | Base | Default channel | Visibility |
| --- | --- | --- | --- | --- |
| `profile_wachiland_elite` | `Wachiland Elite` | none | `stable` | member + admin |
| `profile_low_end` | `Low End` | pinned Elite revision | `stable` | member + admin |
| `profile_beta` | `Beta` | optional pinned official revision | `beta` | admin only |

`DELETE` is deliberately absent from the API. The database marks all official
profiles `official = 1` and `deletable = 0`; the service refuses mutations that
would weaken either invariant. A profile can gain new immutable revisions and
new channels, but an old revision is never edited in place.

"Extensible official profile" has two meanings in v1:

- one official profile may derive from a **specific immutable revision** of
  another official profile; and
- an accessible official profile may permit a client-local derived profile via
  its signed `permissions` block.

A derived revision never means "inherit latest". It pins
`base.profile_id + base.revision_id + base.manifest_sha256`, so the same signed
revision resolves to the same inputs forever.

## Trust boundaries

There are four independent authorities:

1. **Identity provider** — authenticates a human/device and issues a short-lived
   access token for the distribution API.
2. **Distribution service** — checks authorization, validates signed release
   material, stores immutable metadata/CAS objects, and exposes channel heads.
3. **Release signer** — an Ed25519 private key kept **off the service host**.
   It signs revision manifests and explicit rollback statements.
4. **Launcher client** — carries a pinned/root-authorized public-key set,
   re-verifies signatures and object hashes, resolves inheritance, and later
   hands the desired layout to the profile transaction layer.

A compromised distribution process can cause denial of service and, because it
can read its own storage, is not a confidentiality boundary for already hosted
pack bytes. It must not be able to forge a new trusted revision without also
compromising a release-signing key.

Minecraft/Microsoft access tokens are **not** distribution-service credentials.
The private service has its own OIDC-compatible issuer/audience and maps its
stable subject (`sub`) to local roles/ACLs.

## Signed revision format

The JSON Schema is authoritative for the wire manifest. Before signing:

1. parse JSON while rejecting duplicate keys and invalid Unicode;
2. validate against `revision-manifest.schema.json`;
3. serialize with RFC 8785 JSON Canonicalization Scheme (JCS);
4. calculate SHA-256 of the canonical bytes; and
5. sign those same canonical bytes with Ed25519.

The envelope returned by the API contains:

```text
canonicalization = "RFC8785-JCS"
manifest_sha256   = sha256(canonical_manifest_bytes)
manifest          = parsed manifest object
signature = {
    key_id,
    algorithm = "Ed25519",
    value = base64url(signature)
}
```

The service verifies the signature before accepting publication. The launcher
verifies it again. An HTTP/TLS success response is never enough to trust a
revision.

### Key bootstrap and rotation

The first trusted public key (or root keyring key) ships with the private
launcher build/configuration. `/v1/keys/revision-signing` is informational; a
network response cannot self-authorize a new trust root.

A production implementation should use a root-signed keyring document with:

- `key_id`;
- Ed25519 public key;
- `not_before` / optional retirement timestamp;
- state `active` or `retired`; and
- root signature over the canonical keyring.

Retiring a key prevents it from signing future releases but does not invalidate
historical revisions needed for rollback/offline verification unless an
explicit compromise policy says otherwise. A private signing key is never
placed in the Git repository, systemd unit, reverse-proxy config, service
database, container image, or environment file.

## Revision identity and immutability

Every official profile has an increasing integer `revision.sequence` and an
opaque `revision.id`. The pair is unique per profile. The service rejects:

- reusing a revision id with different canonical bytes;
- reusing a sequence for a different revision;
- publishing sequence `N` after a higher sequence already exists unless it is
  the exact already-stored revision (idempotent retry);
- a base manifest hash that does not match the referenced immutable base; or
- a revision whose directly referenced object is absent from CAS.

The signed sequence is also the client-side anti-rollback memory. A normal
channel response with a lower sequence than the highest sequence previously
accepted for that profile/channel is rejected unless accompanied by a valid,
release-key-signed rollback event.

## Content model

The signed manifest always contains:

- exact Minecraft version;
- exact NeoForge version;
- profile id/name;
- revision id/sequence/timestamp;
- exact base profile/revision/hash or `null`;
- derivation permissions;
- mod additions/replacements;
- mod removals relative to the pinned base;
- config additions/replacements with policy `enforced` or `default_once`;
- config removals relative to the pinned base; and
- arbitrary additional path/object entries, all backed by SHA-256 object refs.

Every file payload is identified by lowercase SHA-256 plus expected size. The
object hash is the identity; filenames are presentation/destination metadata,
not trust authority.

### Safe destinations

Manifest paths are relative UTF-8 paths. Resolution rejects:

- absolute paths;
- `..` path components;
- empty/repeated separators;
- backslash-based alternate traversal;
- destination collisions after the client's platform normalization rules;
- duplicate logical mod ids;
- a file/directory ambiguity; and
- any path outside the future managed-layout namespace.

The JSON Schema performs a first syntactic gate. The server and client must
repeat platform-aware safe-path validation before use.

## Elite, Low End and Beta

### Wachiland Elite

Elite is a root official profile (`base = null`). Its revision directly lists
the pack's managed mods/configs/other objects. Root manifests cannot contain
`remove_mods` or `remove_configs` because there is no parent set to subtract
from.

### Low End

Low End is a signed overlay over an exact Elite revision. The manifest expresses
only the delta that makes Low End different, for example:

```json
{
  "base": {
    "profile_id": "profile_wachiland_elite",
    "revision_id": "rev_ELITE_EXAMPLE_0001",
    "manifest_sha256": "<64 lowercase hex>"
  },
  "mods": [
    {
      "id": "mod_low_end_specific",
      "path": "mods/low-end-specific.jar",
      "object": { "sha256": "<64 lowercase hex>", "size": 1234 }
    }
  ],
  "remove_mods": [
    {
      "id": "mod_expensive_example",
      "expect_base_object_sha256": "<64 lowercase hex>"
    }
  ],
  "configs": [
    {
      "path": "config/example.toml",
      "object": { "sha256": "<64 lowercase hex>", "size": 456 },
      "policy": "enforced",
      "expect_base_object_sha256": "<64 lowercase hex>"
    }
  ]
}
```

The `expect_base_object_sha256` guard makes a delta fail closed if an admin tries
to reuse it against a parent whose targeted entry changed. Publishing a new
Elite revision does **not** silently modify Low End. The publisher creates and
signs a new Low End revision pinned to the intended new Elite revision.

### Beta

Beta is an official non-deletable profile but its profile/channel ACL is
`admin` only. An unauthorized caller receives the same `404` shape used for an
absent profile, so profile enumeration does not reveal Beta metadata. Objects
referenced only by Beta also require authorization; knowing a SHA-256 is not a
bearer capability.

A Beta revision may be root or derived, but derivation never weakens the Beta
ACL. Effective visibility is the intersection of the requested profile/channel
ACL and authenticated subject permissions, not the union of base visibility.

## Config policy semantics

The server publishes intent; it does not edit client files.

### `enforced`

`enforced` means the official resolved layout requires those official bytes.
Future client reconciliation may treat the path as managed **only after proving
its previous managed ownership**. If a user/local overlay modified the path,
the persistent-layout ownership rules still require an explicit conflict or
permitted overlay decision; the service policy is not permission to destroy
unknown local bytes.

A local derived profile can replace an `enforced` config only when the signed
base permissions explicitly set `configs.override_enforced = true`.

### `default_once`

`default_once` is a seed/default. The first future client application may
materialize it when the destination is absent and the transaction can claim the
new path safely. After that initial application, the live value is local/user
state unless the user explicitly chooses to reset it. Later official revisions
do not silently overwrite the user's changed value.

A local derived profile can supply a different default only when
`configs.override_default_once = true`.

## Deterministic inheritance and overlay resolution

Resolution is a pure operation over signed manifests; it does not read player
files.

For target revision `T`:

1. Verify `T` schema, canonical SHA-256 and Ed25519 signature.
2. Follow its exact base reference recursively.
3. At each hop verify the referenced profile id, revision id and canonical
   manifest SHA-256.
4. Reject a cycle or depth greater than the signed/implementation cap (maximum
   8 bases; 9 manifests including the target).
5. Require exact Minecraft/NeoForge compatibility unless a future schema
   explicitly defines a legal transition.
6. Start with the root's direct maps keyed by mod id/config path/object id.
7. For every derived layer from oldest to newest:
   - check the parent's signed derivation permissions;
   - apply removals only when the expected parent object hash matches;
   - apply additions/replacements only when any declared expected parent hash
     matches;
   - reject path/id collisions and policy escalation not permitted by the base.
8. Canonicalize the resulting effective projection and calculate a
   `resolution_sha256` for cache/debug identity.

`GET /v1/revisions/{id}/resolution` may return this effective projection as a
convenience, but clients must recompute it from the signed chain. The projection
is not signed and is not a trust root.

## Local derived profiles

Local profiles are a **client-owned layer** and are not uploaded by this v1
service.

A future local profile records at least:

```text
local_profile_uuid
origin_profile_id
origin_revision_id
origin_manifest_sha256
local_overlay = {
    add_mods,
    remove_mods,
    config_overrides,
    local_objects/references
}
```

Creation is allowed only if the currently accessible official revision has
`permissions.derive_local = true`. The launcher keeps this overlay when the
user rebases to a newer official revision.

Rebase is three-way, not destructive replacement:

```text
old official base + existing local overlay + new official base
```

If a local removal/replacement expectation no longer matches the new official
base, the client records a conflict and preserves the overlay. It does not drop
local choices merely because the server channel moved. Applying a successfully
rebased result later still goes through profile-local staging, ownership proof,
backup and recovery.

The distribution server never needs the player's live `.minecraft` tree to
perform this rebase.

## Authentication, roles and visibility

### User authentication

The launcher obtains a short-lived API access token from a configured private
OIDC-compatible identity provider using a native-app-safe flow (Authorization
Code + PKCE or Device Authorization where supported). The service validates:

- issuer;
- audience dedicated to this API;
- token signature;
- expiry/not-before;
- stable subject; and
- optional configured tenant/group constraints.

The service maps `sub` to its own role/ACL rows. It does not derive distribution
access from Minecraft ownership, username, email string, or a Microsoft game
access token.

### Roles

Minimum roles:

- `member`: discover/download Elite and Low End channels granted to members;
- `admin`: all member rights plus Beta discovery/download and publication APIs.

Per-user grants can narrow or extend channel visibility without republishing a
revision. A channel ACL is evaluated before returning channel heads, revision
metadata, resolution projections, or objects.

### Admin publication authentication

Admin API calls require the `admin` role. A deployment may additionally require
mTLS at the reverse proxy for `/v1/admin/**`; this is recommended for a small
private service but is separate from revision signing. Even an authenticated
admin request cannot publish an unsigned/invalid revision.

## API contract

`distribution-service/openapi.yaml` is the executable HTTP contract. The core
flow is:

```text
GET  /v1/profiles
GET  /v1/profiles/{profile}/channels/{channel}
GET  /v1/revisions/{revision}
GET  /v1/revisions/{revision}/resolution
HEAD /v1/objects/sha256/{hash}
GET  /v1/objects/sha256/{hash}       # Range supported

PUT  /v1/admin/objects/sha256/{hash}
POST /v1/admin/revisions
POST /v1/admin/profiles/{profile}/channels/{channel}/promote
POST /v1/admin/profiles/{profile}/channels/{channel}/rollback
```

There is no client-side "report installed files" endpoint, no Start endpoint,
and no remote command endpoint.

Object responses use immutable identity semantics:

```text
ETag: "<sha256>"
Cache-Control: private, max-age=31536000, immutable
Accept-Ranges: bytes
```

The authorization check still runs before an object is served; proxy/CDN
configuration must not accidentally turn a private hash into a public static
URL.

## Publication workflow

Authoring/signing and serving are deliberately separate.

1. Admin build tooling creates a revision from an **explicit release input set**.
   It must not crawl player installations or use launcher Start as an ingestion
   trigger.
2. Tooling calculates SHA-256/size for each release object.
3. Missing objects are uploaded with
   `PUT /v1/admin/objects/sha256/{expected_hash}`.
4. Service streams upload to an untrusted temporary file, calculates SHA-256,
   compares size/hash, fsyncs, and atomically renames it into CAS only on exact
   match. Existing matching object makes the request idempotent.
5. Admin tooling creates and validates the revision manifest.
6. The manifest is canonicalized/signed on the signing workstation or signing
   device. The service never receives the private key.
7. `POST /v1/admin/revisions` verifies signature/schema/base/object existence,
   inserts immutable revision metadata, and commits it without changing any
   channel head.
8. Admin explicitly promotes the revision with compare-and-swap on the previous
   channel head.

A partially uploaded object or failed revision insert is not discoverable as a
published revision.

## Channel promotion and rollback

Normal promotion may only move to a revision that belongs to the same profile
and whose signed sequence is greater than the current head. The request includes
`expected_current_revision_id`; a stale admin UI/CLI therefore receives `409`
instead of overwriting another operator's action.

Rollback never edits an old revision. The release signer signs a canonical
rollback statement containing:

```text
profile_id
channel
from_revision_id
from_sequence
to_revision_id
reason
issued_at
```

The admin sends statement + detached Ed25519 signature to the rollback endpoint.
The service verifies it with a trusted release key, writes an immutable rollback
event, and atomically updates the channel pointer if the expected current head
still matches.

A client that previously accepted sequence 12 and later sees sequence 10 accepts
the lower head only when it also verifies a rollback event authorizing that
specific transition. This blocks accidental/proxy replay of an old normal
channel response.

## Retention and garbage collection

V1 chooses safety over aggressive reclamation:

- published revision rows/manifests are retained indefinitely;
- objects referenced by any published revision are retained indefinitely;
- rollback events are retained indefinitely;
- channel-head changes never delete revisions/objects;
- only failed-upload temporary files and CAS objects that are not referenced by
  **any** published revision may be garbage-collected;
- unreferenced objects receive a quarantine age (recommended minimum seven
  days) before deletion so publication races and operator mistakes remain
  recoverable.

If storage pressure later requires revision expiry, that is a schema/protocol
change with an explicit minimum rollback/offline window; it must not be added as
a hidden cron policy.

Database backups and CAS backups are operational retention, separate from API
rollback. Restoring a backup must not manufacture a lower channel sequence
without its already-recorded signed rollback history.

## Offline behavior

The service provides distribution, not DRM.

Once an authorized client has downloaded and verified a revision and its
objects, it may keep those bytes locally and use the last verified revision
offline according to launcher policy. Server-side access revocation prevents
future discovery/downloads but cannot securely erase bytes already delivered to
a machine.

Offline rules:

- no network means no channel update decision;
- use only a complete previously verified signed revision chain and verified
  object set;
- never treat a partial download as a revision;
- preserve the highest accepted sequence/rollback events locally;
- do not re-resolve `latest` while offline;
- a local derived profile keeps its pinned official origin + local overlay; and
- when connectivity returns, compare the remote channel head and run normal
  signed update/rebase logic outside Start.

Beta confidentiality therefore means "not served to non-admin users", not an
impossible promise that a previously authorized admin's cached Beta bytes can be
remotely revoked.

## Linux storage layout

Recommended single-node v1 layout:

```text
/etc/bootoptim-distribution/
  config.toml                         # no private release-signing key
  service.env                         # optional runtime secrets, mode 0600
  trusted-release-keys/               # public keys/root-signed keyring only

/var/lib/bootoptim-distribution/
  metadata.sqlite3
  metadata.sqlite3-wal                # when WAL is active
  objects/sha256/aa/<remaining-62-hex>
  manifests/sha256/aa/<remaining-62-hex>.json
  rollback/sha256/aa/<remaining-62-hex>.json
  incoming/<random-upload-id>.part

/run/bootoptim-distribution/
  service.sock                        # optional Unix socket to reverse proxy
```

The CAS path is derived only from a validated lowercase 64-hex digest, never
from a user-supplied filename. Objects/manifests become immutable after atomic
publication. The service account owns `/var/lib/bootoptim-distribution`; the
reverse proxy does not receive direct write access.

SQLite is adequate for the initial private single-node deployment because
publication is low-frequency and channel-head changes need simple ACID
transactions. The storage/API model does not depend on SQLite and can move to
PostgreSQL later without changing signed revision semantics.

### Logical metadata schema

The implementation should preserve this logical model (types abbreviated):

```sql
CREATE TABLE profiles (
  id TEXT PRIMARY KEY,
  system_key TEXT UNIQUE NOT NULL,
  display_name TEXT NOT NULL,
  official INTEGER NOT NULL CHECK (official = 1),
  deletable INTEGER NOT NULL CHECK (deletable = 0),
  derivable INTEGER NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE profile_acl (
  profile_id TEXT NOT NULL REFERENCES profiles(id),
  principal_kind TEXT NOT NULL CHECK (principal_kind IN ('role','user')),
  principal TEXT NOT NULL,
  can_discover INTEGER NOT NULL,
  can_download INTEGER NOT NULL,
  PRIMARY KEY (profile_id, principal_kind, principal)
);

CREATE TABLE channels (
  profile_id TEXT NOT NULL REFERENCES profiles(id),
  name TEXT NOT NULL,
  head_revision_id TEXT,
  PRIMARY KEY (profile_id, name)
);

CREATE TABLE channel_acl (
  profile_id TEXT NOT NULL,
  channel TEXT NOT NULL,
  principal_kind TEXT NOT NULL CHECK (principal_kind IN ('role','user')),
  principal TEXT NOT NULL,
  can_discover INTEGER NOT NULL,
  can_download INTEGER NOT NULL,
  PRIMARY KEY (profile_id, channel, principal_kind, principal),
  FOREIGN KEY (profile_id, channel) REFERENCES channels(profile_id, name)
);

CREATE TABLE objects (
  sha256 TEXT PRIMARY KEY CHECK (length(sha256) = 64),
  size INTEGER NOT NULL CHECK (size >= 0),
  created_at TEXT NOT NULL
);

CREATE TABLE revisions (
  id TEXT PRIMARY KEY,
  profile_id TEXT NOT NULL REFERENCES profiles(id),
  sequence INTEGER NOT NULL,
  manifest_sha256 TEXT UNIQUE NOT NULL,
  signing_key_id TEXT NOT NULL,
  signature TEXT NOT NULL,
  base_profile_id TEXT,
  base_revision_id TEXT,
  created_at TEXT NOT NULL,
  UNIQUE (profile_id, sequence)
);

CREATE TABLE revision_objects (
  revision_id TEXT NOT NULL REFERENCES revisions(id),
  object_sha256 TEXT NOT NULL REFERENCES objects(sha256),
  purpose TEXT NOT NULL,
  destination TEXT,
  PRIMARY KEY (revision_id, object_sha256, purpose, destination)
);

CREATE TABLE channel_events (
  event_id TEXT PRIMARY KEY,
  profile_id TEXT NOT NULL,
  channel TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('promote','rollback')),
  from_revision_id TEXT,
  to_revision_id TEXT NOT NULL,
  rollback_statement_sha256 TEXT,
  created_at TEXT NOT NULL
);
```

Application code exposes no revision update/delete path. The production schema
should additionally install triggers that reject `UPDATE`/`DELETE` of
`revisions` and `channel_events` except through an explicitly versioned database
migration performed with the service stopped.

## Filesystem publication safety on Linux

The service implementation must:

- create incoming temp files with restrictive mode and unpredictable names;
- reject symlinks in service-owned state paths;
- avoid path construction from arbitrary names;
- hash while streaming, not after trusting request metadata;
- enforce configured maximum object and request sizes;
- fsync object bytes and containing directories before considering publication
  durable;
- publish CAS files by same-filesystem atomic rename;
- treat an existing hash path with wrong size/bytes as corruption and stop
  serving it rather than overwriting it silently; and
- periodically permit an explicit operator scrub that re-hashes CAS storage.

No correctness decision relies on mtime.

## systemd deployment contract

The future service runs as an unprivileged dedicated account and listens only on
loopback or a Unix socket. A representative unit is:

```ini
[Unit]
Description=BootOptim private modpack distribution service
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=bootoptim-dist
Group=bootoptim-dist
ExecStart=/usr/local/bin/bootoptim-distribution serve --config /etc/bootoptim-distribution/config.toml
EnvironmentFile=-/etc/bootoptim-distribution/service.env
StateDirectory=bootoptim-distribution
RuntimeDirectory=bootoptim-distribution
UMask=0077
NoNewPrivileges=yes
PrivateTmp=yes
PrivateDevices=yes
ProtectSystem=strict
ProtectHome=yes
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectControlGroups=yes
RestrictSUIDSGID=yes
LockPersonality=yes
CapabilityBoundingSet=
AmbientCapabilities=
RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

`service.env` may contain an OIDC client secret or online service-auth secret if
the selected identity integration needs one. It is provisioned out of band,
mode `0600`, never committed. It never contains the release-signing private key.

## Reverse proxy contract

Nginx, Caddy, or an equivalent maintained reverse proxy terminates public TLS.
The application itself binds `127.0.0.1:9080` or a Unix socket. A representative
Nginx shape is:

```nginx
server {
    listen 443 ssl http2;
    server_name packs.example.invalid;

    ssl_certificate     /etc/letsencrypt/live/packs.example.invalid/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/packs.example.invalid/privkey.pem;

    client_max_body_size 2g;

    location / {
        proxy_pass http://127.0.0.1:9080;
        proxy_http_version 1.1;
        proxy_request_buffering off;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-Proto https;
        proxy_set_header X-Forwarded-For $remote_addr;
    }
}
```

TLS private-key paths above are examples, not repository secrets. Production
config should add the operator's normal TLS policy/rate limits and, if chosen,
mTLS on `/v1/admin/`. The proxy must not expose
`/var/lib/bootoptim-distribution/objects` as an unauthenticated static root.

For large files, a later implementation may authorize in the app and then use
an internal `X-Accel-Redirect` location. That internal location must be
unreachable directly from the public listener.

## Threat model

| Threat | Required behavior / mitigation |
| --- | --- |
| Unauthorized user guesses Beta/profile/object id | ACL before metadata and object delivery; inaccessible resource returns non-enumerating `404`. |
| Reverse proxy/CDN replays old channel head | Client stores highest accepted sequence; lower sequence requires signed rollback event. |
| Distribution database is edited | Client signature/hash verification rejects forged revision bytes; immutable DB constraints make accidental edits harder. |
| Distribution host is fully compromised | Attacker can deny service/read hosted bytes, but cannot mint a trusted new revision without release key; clients still enforce signatures/hashes/anti-rollback. |
| Release-signing private key stolen | High-severity trust compromise; rotate/revoke key through pinned/root-signed keyring and publish incident policy. Keep signing key off server. |
| Object changed/truncated in storage/transit | SHA-256 object identity verified by service at ingest and client before use; size is an additional check, not authority. |
| Malicious manifest path traversal | Schema + server/client safe-path normalization reject absolute/parent/ambiguous paths before publication/application. |
| Inheritance cycle/depth bomb | Exact base pin, visited-set cycle detection, max depth 8 bases. |
| Overlay silently targets changed parent | `expect_base_object_sha256` mismatch rejects resolution/publication; admin must author a new overlay. |
| `enforced` config destroys local edit | Server never writes player files; future client must use ownership proof/conflict handling. |
| Partial object/revision upload | Temp+hash+fsync+atomic CAS publication; revision inserted only after all direct objects exist and signature/schema validate. |
| Concurrent admins race channel head | Compare-and-swap expected current revision; loser gets `409`. |
| API token theft/replay | TLS + short-lived issuer/audience-bound tokens; optional admin mTLS; no game-account token reuse. |
| User revocation while offline | Stops future server access; service does not pretend it can erase already downloaded bytes. |
| Player has malicious/symlinked local game tree | Out of server trust boundary; future client persistent-layout safety rules handle local paths. Service never scans that tree. |
| Oversized upload / storage DoS | Authenticated admin-only upload, configured size/quota/rate limits, free-space guard, orphan quarantine/GC. |
| Logs leak tokens | Never log Authorization headers/signatures as credentials; structured audit logs use subject id/operation/revision ids and redact sensitive headers. |

## Operational audit log

Record security/audit facts, not player filesystem data:

- authenticated subject id and role decision;
- revision publication id/hash/key id;
- channel promote/rollback event;
- object upload result/hash/size;
- authorization denial category;
- signature/schema/hash validation failure; and
- service/database version at startup.

Do not log bearer tokens, OIDC refresh tokens, private key material, or arbitrary
request bodies containing credentials.

## Client integration boundary

This service can be implemented/tested before launcher application because the
wire objects are immutable. **Content application remains disabled** until the
client has a coherent integration of:

1. persistent profile UUID/state;
2. transaction journal + staging/backup + idempotent recovery (PR #34 lineage);
3. ownership/local-override/tombstone/collision planner (PR #33 lineage);
4. deterministic desired-layout resolver consuming this signed effective
   manifest;
5. stopped-profile mutation hooks; and
6. only later, a Ready-only Start fast path.

The client update path should be:

```text
fetch auth-visible channel head
-> verify anti-rollback
-> fetch/verify signed revision chain
-> resolve effective manifest
-> download missing hash objects to immutable content library
-> build profile-local desired-layout delta
-> ownership plan
-> stage
-> publish/recover transactionally
-> Ready
```

None of these steps belongs to an unconditional filesystem walk at Start.

## Implementation cuts

Each cut has a narrow acceptance boundary.

### Cut 1 — protocol fixtures and validators

- Parse/validate JSON Schema.
- RFC 8785 canonicalization.
- Ed25519 sign/verify test fixtures with public test keys only.
- Inheritance resolver, cycle/depth/path/collision/permission tests.
- Elite root + Low End +/- overlay + admin-only Beta fixtures.

No network listener or production key.

### Cut 2 — read-only metadata/object service

- SQLite migrations with bootstrap official profiles/ACLs.
- Authenticated profile/channel/revision endpoints.
- CAS read/HEAD/Range with per-subject authorization.
- No admin mutation endpoints yet.

### Cut 3 — safe object ingestion

- Admin auth.
- streaming hash/size verification;
- temp/fsync/atomic CAS publication;
- idempotent identical-object upload;
- corrupt-existing-object fail-closed tests.

### Cut 4 — immutable revision publication

- verify signed envelope/schema/base/direct objects;
- immutable DB insert;
- revision update/delete rejection;
- CAS race/concurrent publication tests.

### Cut 5 — channel promotion + signed rollback

- compare-and-swap promotion;
- signed rollback statement/event;
- client anti-rollback fixtures;
- audit log.

### Cut 6 — hardened Linux deployment package

- unprivileged systemd unit;
- reverse-proxy reference config;
- backup/restore procedure;
- CAS orphan quarantine/GC;
- explicit operator scrub;
- secret provisioning documentation without committed credentials.

### Cut 7 — launcher fetch/cache only

After the service protocol is stable, Pandora may fetch/cache verified manifests
and objects without applying them. This proves auth/offline/cache semantics
without touching live profiles.

### Cut 8 — launcher apply/rebase

Only after staging/ownership/recovery are integrated: resolve desired official
or local-derived layout, perform profile-stopped transactional reconcile, and
preserve local overlay/conflicts. Start remains outside this cut until the
separate Ready fast-path gate is satisfied.

## Adversarial acceptance matrix

Before service promotion, tests must cover at least:

- member sees Elite/Low End but not Beta;
- admin sees all three;
- inaccessible direct revision/object ids return hidden-not-found behavior;
- invalid/expired/wrong-audience token;
- wrong revision signature and unknown signing key;
- duplicate JSON keys/canonicalization mismatch;
- object length/hash mismatch;
- partial upload and process crash before/after rename;
- same object uploaded twice;
- revision id or sequence collision;
- missing base and base hash mismatch;
- Low End expected-parent mismatch after Elite changes;
- inheritance cycle and depth overflow;
- path traversal/collision;
- forbidden local-derivation permission escalation;
- concurrent promotion CAS race;
- replayed older channel head without rollback authorization;
- valid signed rollback to retained revision;
- DB restart/recovery preserving immutable heads/events;
- offline complete cached revision versus incomplete object set; and
- no request path that ingests/scans a player game directory.

## Validation tier for this PR

This PR is **docs/spec only**. It changes Markdown, OpenAPI YAML and JSON Schema;
it does not touch a Rust crate, launcher runtime, package, systemd deployment,
or workflow. Under the current Pandora iteration rule, the minimum gate is
scope/diff inspection rather than `cargo fmt`, `cargo check`, Rust tests,
release Windows, full matrix, packaging or physical startup measurement.

A later service implementation must add focused protocol/schema/signature/storage
unit/integration tests before broader deployment tests. A later Pandora runtime
integration must use the appropriate focused Rust gate first and must not treat
CI duration as startup-performance evidence.

## Explicit non-goals

- no Automodpack implementation;
- no launcher Start/runtime modification;
- no mod/config copy/materialization change in this PR;
- no player-file inventory upload;
- no background Start scan;
- no Microsoft account/token reuse for distribution auth;
- no private signing key on the service;
- no repository/fork creation;
- no infrastructure deployment;
- no AppCDS/USN/assets changes; and
- no performance or TTMM claim.
