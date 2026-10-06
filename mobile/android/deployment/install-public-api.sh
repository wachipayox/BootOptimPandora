#!/usr/bin/env bash
# Wachiland read-only Distribution routes for the observed Nginx deployment.
set -euo pipefail
[[ $(id -u) == 0 ]] || { echo 'Run with sudo bash install-public-api.sh'; exit 1; }
command -v python3 >/dev/null
command -v nginx >/dev/null
target=$(readlink -f /etc/nginx/sites-enabled/pterodactyl.conf)
[[ -f "$target" && "$target" == /etc/nginx/* ]] || { echo 'Expected Pterodactyl Nginx configuration was not found.'; exit 1; }
nginx -t
stamp=$(date -u +%Y%m%dT%H%M%SZ)-$$
backup_directory="/var/backups/wachiland-nginx/$stamp"
install -d -m 700 "$backup_directory"
backup="$backup_directory/pterodactyl.conf"
snippet=/etc/nginx/snippets/wachiland-distribution-read-api.conf
snippet_backup="$backup_directory/distribution-read-api.conf"
cp -p -- "$target" "$backup"
had_snippet=0
if [[ -e "$snippet" ]]; then
    cp -p -- "$snippet" "$snippet_backup"
    had_snippet=1
fi
restore() {
    cp -p -- "$backup" "$target"
    if (( had_snippet )); then cp -p -- "$snippet_backup" "$snippet"; else rm -f -- "$snippet"; fi
    echo "Previous configuration restored. Backup: $backup" >&2
}
trap restore ERR
install -d -m 755 /etc/nginx/snippets
cat > "$snippet" <<'NGINX'
# Wachiland: public read routes only. Administrator routes stay on private 8444.
location ~ "^/v1/(profiles(?:/profile_[A-Za-z0-9_-]+/revisions(?:/rev_[A-Za-z0-9_-]+)?)?|objects/sha256/[0-9a-f]{64}|signing-keys|meta/version)$" {
    limit_except GET { deny all; }
    proxy_pass https://192.168.1.69:8444;
    proxy_bind 192.168.1.69;
    proxy_http_version 1.1;
    proxy_set_header Host welite.ddns.net;
    proxy_set_header Connection "";
    proxy_set_header Authorization "";
    proxy_set_header Cookie "";
    proxy_ssl_server_name on;
    proxy_ssl_name welite.ddns.net;
    proxy_ssl_verify on;
    proxy_ssl_trusted_certificate /etc/ssl/certs/ca-certificates.crt;
    proxy_ssl_verify_depth 4;
    proxy_connect_timeout 5s;
    proxy_read_timeout 300s;
    proxy_buffering off;
    proxy_max_temp_file_size 0;
    add_header X-Content-Type-Options nosniff always;
}
NGINX
chmod 644 "$snippet"
python3 - "$target" <<'PYTHON'
import os, re, sys, tempfile
from pathlib import Path
path=Path(sys.argv[1])
text=path.read_text()
include='    include /etc/nginx/snippets/wachiland-distribution-read-api.conf;'
if text.count(include)>1:
    raise SystemExit('Duplicate Wachiland includes: inspect configuration before proceeding.')
if include not in text:
    anchor=re.compile(r'(?m)^\s*ssl_certificate_key\s+/etc/letsencrypt/live/welite\.ddns\.net/privkey\.pem;[^\n]*$')
    matches=list(anchor.finditer(text))
    if len(matches)!=1 or not re.search(r'listen\s+443\s+ssl',text):
        raise SystemExit('HTTPS configuration differs from the supplied server block; no edit applied.')
    match=matches[0]
    text=text[:match.end()]+'\n\n'+include+text[match.end():]
    stat=path.stat()
    with tempfile.NamedTemporaryFile('w',dir=path.parent,prefix='.wachiland-nginx-',delete=False) as file:
        temporary=file.name
        file.write(text)
        file.flush()
        os.fsync(file.fileno())
    os.chmod(temporary,stat.st_mode)
    os.chown(temporary,stat.st_uid,stat.st_gid)
    os.replace(temporary,path)
PYTHON
nginx -t
systemctl reload nginx
trap - ERR
echo "Wachiland public read API installed on HTTPS port 443. Backup: $backup"
echo 'Catalog: https://welite.ddns.net/v1/profiles'
