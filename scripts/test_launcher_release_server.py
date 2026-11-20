"""Local HTTPS stand-in for Distribution's launcher release endpoints, for testing the Windows setup.

It serves one synthetic `player` release of `pandora` for `windows-x86_64` whose artifact is the given
file, signed with a throwaway Ed25519 key that the setup discovers through /v1/signing-keys (the same path
the real server's own key takes). A throwaway CA signs a certificate for 127.0.0.1. Nothing here touches
the real server.

    python scripts/test_launcher_release_server.py --artifact WachilandLauncher.exe --version 1.0.0 \
        --mode ok --ready-file ready.json

`--ready-file` receives {"origin": "https://127.0.0.1:<port>", "ca": "<PEM path>"} once it listens; point
the setup at it with WBOMBO_SETUP_TEST_ORIGIN and WBOMBO_SETUP_TEST_CA. Modes:

    ok             valid release
    bad-signature  the statement signature is altered
    bad-hash       the served bytes differ from the signed SHA-256 (same size)
    no-release     the index is empty
    server-error   every request answers HTTP 503

Needs the `cryptography` package.
"""
import argparse
import base64
import datetime
import hashlib
import http.server
import ipaddress
import json
import os
import ssl
import tempfile
import threading
import time

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec, ed25519
from cryptography.x509.oid import NameOID

PLATFORM = "windows-x86_64"


def b64url(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()


def make_tls(directory: str):
    now = datetime.datetime.now(datetime.timezone.utc)
    ca_key = ec.generate_private_key(ec.SECP256R1())
    ca_name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "Wachiland setup test CA")])
    ca = (
        x509.CertificateBuilder()
        .subject_name(ca_name)
        .issuer_name(ca_name)
        .public_key(ca_key.public_key())
        .serial_number(x509.random_serial_number())
        .not_valid_before(now - datetime.timedelta(minutes=5))
        .not_valid_after(now + datetime.timedelta(days=1))
        .add_extension(x509.BasicConstraints(ca=True, path_length=0), critical=True)
        .add_extension(
            x509.KeyUsage(False, False, False, False, False, True, True, False, False), critical=True
        )
        .sign(ca_key, hashes.SHA256())
    )
    key = ec.generate_private_key(ec.SECP256R1())
    leaf = (
        x509.CertificateBuilder()
        .subject_name(x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "127.0.0.1")]))
        .issuer_name(ca_name)
        .public_key(key.public_key())
        .serial_number(x509.random_serial_number())
        .not_valid_before(now - datetime.timedelta(minutes=5))
        .not_valid_after(now + datetime.timedelta(days=1))
        .add_extension(
            x509.SubjectAlternativeName(
                [x509.IPAddress(ipaddress.ip_address("127.0.0.1")), x509.DNSName("localhost")]
            ),
            critical=False,
        )
        .add_extension(x509.BasicConstraints(ca=False, path_length=None), critical=True)
        .add_extension(x509.ExtendedKeyUsage([x509.oid.ExtendedKeyUsageOID.SERVER_AUTH]), critical=False)
        .sign(ca_key, hashes.SHA256())
    )
    ca_path = os.path.join(directory, "ca.pem")
    cert_path = os.path.join(directory, "server.pem")
    key_path = os.path.join(directory, "server.key")
    with open(ca_path, "wb") as f:
        f.write(ca.public_bytes(serialization.Encoding.PEM))
    with open(cert_path, "wb") as f:
        f.write(leaf.public_bytes(serialization.Encoding.PEM))
    with open(key_path, "wb") as f:
        f.write(
            key.private_bytes(
                serialization.Encoding.PEM,
                serialization.PrivateFormat.PKCS8,
                serialization.NoEncryption(),
            )
        )
    return ca_path, cert_path, key_path


