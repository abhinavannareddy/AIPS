# AIPS Phase 0: Engineering report

30 October 2026

This report covers the Phase 0 demonstrator in this repository. It was built against the AIPS PRD and
checked against our production-readiness checklist (security, testing, reliability, performance and
honest status reporting). If something is marked *verified*, we actually ran it here. If we could not
run it, it says *not verified* or *blocked* and explains why. See also
[`TECHNICAL_AUDIT.md`](TECHNICAL_AUDIT.md), [`THREAT_MODEL.md`](THREAT_MODEL.md) and
[`REQUIREMENTS_TRACE.md`](REQUIREMENTS_TRACE.md).

In short: 76 of the 95 requirements are built and verified (59 in the first iteration). Nine are
partial, five are built but not verified, one is blocked and four don't apply. Nothing failed. Every
PRD module exists now, Flight included. We ran Kafka-protocol ingestion, Temporal workflows, Keycloak
login with MFA, OpenTelemetry export, Prometheus scraping and TLS 1.3 against real servers, and that
testing turned up 14 more defects (B7 to B21; B17 turned out not to be one). They are all fixed and
covered by regression tests.

Vault, Neo4j, Apache Kafka itself and the container builds are still blocked, because their binaries
and images can't be downloaded in this environment.

Keep in mind that this is a demonstrator running on synthetic data. It isn't certified or approved
by any authority, and it isn't ready for real aircraft operations.

---

## 0. Environment and method

| Item | Value |
|---|---|
| Host | Linux container, 2 vCPU, 7 GB RAM |
| Toolchain | rustc 1.97, g++ 13.3, CMake 3.28, Node 22, Python 3.13 (training and tests only) |
| Database | PostgreSQL 16, real server. TimescaleDB extension not installed, so `param_samples` runs as a plain table; the migration creates the hypertable when the extension exists |
| Live infrastructure run | Temporal CLI 1.1.2 dev server; Keycloak 26.0.7 with the shipped realm; Tansu 0.6 (Apache Kafka wire protocol, SQLite storage); otelcol-contrib 0.116.1; Prometheus 3.0.1 |
| Blocked | Docker Hub, ghcr.io, dist.neo4j.org, releases.hashicorp.com, proxy.golang.org, Apache/Maven mirrors: no Neo4j, Vault, Apache Kafka, container builds or Trivy database |
| Data | Replay Lab synthetic fleet, seed 20261030: 50 aircraft (500 for NFR-04), 21,200 flights, 2,450 fault events, 212,000 parameter samples, 19 labelled scenarios |

---

## A. Technology stack implementation

| Technology | Status | Where | Verification evidence |
|---|---|---|---|
| **Rust** (62% of source) | Adopted | All services and crates | 28 unit/property tests, clippy `-D warnings` clean, `cargo fmt` clean |
| **C++17** (13%) | Adopted | `libs/aips-signal` | Unit tests and a 4,000-iteration fuzz loop under ASan + UBSan; 4 FFI property tests (300 cases each) |
| **CUDA C++** | Optional, off | `batch_cuda.cu` | **Not verified**: no GPU. Not needed: the CPU pass takes 0.12–0.23 s at 50 aircraft |
| **ONNX Runtime (C++)** | Adopted | RUL inference via FFI | Latency p99: 0.057 ms (1 row), 0.080 ms (100), 0.423 ms (1,000); quantiles ordered |
| **Python** (1.3%) | Training and test tooling only | `ml/`, `tests/security` | Not used at runtime |
| **TypeScript / React** (15%) | Adopted | `apps/command`, `workers/temporal` | Strict `tsc`; Playwright UI run (27 screenshots, 5 roles, mobile); axe-core 16/16 |
| **PostgreSQL** | Adopted, verified live | Migrations 0001–0004 | Integration, restart, outage, backup/restore, concurrency |
| **TimescaleDB** | Hypertable when available | `param_samples` | **Partially verified**: fallback verified; extension not installed here |
| **Apache Kafka** | Implemented | `bus.rs`, worker consumer | **Verified on a Kafka-protocol broker (Tansu)**: produce, consume, duplicates, poison messages, consumer restart. Apache Kafka binary itself **blocked** |
| **Temporal** | Implemented | `workers/temporal` | **Verified live**: case SLA and pre-departure workflows on a real server, worker stop/restart, idempotent starts, 3 workflow tests on the CLI server |
| **Keycloak** | Implemented | Realm export, JWKS validation | **Verified live**: service identity, forged tokens, foreign issuer, PKCE browser login, forced password change, TOTP enrolment and use |
| **OpenTelemetry** | Implemented | OTLP/HTTP export | **Verified live**: request and `analysis_pass` spans from both services in the collector |
| **Prometheus / Grafana** | Adopted | `/metrics`, scrape config, dashboard | Prometheus scraping both services **verified**; Grafana not run |
| **TLS (rustls, ring)** | Implemented | Optional TLS 1.3-only listener | **Verified**: TLS 1.3 accepted, TLS 1.2 and plaintext refused, HTTP/2 via ALPN |
| **Neo4j** | Implemented | `graph.rs` projection | **Blocked**: server cannot be downloaded. In-process graph serves the UI and is verified |
| **HashiCorp Vault** | Implemented | KV v2 loader | **Blocked**: binary download and Go module proxy refused. Environment fallback verified |
| **Docker / Kubernetes** | Implemented | Dockerfiles, `deploy/k8s` | **Blocked**: base images cannot be pulled; manifests reviewed only |

