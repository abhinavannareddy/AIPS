#!/usr/bin/env bash
# NFR-05: aips-core in-process TLS accepts TLS 1.3 only. Usage: tests/security/tls13.sh target/release/aips-core
set -euo pipefail
BIN=${1:-target/release/aips-core}
D=$(mktemp -d)
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -keyout "$D/k.pem" -out "$D/c.pem" -days 1 \
  -subj "/CN=localhost" -addext "subjectAltName=DNS:localhost" 2>/dev/null
AIPS_AUTH_MODE=dev AIPS_CORE_ADDR=127.0.0.1:8443 AIPS_TLS_CERT="$D/c.pem" AIPS_TLS_KEY="$D/k.pem" "$BIN" > "$D/core.log" 2>&1 &
PID=$!; trap 'kill $PID 2>/dev/null; rm -rf "$D"' EXIT
for i in $(seq 1 60); do curl -sf --cacert "$D/c.pem" https://localhost:8443/readyz >/dev/null && break; sleep 1; done
fail=0
t13=$(echo | openssl s_client -connect 127.0.0.1:8443 -tls1_3 2>/dev/null || true)
t12=$(echo | openssl s_client -connect 127.0.0.1:8443 -tls1_2 2>&1 || true)
# The handshake line, not "Protocol  : TLSv1.3": that session block needs the post-handshake
# ticket, which can arrive after `echo |` has already closed the connection.
grep -q "^New, TLSv1.3, Cipher is" <<<"$t13" && echo "PASS: TLS 1.3 handshake" || { echo "FAIL: TLS 1.3"; fail=1; }
grep -q "alert protocol version" <<<"$t12" && echo "PASS: TLS 1.2 refused" || { echo "FAIL: TLS 1.2 accepted"; fail=1; }
[ "$(curl -s -o /dev/null -w '%{http_code}' --cacert "$D/c.pem" https://localhost:8443/api/health)" = 200 ] && echo "PASS: HTTPS API" || { echo "FAIL: HTTPS API"; fail=1; }
[ "$(curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:8443/api/health || true)" = 000 ] && echo "PASS: plaintext HTTP refused" || { echo "FAIL: plaintext served"; fail=1; }
exit $fail
