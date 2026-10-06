# Mobile data connectivity diagnosis — 2026-10-06

The first APK connects to `https://welite.ddns.net:8444`. The development PC has
an explicit hosts entry mapping that hostname to `192.168.1.69`. Its successful
catalog response therefore established LAN access, not Internet access.

On the phone, public DNS resolves `79.116.38.74`. The user confirmed mobile data
and no response from the same URL in the phone browser. A TCP connection from
the PC forced to that public IP also timed out on 8444. This occurs before TLS,
signature validation or JSON parsing.

Public port 443 responds with the existing website. `/v1/profiles` there currently
returns HTML, not the Distribution protocol. Distribution's documented deployment
binds `192.168.1.69:8444` and restricts the whole listener to `192.168.1.0/24`.
Simply forwarding port 8444 does not address the application's CIDR restriction.

## Prepared route

`distribution-read-api.nginx.conf` is an include for the existing HTTPS server
block on port 443. It forwards only catalog, immutable revisions, object hashes,
public signing keys and version metadata. GET/HEAD only, upstream TLS verification
enabled, no administrator routes, no credentials forwarded for public readers.
It streams pack files without writing an Nginx temporary copy.

Before deployment, inspect the actual HTTPS server block and its existing
locations; account for any existing `/v1/` prefix location using `^~`. Add the
include to that exact block, run `nginx -t`, and reload only if it succeeds.
The local read endpoint and panel must remain unchanged. Confirm the public
catalog returns protocol JSON over verified TLS before changing the mobile
client's default URL to `https://welite.ddns.net` (port 443).

The user provided `nginx -T`: public 443 is the HTTPS server block in
`/etc/nginx/sites-enabled/pterodactyl.conf`, with the existing Certbot key directive
and no conflicting `/v1/` location. `install-public-api.sh` adds the include after
that directive. It resolves the enabled-site symlink, backs up original files to
`/var/backups/wachiland-nginx`, checks syntax before and after editing, and reloads
Nginx. Failure restores the prior files. Backups are outside included configuration
directories so a regular file in sites-enabled cannot create duplicate servers.

The configuration has not yet been applied. SSH with BatchMode rejected the
agent's identity, so the operator must execute this one-time installer on Linux.
The Android 0.1.1 default uses port 443. It recognizes an HTML website response
as an API deployment error rather than reporting only a JSON parser exception.
Do not disable certificate checking or expand administrator CIDRs as a workaround.