Evaluated and not adopted: Redis (an in-process per-snapshot response cache serves the same need
without a consistency tier), Apache Arrow, gRPC, ZeroMQ and MinIO (no Phase 0 need, or duplicates
Kafka), Ada/SPARK (recommended for any future airborne component), TPM/HSM, SPIFFE/SPIRE and OPA
(deferred to production key management and multi-tenancy), ML-KEM/ML-DSA (depends on ingress and
mesh; custom cryptography forbidden), Suricata/Zeek/eBPF (platform layer). Sigstore/SLSA steps are in
CI but not executed here.

---

## B. Feature testing report

| Suite | Tests | Passed | Failed |
|---|---|---|---|
| Rust unit + property (`cargo test --workspace`) | 28 (9 proptest properties, up to 300 cases each) | 28 | 0 |
| C++ unit + fuzz under ASan/UBSan | 7 groups, 4,000 fuzz inputs | all | 0 |
| TypeScript worker: unit | 5 | 5 | 0 |
| TypeScript worker: workflows on a real Temporal server | 3 | 3 | 0 |
| API integration on PostgreSQL (`tests/integration/api.mjs`) | 35 | 35 | 0 |
| Persistence after restart | 5 | 5 | 0 |
| Concurrency and races (`tests/integration/concurrency.mjs`) | 10 checks | 10 | 0 |
| OIDC attack suite | 12 | 12 | 0 |
| Live Keycloak (`tests/live/keycloak.mjs`) | 6 | 6 | 0 |
| Live Kafka-protocol + Temporal (`tests/live/live-infra.mjs`) | 7 | 7 | 0 |
| TLS 1.3 (`tests/security/tls13.sh`) | 4 | 4 | 0 |
| Reliability: outage, recovery, backup/restore | 7 | 7 | 0 |
| Accessibility, axe-core WCAG A/AA | 16 screens | 16 | 0 |
| UI end-to-end (Playwright, 27 screenshots) | 1 run | pass | 0 |
| Detection quality on labelled replay data | 19 scenarios | 19 | 0 |

First runs of the new suites failed before fixes: live Kafka/Temporal 2 of 7 passed (three failures
were invalid test events, corrected in the test; two were B13 and B14), live Keycloak 3 of 6 (B15,
B16, missing `sub`), accessibility 5 of 16 screens (contrast), concurrency 1 of 2 load checks (B8,
B9, B10), and the metrics showed 7 of 15 alerts matched because flight alerts were not matched to
their labelled scenarios (metrics defect, fixed). Nothing was skipped or suppressed.

