# AIPS Phase 0: Threat model (STRIDE)

This covers the Phase 0 demonstrator: aips-core, aips-emergency, the Temporal worker and AIPS
Command, plus what they depend on (PostgreSQL, Kafka, Temporal, Keycloak, Vault, Neo4j and the
OpenTelemetry collector). We went through STRIDE at each trust boundary and, for every threat, wrote
down the control that handles it and the test that exercises that control. Anything marked
residual is either accepted for Phase 0 or left to the platform layer.

It supports PRD NFR-06. It is not a DO-326A/DO-355 security assessment, and nobody outside the team
has reviewed it yet.

## 1. System and trust boundaries

```
 Browser (AIPS Command)  --TLS/OIDC-->  Ingress  -->  aips-core  -->  PostgreSQL / TimescaleDB
        |                                   |            |   \-->  Neo4j (projection)
        |                                   |            \---->  Kafka  <--  airline/OEM connectors
        |--OIDC code+PKCE--> Keycloak <-----+                         \--> Temporal worker --> Temporal
        \--------------------------------> aips-emergency --> own PostgreSQL + local journal
 Secrets: Vault (KV v2)       Telemetry: OTLP --> collector; Prometheus scrape
```

| ID | Boundary | Crosses from → to |
|---|---|---|
| TB1 | User | Browser → ingress/API (public network) |
| TB2 | Identity | Browser/worker → Keycloak; API → Keycloak JWKS |
| TB3 | Integration | Airline/OEM connector → Kafka `aips.ingest` / REST ingest |
| TB4 | Workflow | Temporal worker → aips-core API (service identity) |
| TB5 | Data | Services → PostgreSQL, Neo4j, Vault |
| TB6 | Emergency | Crew devices / trainers → aips-emergency |
| TB7 | Supply chain | Dependencies, model artefacts, container images → build |

Assets, highest value first: integrity of safety findings and evidence grades; the audit trail;
human decisions (grades, acknowledgements, corrections, erasures); tenant data (lessor isolation);
emergency workflow availability; model artefacts; credentials and keys.

## 2. Threats and controls

