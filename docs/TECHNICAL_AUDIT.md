# AIPS Phase 0: Technical audit

We did this audit on the first Phase 0 delivery, before starting the second iteration. It lists what
exists, how each part is built, where it falls short of the PRD or our production-readiness checklist,
and what we did about each finding. The evidence behind each disposition is in
`ENGINEERING_REPORT.md`.

## 1. Inventory

| Component | Language / runtime | Responsibility | Persistence |
|---|---|---|---|
| `crates/aips-domain` | Rust | Domain model, Cross-Flight Memory, Prevent rules, Flight analysis, audit chain | None |
| `crates/aips-replay` | Rust | Deterministic synthetic fleet (Replay Lab) with labelled scenarios | None |
| `crates/aips-platform` | Rust | OIDC/JWKS auth, Vault loader, telemetry, shutdown handling | None |
| `crates/aips-signal-sys` | Rust FFI | Safe bindings to the C++ library | None |
| `libs/aips-signal` | C++17 (optional CUDA) | Signal statistics, change detection, batch extraction, ONNX Runtime host | None |
| `services/aips-core` | Rust (axum, tokio) | Ingestion, analysis passes, API, decisions, projections | PostgreSQL/TimescaleDB, Neo4j (projection), Kafka |
| `services/aips-emergency` | Rust | Guardian training drills, notification state machine, journal | Own PostgreSQL + local journal |
| `workers/temporal` | TypeScript (Temporal SDK) | Durable case SLA and pre-departure escalation workflows | Temporal |
| `apps/command` | React + TypeScript | Operator UI | None |
| `ml/` | Python (training only) | RUL quantile model training, ONNX export, model cards | Model registry files |
| `deploy/` | Docker, Kubernetes, Keycloak, Vault, OTel, Prometheus, Grafana | Deployment assets | None |

Language share of source (excluding generated and vendored code): Rust 62%, TypeScript 15%,
C++/CUDA 13%, test JavaScript/shell 6%, CSS 2%, Python 1.3%, SQL 1%. Python stays within the 10%
allowance and is not used at runtime.

## 2. Findings

Severity: **High** = incorrect safety output, lost data, or security exposure; **Medium** = a
requirement not met or a scalability limit; **Low** = quality or maintainability.

### 2.1 Functional gaps against the PRD

| # | Finding | Sev. | Disposition |
|---|---|---|---|
| F1 | Flight module absent (FLT-01..03) | Medium | Implemented: trajectory summaries, seven observable event types, weather context, fault correlation, post-flight summary, UI page |
| F2 | No per-alert acknowledgement or escalation (WRK-03) | Medium | Implemented with atomic acknowledgement and 30-minute Warning escalation |
| F3 | No pre-departure escalation workflow (PRV-04) | Medium | Implemented in embedded mode and as a Temporal workflow |
| F4 | No versioned record corrections (PSP-02) | Medium | Implemented; append-only in PostgreSQL |
| F5 | No maintenance-effectiveness view (PRD-06) | Medium | Implemented (API and UI) |
| F6 | No report export (CMD-05) | Medium | CSV exports for alerts, cases, pre-departure reports |
| F7 | No data erasure (DAT-03) | Medium | Implemented with a two-administrator rule |
| F8 | No drift monitoring or rollback (NFR-10) | Medium | Model registry, hash verification, PSI drift, audited rollback |
| F9 | No explicit indicator disagreement (EMG-04) | Medium | Implemented |
| F10 | No procedure cards (GRD-08), ATC message lacked weather and alternates (AUT-03) | Medium | Implemented; procedure content labelled training example |
| F11 | Weather and NOTAMs missing from pre-departure context (WRK-01) | Medium | Synthetic provider integrated; live feeds not connected (partial) |
| F12 | Fleet patterns for lessors not available (CFM-05) | Low | k-anonymous aggregates implemented; cross-customer sharing needs multi-tenancy |

### 2.2 Verification gaps

| # | Finding | Sev. | Disposition |
|---|---|---|---|
| V1 | Kafka path never run against a broker | High | Run against Tansu (Kafka protocol): exposed B14; Apache Kafka itself blocked |
| V2 | Temporal workflows never run against a server | High | Run on the Temporal CLI dev server, including worker restart: exposed B13 |
| V3 | Keycloak never run live; MFA not exercised | High | Keycloak 26 run with the shipped realm: exposed B15, B16 and a missing `sub` claim |
| V4 | OTel export never verified | Medium | Verified with otelcol-contrib: exposed B18 |
| V5 | No property-based, concurrency, inference-latency or accessibility tests | Medium | Added (proptest, concurrency suite, latency benchmark, axe-core audit): exposed B8, B9, B10 |
| V6 | NFR-04 (500 aircraft) never tested | Medium | Tested: exposed B11, B12 |
| V7 | TLS only delegated, never tested | Medium | Optional TLS 1.3-only listener added and tested |
| V8 | Vault, Neo4j, container builds and image scanning not run | Medium | **Blocked**: binaries and images cannot be downloaded in this environment |

### 2.3 Defects found by the new verification (all fixed, all with regression tests)

| ID | Sev. | Defect |
|---|---|---|
| B7 | High | Duplicate ingest deliveries were added twice in memory, inflating recurrence counts |
| B8 | High | Concurrent record corrections collided on the same version (HTTP 500) and took the old value from a stale snapshot |
| B9 | High | Audit entries were persisted fire-and-forget; a pool timeout left a permanent gap in the chain |
| B10 | Medium | Every decision recomputed synchronously on the request thread; bursts exhausted the database pool |
| B11 | Medium | Quadratic loops in next-departure lookup, pre-departure reports and aircraft views (15.5 s pass at 500 aircraft) |
| B12 | Medium | The analysis pass held the engine lock, so ingestion and readers queued behind it at scale |
| B13 | Medium | Worker crashed with `IllegalStateError` on every graceful stop (double shutdown) |
| B14 | High | Services ignored SIGTERM; the Kafka consumer never left its group and ingestion stalled after restarts |
| B15 | High | Keycloak realm issued tokens without roles or `sub`; the worker's service identity would have been refused in production |
| B16 | High | Shipped demo passwords marked temporary were not forced to change at first login |
| B17 | None | (Investigated, not a defect) JWKS refresh is rate-limited to once per 60 s by design; documented |
| B18 | Medium | HTTP spans were created at DEBUG, so no traces were exported at the default log level |
| B19 | High | A model card without a hash was accepted (integrity check failed open) |
| B20 | Medium | Metrics computation scanned all events per case (420 ms per pass under load) |
| B21 | Medium | Multi-megabyte read responses were rebuilt for every request |

### 2.4 Security and architecture observations (not changed)

- CORS is permissive for the development setup; production must restrict it at the ingress.
- The SSE access token travels in the query string (browser limitation); tokens are short-lived.
- aips-core is a single replica with an in-memory snapshot; HA needs state partitioning (Phase 1).
- Payload sizes of `/api/memory/hypotheses` and `/api/cases` grow with history; pagination would
  change the API contract and was not introduced without approval.
- Transitive unmaintained crates (backoff, instant, paste, rustls-pemfile) come from neo4rs 0.8 and
  axum-server; no known vulnerabilities.

## 3. Constraints honoured

No aircraft control capability, no live emergency notifications, no connection to aircraft systems,
no proprietary or hand-written cryptography (rustls/ring, Keycloak, Vault, SHA-256 via `sha2`), no
deletion of existing functionality or migrations (migrations 0003 and 0004 are additive), no
destructive operations outside the local test databases.