def make_release(artifact: bytes, version: str, mode: str):
    signing = ed25519.Ed25519PrivateKey.generate()
    key_id = "setup-test-" + os.urandom(4).hex()
    public = signing.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)
    digest = hashlib.sha256(artifact).hexdigest()
    now = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    statement = {
        "app": "pandora",
        "issued_at": now,
        "platform": PLATFORM,
        "sha256": digest,
        "size": len(artifact),
        "type": "wbombo.launcher-artifact.v1",
        "version": version,
    }
    # RFC 8785 for this object: sorted keys, no whitespace, plain integers and ASCII strings.
    raw = json.dumps(statement, sort_keys=True, separators=(",", ":")).encode()
    signature = bytearray(signing.sign(raw))
    if mode == "bad-signature":
        signature[0] ^= 1
    release = {
        "version": version,
        "channel": "player",
        "published_at": now,
        "notes": "Release sintética de prueba.",
        "artifact": {
            "platform": PLATFORM,
            "file_name": f"WachilandLauncher-{version}.exe",
            "url": f"/v1/objects/sha256/{digest}",
            "sha256": digest,
            "size": len(artifact),
            "statement": b64url(raw),
            "signature": {"key_id": key_id, "algorithm": "Ed25519", "value": b64url(bytes(signature))},
        },
    }
    served = bytearray(artifact)
    if mode == "bad-hash":
        served[len(served) // 2] ^= 0xFF
    keys = {"schema_version": 1, "keys": {key_id: b64url(public)}}
    releases = [] if mode == "no-release" else [release]
    index = {"schema_version": 1, "app": "pandora", "latest": releases[0] if releases else None, "releases": releases}
    return keys, index, digest, bytes(served)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact", required=True)
    parser.add_argument("--version", default="1.0.0")
    parser.add_argument(
        "--mode", default="ok", choices=["ok", "bad-signature", "bad-hash", "no-release", "server-error"]
    )
    parser.add_argument("--ready-file", required=True)
    parser.add_argument("--port", type=int, default=0)
    parser.add_argument("--tls-dir", help="reuse (or create) the test CA and certificate in this folder")
    parser.add_argument("--rate", type=int, default=0, help="limit the artifact download to this many bytes/s")
    args = parser.parse_args()

    with open(args.artifact, "rb") as f:
        artifact = f.read()
    keys, index, digest, served = make_release(artifact, args.version, args.mode)
    if args.tls_dir and os.path.exists(os.path.join(args.tls_dir, "server.key")):
        directory = args.tls_dir
        ca_path, cert_path, key_path = (os.path.join(directory, name) for name in ("ca.pem", "server.pem", "server.key"))
    else:
        directory = args.tls_dir or tempfile.mkdtemp(prefix="wachiland-setup-test-")
        os.makedirs(directory, exist_ok=True)
        ca_path, cert_path, key_path = make_tls(directory)
    routes = {
        "/v1/signing-keys": ("application/json", json.dumps(keys).encode()),
        "/v1/launcher/pandora/releases": ("application/json", json.dumps(index).encode()),
        f"/v1/objects/sha256/{digest}": ("application/octet-stream", served),
    }

    class Handler(http.server.BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def do_GET(self):
            path = self.path.split("?", 1)[0]
            route = routes.get(path)
            if args.mode == "server-error":
                status, content_type, body = 503, "text/plain", b"unavailable"
            elif route is None:
                status, content_type, body = 404, "text/plain", b"not found"
            else:
                status, (content_type, body) = 200, route
            self.send_response(status)
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            if args.rate > 0 and content_type == "application/octet-stream":
                step = max(1, args.rate // 10)
                for offset in range(0, len(body), step):
                    self.wfile.write(body[offset : offset + step])
                    time.sleep(0.1)
            else:
                self.wfile.write(body)

        def log_message(self, format, *values):
            print(f"[{args.mode}] {self.command} {self.path.split('?', 1)[0]} -> {format % values}", flush=True)

    server = http.server.ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(cert_path, key_path)
    server.socket = context.wrap_socket(server.socket, server_side=True)
    origin = f"https://127.0.0.1:{server.server_address[1]}"
    with open(args.ready_file + ".tmp", "w", encoding="utf-8") as f:
        json.dump({"origin": origin, "ca": ca_path}, f)
    os.replace(args.ready_file + ".tmp", args.ready_file)
    print(f"listening on {origin} mode={args.mode}", flush=True)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        threading.Event().wait()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
