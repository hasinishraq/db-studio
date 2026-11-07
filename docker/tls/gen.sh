#!/usr/bin/env bash
# Test-only CA and server certificate for localhost. Never use outside tests.
set -euo pipefail
export MSYS_NO_PATHCONV=1
cd "$(dirname "$0")"
openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -subj "/CN=AI DB Studio Test CA" \
  -keyout ca.key -out ca.crt
openssl req -newkey rsa:2048 -nodes -subj "/CN=localhost" -keyout server.key -out server.csr
printf 'subjectAltName=DNS:localhost,IP:127.0.0.1\n' > san.ext
openssl x509 -req -in server.csr -CA ca.crt -CAkey ca.key -CAcreateserial -days 3650 \
  -extfile san.ext -out server.crt
rm -f san.ext server.csr
chmod 644 server.crt ca.crt server.key