| # | STRIDE | Boundary | Threat | Control | Verified by |
|---|---|---|---|---|---|
| S1 | Spoofing | TB1/TB2 | Forged or replayed access token | RS256 only, JWKS from issuer, `iss`/`aud`/`exp` checks, unknown roles dropped, no `alg=none`/HS256 | `tests/security/oidc_validation.py` 12/12; `tests/live/keycloak.mjs` (forged signature, payload tampering, alg=none, foreign issuer) |
| S2 | Spoofing | TB1 | Header-based dev identity used in production | `AIPS_AUTH_MODE=dev` refused when `AIPS_ENV=production`; ignored in keycloak mode | OIDC suite start-up test; live Keycloak test (dev headers → 401) |
| S3 | Spoofing | TB2 | Password-only account takeover | MFA (TOTP) required, temporary password must change at first login, brute-force protection, no direct password grant for the UI client | `tests/live/keycloak.mjs` 6/6 (realm bugs B15/B16 fixed) |
| S4 | Spoofing | TB4 | Worker impersonation | Client-credentials service account with `mcc_controller` only; API role checks per endpoint | Live Keycloak test (service token cannot reach admin endpoints); live worker run with Keycloak identity |
| T1 | Tampering | TB5 | Audit log edited or truncated in the database | SHA-256 hash chain; PostgreSQL trigger blocks UPDATE/DELETE; ordered lossless writer | Unit tamper test, proptest (any single mutation or deletion detected), reliability script (trigger survives restore), concurrency test (chain intact under load) |
| T2 | Tampering | TB3 | Malformed, wrong-version or poisoned ingest messages | Typed deserialisation, `x-aips-schema` contract check, unknown tails rejected, 2 MB body limit, idempotent by event id | Integration (415/400/413, duplicate), live Kafka test (malformed, wrong schema, unknown tail, consumer survives) |
| T3 | Tampering | TB7 | Model artefact swapped | SHA-256 in model card checked before load; load refused when the hash is missing (fail closed, bug B19) | `predict::tests::registry_loads_hash_verified_versions_and_rejects_tampering` |
| T4 | Tampering | TB1 | Record history rewritten | Corrections are new attributed versions; originals immutable; correction rows append-only (trigger) | Integration PSP-02, concurrency test (sequential versions) |
| T5 | Tampering | TB1 | Spreadsheet formula injection via CSV export | Cells starting with `= + - @` are prefixed; RFC 4180 quoting | Integration CMD-05 |
| R1 | Repudiation | all | A user denies a decision | Every human decision audited with subject, time and detail; correction, acknowledgement, erasure and model activation included | Integration audit assertions |
| I1 | Information disclosure | TB1 | Lessor reads another lessor's aircraft | Per-request tenant filter on `org`; redaction on export; fleet patterns k-anonymous (k = 3), no tails | Integration (tenant isolation, CFM-05 no tails) |
| I2 | Information disclosure | TB1 | Path traversal or SQL injection | Static files confined to the UI directory; parameterised SQL only | Integration Sentinel tests |
| I3 | Information disclosure | TB1 | Token in SSE query string appears in logs | Short-lived tokens (300 s) | Residual (R2 in the engineering report) |
| I4 | Information disclosure | TB5 | Secrets in images or repo | Vault KV v2 loader; secrets scan in CI | Regex scan of repo; Vault live test **blocked** (binary not downloadable) |
| D1 | Denial of service | TB1 | Request floods or huge bodies | Body limit, bounded channels, analysis off the async runtime and outside the engine lock, coalesced recompute, response cache per snapshot | Perf: 200-way spike 0 errors; concurrency burst 200 mixed writes 0 errors (bugs B10/B12/B21 fixed) |
| D2 | Denial of service | TB2 | Unknown-`kid` flood forces JWKS refetches | JWKS refresh at most once per 60 s | Code review; behaviour observed in the live Keycloak test |
| D3 | Denial of service | TB3 | Consumer stalls after restarts | SIGTERM handling, consumer leaves the group, 10 s session timeout, manual commits | Live test: restart resumes from committed offsets (bug B14 fixed) |
| D4 | Denial of service | TB6 | Emergency path depends on core or database | Separate service, own store and local journal; works with database down | Reliability tests |
| E1 | Elevation of privilege | TB1 | Self-approval of destructive actions | Two-administrator rule for erasure; atomic execution claim | Integration DAT-03 (same admin refused) |
| E2 | Elevation of privilege | TB1 | Machine output treated as engineering confirmation | Only engineers can confirm; machine grades never exceed Corroborated | Integration, proptest `memory_invariants` |
| E3 | Elevation of privilege | TB6 | Training drill notifies real recipients or controls aircraft | Live mode refused; ATC channel never transmits in training; no aircraft control interface exists | Integration Guardian tests, AUT-03 test |

## 3. Safety-relevant abuse cases

| Abuse case | Why it matters | Control |
|---|---|---|
| Conflicting sensor evidence triggers a crew alert | False emergency handling | Contradicting indicators block qualification and are shown explicitly (EMG-04) |
| Stale data shown as "no findings" | False assurance before departure | Stale feeds yield *Insufficient data*, never a clear state (PRV-06, proptest) |
| Correlation presented as root cause | Wrong maintenance action | Correlations are graded *Corroborated* at most; every flight event carries a limitation statement |
| AIPS treated as dispatch authority | Regulatory and safety breach | No approve/deny actions; disclaimers on reports and screens |

## 4. Residual risks (accepted or deferred)

- CORS permissive for the development split-origin setup; restrict at the ingress in production.
- SSE token in query string (I3).
- aips-core single replica with in-memory snapshot; restart recovery measured, no HA.
- Encryption at rest and key rotation delegated to the platform; not verified here.
- Vault, Neo4j and container image scanning not exercised (downloads blocked in this environment).
- No independent penetration test; no claim that the application is free of vulnerabilities.
