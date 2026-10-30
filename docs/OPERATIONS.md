# AIPS operations guide

## Configuration (environment)

| Variable | Service | Default | Meaning |
|---|---|---|---|
| `AIPS_ENV` | all | development | Dev auth is allowed only for `development`, `local`, `test` or `ci` |
| `AIPS_AUTH_MODE` | core, emergency | keycloak | `dev` = header roles (never in production) |
| `AIPS_OIDC_ISSUER` / `AIPS_OIDC_AUDIENCE` | core, emergency | keycloak realm / `aips-api` | Token validation |
| `AIPS_OIDC_JWKS_BASE` | core, emergency | `AIPS_OIDC_ISSUER` | Realm URL to fetch signing keys from, when the public issuer in tokens is not reachable from the service (docker compose) |
| `DATABASE_URL` | core | unset (in-memory) | PostgreSQL / TimescaleDB |
| `EMERGENCY_DATABASE_URL` | emergency | unset | Separate emergency database |
| `NEO4J_URI`, `NEO4J_USER`, `NEO4J_PASSWORD` | core | unset | Graph projection |
| `KAFKA_BROKERS` | core, worker | unset | Event streaming |
| `AIPS_WORKFLOW_ENGINE` | core | embedded | `temporal` delegates SLA escalation to the worker |
| `VAULT_ADDR` + `VAULT_TOKEN` or `VAULT_ROLE_ID`/`VAULT_SECRET_ID` | core, emergency | unset | Secrets from KV v2 at `secret/aips/core`, `secret/aips/emergency` |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | core, emergency | unset | Trace export (OTLP/HTTP) |
| `AIPS_MODEL_REGISTRY` | core | `ml/models/registry` | Directory of `rul-qr-<version>.onnx` + `.card.json` |
| `AIPS_RUL_VERSION` | core | latest (or last administrator activation) | Pin a model version; overrides a persisted activation |
| `AIPS_TLS_CERT`, `AIPS_TLS_KEY` | core | unset | PEM files; enables the TLS 1.3-only listener |
| `AIPS_REPLAY_FLEET` | core | 50 | Synthetic fleet size (50–2000; smaller values are raised to 50) for capacity tests |
| `AIPS_GUARDIAN_PROCEDURES` | emergency | built-in training examples | JSON file of procedure cards (operator-approved content) |
| `AIPS_KAFKA_DEBUG` | core | unset | librdkafka debug contexts, e.g. `cgrp,consumer` |
| `AIPS_UI_DIR` | core | `apps/command/dist` | Static UI |
| `AIPS_REPLAY_SEED`, `AIPS_REPLAY_SPEED`, `AIPS_REPLAY_WARMUP_HOURS` | core | 20261030, 30, 3 | Replay Lab |
| `AIPS_EMERGENCY_DATA` | emergency | `.data/emergency` | Local journal directory (persistent volume) |
| `AIPS_EMERGENCY_ACK_TIMEOUT_S` | emergency | 20 | Escalation deadline in drills |
| `AIPS_EMERGENCY_LIVE_ENABLED` | emergency | false | Must stay false; `true` refuses to start |
| `AIPS_LOG_JSON`, `RUST_LOG` | core, emergency | 0, info | Structured logs |

## Health and monitoring

`/livez` (process alive), `/readyz` (first analysis complete), `/metrics` (Prometheus) on both services.
Key metrics: `aips_recompute_milliseconds` (alert if > 2000), `aips_ingest_rejected_total`,
`aips_ingest_duplicates_total`, `aips_cases_escalated`,
`aips_detection_precision`, `aips_emergency_active`, `aips_sse_subscribers`. The Grafana dashboard
`AIPS operations` is provisioned from `deploy/grafana`.

## Database migrations

Migrations in `db/migrations/core` run automatically at start-up, in order, and are idempotent and
additive. Before applying a new migration in production, take a backup (below), apply it in staging,
and verify the row counts. Rollbacks: 0002 `ALTER TABLE replay_meta DROP COLUMN clock;`, 0003
`DROP TABLE flight_summaries;`, 0004 `DROP TABLE decision_docs;` (this discards acknowledgements,
corrections, escalations, erasure records and model activations, so export them first).
`audit_log` has a trigger that rejects UPDATE and DELETE; do not disable it.

## Backup and restore

```bash
pg_dump -Fc -d aips -f aips-$(date +%F).dump            # measured 0.5 s / 3.8 MB on demo data
createdb aips_restore && pg_restore -d aips_restore aips-YYYY-MM-DD.dump
# verify: compare counts of aircraft, flights, fault_events, work_orders, hypothesis_grades, case_state, audit_log
# then GET /api/audit/verify after pointing a staging aips-core at the restored database
```
Back up the emergency database the same way, and snapshot the emergency journal volume. The
journal is authoritative if they ever disagree.

## Recovery procedures

| Situation | Behaviour | Action |
|---|---|---|
| aips-core crash | Restarts in about 1 s; reloads dataset, decisions, clock and audit | None; check `/api/audit/verify` |
| PostgreSQL down | Reads continue from memory; writes return 500 and are not applied | Restore the database; writes resume without a restart |
| Emergency DB down | Drills continue on the local journal | Restore; the next change re-syncs the incident |
| Audit chain reports broken | Start-up logs `AUDIT CHAIN BROKEN` | Treat as a security incident: freeze writes, compare with backups |
| Neo4j or Kafka down | Logged; the UI graph falls back to in-process; REST ingestion still works | Restore the service |
| Kafka consumer not consuming after a restart | A previous instance died without leaving the group | Wait one session timeout (10 s); always stop services with SIGTERM so they leave the group |
| Model rollback | `POST /api/models/rul/activate {version, reason}` as admin | Takes effect at the next analysis pass; persists across restarts unless `AIPS_RUL_VERSION` is set |
| Data erasure | Request and approval by two different administrators | Irreversible; take a backup first if the contract allows |

## Troubleshooting

- `libonnxruntime.so.1: cannot open shared object`: set `AIPS_ONNXRUNTIME_ROOT` before building, or install the library in `/usr/local/lib`.
- librdkafka build fails on `curl/curl.h`: install `libcurl4-openssl-dev libsasl2-dev zlib1g-dev`.
- Every request returns 401: check the issuer URL matches the token's `iss`, and that the `aips-api-audience` scope (audience, realm roles, username and `sub` mappers) is attached to the client.
- 401 for about a minute after a Keycloak key change: JWKS refresh on an unknown key id is limited to once per 60 s by design.
- Pre-departure shows Insufficient data for every aircraft: the health feed is older than 12 h; check ingestion.