| Module | Tested | Result | New defects fixed |
|---|---|---|---|
| Integrity | REST and Kafka ingestion, schema version, duplicates, poison messages, detectors, evidence, acknowledgement | Pass | B7, B14 |
| Predict | RUL quantiles, registry, hash check, drift (PSI), rollback across restart, reliability by ATA/team, inference latency | Pass | B19 |
| Prevent | 9 categories, weather/NOTAM context, escalation (embedded and Temporal), stale-data invariant (proptest), indexed build equals per-departure build (proptest) | Pass | B11 |
| Workflow | Case SLA and pre-departure workflows on Temporal, worker restart, alert acknowledgement and escalation | Pass | B13 |
| Flight | 7 event types, weather context, fault correlation, limitation statement, track, post-flight summary, role gating | Pass | None |
| Guardian | Conflicting indicators (never notify), procedure cards, ATC message with METAR and alternates (never transmitted), all earlier drill tests | Pass | None |
| Autoland | Capability registry; no command interface | Pass (boundary) | None |
| Passport | Versioned corrections, history, concurrency, restart | Pass | B8 |
| Command | All screens incl. Flight, Reliability, Admin, model registry; CSV downloads; acknowledge; corrections; contrast | Pass | Contrast tokens (11 screens) |
| Cross-Flight Memory | Invariants (proptest), k-anonymous fleet patterns | Pass | None |
| Sentinel | Auth, MFA, tenant isolation, two-person erasure, TLS, audit integrity under concurrency | Pass | B9, B15, B16 |

### Defects found and fixed in the second iteration

| ID | Severity | Found by | Root cause | Fix | Regression test |
|---|---|---|---|---|---|
| B7 | High | Live Kafka design review | Ingestion not idempotent in memory | Skip known event ids; API reports `duplicate` | Integration DAT-06/INT-01; live duplicate test |
| B8 | High | Concurrency test | Correction version computed without serialisation; old value from stale snapshot | Async lock, insert-once, value from latest correction | Concurrency (sequential versions); integration PSP-02 |
| B9 | High | Concurrency test | Audit inserts spawned fire-and-forget; failures left gaps | Ordered single writer, retry until durable | Concurrency (chain intact); outage test (memory = database count) |
| B10 | Medium | Concurrency test | Synchronous recompute per decision on the async runtime | Coalesced pass with read-your-writes generations | Concurrency (200 mixed writes: 23 s with 64 errors → 0.39 s, 0 errors) |
| B11 | Medium | NFR-04 test | Quadratic per-departure and per-aircraft scans | Index by tail; `prevent::build_all` | Proptest `build_all_matches_build`; 500-aircraft run |
| B12 | Medium | NFR-04 test | Pass held the engine lock | Pass on a shared copy (`Arc` copy-on-write) | 500-aircraft ingest 6.8 → 1,021 req/s |
| B13 | Medium | Live Temporal | Double `worker.shutdown()` | Leave shutdown to the SDK | Live worker graceful-stop test |
| B14 | High | Live Kafka | SIGTERM ignored; consumer never left its group | `shutdown_signal()` in both services; 10 s session | Live core-restart test (resume in 1 s) |
| B15 | High | Live Keycloak | Realm scope lacked role, username and `sub` mappers | Mappers added to the API audience scope | Live service-identity and browser-login tests |
| B16 | High | Live Keycloak | Temporary password not enforced | `UPDATE_PASSWORD` required action | Live first-login test |
| B18 | Medium | Live OTel | Request spans at DEBUG never exported | Spans at INFO; `analysis_pass` span | Collector received spans from both services |
| B19 | High | Test review | Model without a hash was loaded | Fail closed | `registry_loads_hash_verified_versions_and_rejects_tampering` |
| B20 | Medium | Perf after ingest flood | Linear event lookup per case in metrics | Hash index | Pass after 12.5 k events: 642 → 233 ms |
| B21 | Medium | Perf after ingest flood | Multi-MB responses rebuilt per request | Per-snapshot response cache | Soak after flood 70 → 197 req/s |

B17 was investigated and is not a defect: JWKS refresh is limited to once per 60 s to resist kid
flooding. Earlier defects B1–B6 remain fixed and their tests still pass.

---

## C. Security report

