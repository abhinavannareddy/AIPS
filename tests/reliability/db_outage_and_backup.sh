#!/usr/bin/env bash
# Reliability tests against a running aips-core backed by PostgreSQL.
#  1. Database outage: reads keep working, writes fail cleanly, no half-applied decision (bug B4).
#  2. Recovery after the database returns.
#  3. Backup with pg_dump and restore into a fresh database; row counts must match.
# Env: PGHOST PGPORT PGUSER, PGCTL="command to stop/start postgres" (stop|start appended).
set -euo pipefail
CORE=${CORE:-http://localhost:8090}
H=(-H 'x-aips-role: engineer')
fail() { echo "FAIL: $*"; exit 1; }
pass() { echo "PASS: $*"; }

HID=$(curl -s "${H[@]}" "$CORE/api/memory/hypotheses" | python3 -c "import json,sys; print([b for b in json.load(sys.stdin) if b['hypothesis']['grade']!='rejected'][0]['hypothesis']['id'])")
BEFORE=$(curl -s "${H[@]}" "$CORE/api/memory/hypotheses" | python3 -c "import json,sys; print([b['hypothesis']['grade'] for b in json.load(sys.stdin) if b['hypothesis']['id']=='$HID'][0])")

$PGCTL stop
code=$(curl -s -o /dev/null -w '%{http_code}' "$CORE/api/fleet" "${H[@]}")
[ "$code" = 200 ] && pass "reads served during database outage" || fail "read returned $code"
code=$(curl -s -o /dev/null -w '%{http_code}' "${H[@]}" -H 'content-type: application/json' -X POST "$CORE/api/memory/hypotheses/$HID/grade" -d '{"grade":"rejected","note":""}')
[ "$code" = 500 ] && pass "write refused during outage (HTTP 500)" || fail "write returned $code"
NOW=$(curl -s "${H[@]}" "$CORE/api/memory/hypotheses" | python3 -c "import json,sys; print([b['hypothesis']['grade'] for b in json.load(sys.stdin) if b['hypothesis']['id']=='$HID'][0])")
[ "$NOW" = "$BEFORE" ] && pass "failed write left no half-applied decision (grade still $NOW)" || fail "grade changed to $NOW without persistence"
curl -sf "$CORE/livez" >/dev/null && pass "service survived the outage" || fail "service crashed or stopped answering"

$PGCTL start
sleep 2
code=$(curl -s -o /dev/null -w '%{http_code}' "${H[@]}" -H 'content-type: application/json' -X POST "$CORE/api/memory/hypotheses/$HID/grade" -d '{"grade":"corroborated","note":"recovery check"}')
[ "$code" = 200 ] && pass "writes recover after database returns" || fail "write after recovery returned $code"

T0=$(date +%s.%N)
pg_dump -Fc -d aips -f /tmp/aips-backup.dump
T1=$(date +%s.%N)
psql -d postgres -q -c "drop database if exists aips_restore" -c "create database aips_restore"
pg_restore -d aips_restore /tmp/aips-backup.dump
T2=$(date +%s.%N)
for t in aircraft components flights fault_events work_orders mel_items param_samples hypothesis_grades case_state audit_log; do
  a=$(psql -d aips -tAc "select count(*) from $t"); b=$(psql -d aips_restore -tAc "select count(*) from $t")
  [ "$a" = "$b" ] || fail "$t: $a rows vs $b restored"
done
pass "backup ($(printf '%.2f' "$(echo "$T1-$T0" | bc)") s, $(du -h /tmp/aips-backup.dump | cut -f1)) and restore ($(printf '%.2f' "$(echo "$T2-$T1" | bc)") s) row counts match for 10 tables"
psql -d aips_restore -c "update audit_log set actor='x' where seq=1" >/dev/null 2>&1 && fail "restored audit log is mutable" || pass "append-only trigger preserved in restored database"
echo "ALL RELIABILITY CHECKS PASSED"