| Area | Control | Test executed | Result |
|---|---|---|---|
| Authentication | RS256 JWT, JWKS, issuer/audience/expiry, unknown roles dropped | 12 OIDC attacks; live forged-signature, payload-tampering, `alg=none`, foreign-issuer tests | All rejected |
| MFA and account security | TOTP required, forced password change, no password grant for the UI client, brute-force protection | Live browser login and second login with wrong and right code | Pass (after B15/B16) |
| Service identity | Client credentials with a single role | Live: admin endpoint refused; worker escalations audited under the service subject | Pass |
| Authorisation | Per-endpoint roles; engineer-only confirmation; two-administrator erasure | Integration 403 matrix; same-admin approval refused | Pass |
| Tenant isolation | Lessor scoping, redaction, k-anonymous patterns | Integration (no tail disclosed) | Pass |
| Input handling | Typed parsing, schema version, body limit, parameterised SQL, CSV formula neutralisation | Integration, live poison messages | Pass |
| Transport | TLS 1.3 only (rustls + ring), HTTP/2 | `tls13.sh` | Pass |
| Audit integrity | Hash chain, append-only trigger, ordered lossless writer | Proptest (mutation/deletion), concurrency, outage, restore | Pass |
| Model integrity | SHA-256 verification, fail closed | Tamper and missing-hash tests | Pass |
| Memory safety | Rust; C++ fuzzed under ASan/UBSan; FFI property tests | As above | No errors in AIPS code |
| Dependencies | `npm audit` (both projects), `cargo audit` (347 crates) | Executed | npm: 0 vulnerabilities. cargo: 0 unignored vulnerabilities; 4 unmaintained transitive crates (neo4rs, axum-server); RUSTSEC-2023-0071 reviewed exception (sqlx-mysql never compiled) |
| Emergency safety | Training only; ATC never transmitted; contradicting evidence never notifies | Integration and unit tests | Pass |

Residual risks: permissive CORS in development (restrict at ingress); SSE token in query string;
single core replica; encryption at rest and key rotation not verified; Vault, Neo4j and image
scanning blocked; no independent penetration test. No claim is made that the application is free of
vulnerabilities. Full analysis: [`THREAT_MODEL.md`](THREAT_MODEL.md).

---

## D. Performance report

Release build, 2 vCPU, PostgreSQL attached, `tests/perf/load.mjs`. Raw results in
`docs/perf-results.jsonl`.

| Run | Concurrency | Throughput | p50 | p95 | p99 | Errors | RSS start → peak |
|---|---|---|---|---|---|---|---|
| Read, 50 aircraft | 50 | 2,435 req/s | 18.5 ms | 39.8 ms | 55.5 ms | 0 | 411 → 412 MB |
| Spike, 50 aircraft | 200 | 2,538 req/s | 72.6 ms | 139 ms | 201 ms | 0 | 412 → 419 MB |
| Ingest, 50 aircraft | 20 | 1,099 events/s | 15.3 ms | 37.8 ms | 59.7 ms | 0 | 419 → 599 MB |
| Spike after a 16.5 k synthetic-event flood | 200 | 144 req/s | 945 ms | 2.9 s | 7.9 s | 0 | 599 → 755 MB |
| Soak 60 s after the flood | 20 | 197 req/s | 78.5 ms | 250 ms | 445 ms | 0 | 619 → 716 MB |
| **NFR-04: read, 500 aircraft** | 50 | 246 req/s | 189 ms | 370 ms | 466 ms | 0 | 1.6 → 2.2 GB |
| **NFR-04: ingest, 500 aircraft** | 20 | 1,021 events/s | 12.8 ms | 36.3 ms | 68.5 ms | 0 | 2.1 → 4.1 GB |
| Concurrency burst, 200 mixed writes | 200 | 0.39 s total | n/a | n/a | n/a | 0 | None |

Analysis pass: 0.12–0.23 s at 50 aircraft; **1.46 s at 500 aircraft** (was 15.5 s before B11/B12).
Stage timings are exposed on `/api/metrics` as `stages_ms`. At 500 aircraft the remaining cost is
dataset projection (0.56 s) and series indexing (0.45 s). ONNX inference: p99 0.42 ms for 1,000
rows.

Open findings: response size grows with history (`/api/memory/hypotheses` reached 5.6 MB after the
flood), which limits throughput at large histories. Pagination is the remedy but changes the API
contract, so it was not introduced without approval. Peak memory at 500 aircraft under ingest
(4.1 GB) comes from one copy-on-write dataset copy per pass; incremental analysis would remove it.

Against PRD targets (synthetic data): pre-departure report 0.2–0.4 ms (target < 60 s); fault to case
measured live, well under 15 min; ingest-to-alert at 500 aircraft about 2 s (target p95 < 5 s);
alerts 0.3 per aircraft per week (target ≤ 5); scenario recall 100% (19 of 19); hypothesis precision
100% (11 of 11); alert precision 87% (13 of 15, target ≥ 70%). The two unmatched alerts are detector
shifts on SE-RFK with no labelled scenario, that is, false positives on synthetic noise. The recall and precision figures demonstrate the pipeline only; they are not evidence of
operational performance.

---

## E. Reliability report

| Scenario | Result |
|---|---|
| Core restart (PostgreSQL) | Dataset, decisions (grades, acknowledgements, corrections, escalations, erasures, model activation), clock and audit chain reloaded; chain intact |
| Graceful stop (SIGTERM) | Both services drain and exit; Kafka consumer leaves its group; restart consumes within 1 s (B14) |
| Core down while Kafka messages arrive | Consumed after restart from committed offsets; nothing lost (live) |
| Worker down while a case opens | Workflow started on restart from the Kafka backlog (live) |
| Duplicate and poison Kafka messages | Duplicates ignored and counted; malformed, wrong-schema and unknown-tail messages rejected and counted; consumer continues (live) |
| Duplicate workflow starts | Rejected by workflow-id uniqueness (real Temporal server) |
| PostgreSQL outage | Reads continue; writes refused; no half-applied decision; audit entries queued and persisted in order on recovery (memory 32 = database 32, chain intact) |
| Backup and restore | `pg_dump` 0.85 s (5.7 MB), restore 1.59 s, 10 tables match, append-only trigger preserved |
| Concurrent identical decisions | Exactly one of 40 acknowledgements wins; erasure execution claimed atomically |
| Emergency service, database down | Drills continue on the local journal |

Remaining risks: single core replica (no HA); Neo4j and Vault failure modes not exercised (blocked);
the database pool timeout is long for interactive writes.

---

## F. Final status

| Status | Count | First iteration |
|---|---|---|
| IMPLEMENTED AND VERIFIED | 76 | 59 |
| IMPLEMENTED BUT NOT VERIFIED | 5 | 6 |
| PARTIALLY IMPLEMENTED | 9 | 21 |
| FAILED | 0 | 0 |
| BLOCKED | 1 | None |
| NOT IMPLEMENTED (later phase) | 0 | 5 |
| NOT APPLICABLE (justified) | 4 | 4 |

Total 95 requirements. Per-ID evidence is in [`REQUIREMENTS_TRACE.md`](REQUIREMENTS_TRACE.md).

Infrastructure items outside the PRD ID list:

| Item | Status | Reason |
|---|---|---|
| Neo4j live projection | BLOCKED | Server download refused |
| Vault live secret loading | BLOCKED | Binary and Go module proxy refused |
| Apache Kafka broker | BLOCKED | Download refused; Kafka protocol verified with Tansu |
| Container builds, Trivy scan, cosign, SLSA | BLOCKED | Registries refused; steps defined in CI |
| Grafana dashboards | IMPLEMENTED BUT NOT VERIFIED | Provisioned, not run |
| CUDA kernel | IMPLEMENTED BUT NOT VERIFIED | No GPU; CPU path verified and sufficient |

No certification, regulatory approval or operational readiness is claimed.

---

## Change log (second iteration)

1. Flight module (domain analysis, API, UI), weather and NOTAM context, post-flight summary.
2. Alert acknowledgement and escalation; Temporal pre-departure workflow; reliability view; CSV
   reports; k-anonymous fleet patterns; versioned corrections; two-person erasure; model registry,
   drift and rollback; ingest schema versioning; Guardian disagreement, procedure cards and ATC
   message content.
3. Optional TLS 1.3 listener; SIGTERM handling; configurable fleet size (`AIPS_REPLAY_FLEET`).
4. Defects B7–B21 fixed with regression tests.
5. New suites: proptest (domain and FFI), concurrency, inference latency, drift/tamper, live Kafka
   and Temporal, live Keycloak, TLS, axe-core accessibility.
6. Migrations `0003_flight.sql` and `0004_decisions.sql`, both additive (rollback: drop the new
   tables). No existing migration or data was changed.
7. Keycloak realm: role, username and `sub` mappers; forced password change.
8. Removed the unmaintained direct dependency `rustls-pemfile` (PEM parsing via `rustls-pki-types`).
