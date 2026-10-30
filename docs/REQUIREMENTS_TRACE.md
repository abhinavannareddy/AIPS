# Requirements trace

Every PRD requirement ID with its Phase 0 status. Statuses follow our production-readiness checklist; "verified" means a test or measurement in `docs/ENGINEERING_REPORT.md` exercised it.

| Status | Count |
|---|---|
| IMPLEMENTED AND VERIFIED | 76 |
| IMPLEMENTED BUT NOT VERIFIED | 5 |
| PARTIALLY IMPLEMENTED | 9 |
| FAILED | 0 |
| BLOCKED | 1 |
| NOT APPLICABLE (justified) | 4 |

Total: 95

| ID | Requirement | Status | Evidence or gap |
|---|---|---|---|
| INT-01 | Ingest aircraft health data | IMPLEMENTED AND VERIFIED | REST ingestion and the Kafka consumer verified end to end: live run against a Kafka-protocol broker (Tansu 0.6) with PostgreSQL persistence, duplicates, poison messages and consumer restart (tests/live/live-infra.mjs 7/7). Apache Kafka itself not run (download blocked) |
| INT-02 | Abnormal behaviour per system/component | IMPLEMENTED AND VERIFIED | CUSUM/EWMA/slope via C++; all labelled sensor scenarios detected; 13 of 15 non-hypothesis alerts match a labelled scenario (2 false positives on synthetic noise) |
| INT-03 | Sensor reliability (frozen, range, inconsistent) | IMPLEMENTED AND VERIFIED | Frozen, out-of-range and redundant-sensor disagreement detected |
| INT-04 | Cross-system correlation with evidence grade | IMPLEMENTED AND VERIFIED | Lift against fleet base rate; both cross-system scenarios found |
| INT-05 | Alert shows source, data age, model version, reason | IMPLEMENTED AND VERIFIED | Integration test asserts every alert |
| INT-06 | Engineer feedback stored | IMPLEMENTED AND VERIFIED | Feedback persisted and counted in metrics |
| PRD-01 | RUL with uncertainty range | IMPLEMENTED AND VERIFIED | ONNX p10/p50/p90; UI never shows p50 alone |
| PRD-02 | Degradation trends | IMPLEMENTED AND VERIFIED | EGT trend per engine; fleet RUL ranking |
| PRD-03 | Recurring defects | IMPLEMENTED AND VERIFIED | 5/5 recurrence scenarios |
| PRD-04 | Repeated unsuccessful repairs | IMPLEMENTED AND VERIFIED | Repeat-after-fix scenarios detected with fixes counted |
| PRD-05 | Auto-open investigation case | IMPLEMENTED AND VERIFIED | Measured time-to-case |
| PRD-06 | Fleet reliability and maintenance effectiveness views | IMPLEMENTED AND VERIFIED | GET /api/reliability: corrective-action effectiveness (no repeat within 30 days) by ATA chapter and team; Reliability page; integration test |
| PRV-01 | Assemble pre-departure evidence and compare with history | IMPLEMENTED AND VERIFIED | All nine categories; history via hypotheses and cases |
| PRV-02 | Five classifications | IMPLEMENTED AND VERIFIED | Plus Insufficient data (PRV-06) |
| PRV-03 | Rank by severity/grade with freshness | IMPLEMENTED AND VERIFIED | Ranked by status, then severity, then evidence grade; health-feed and maintenance-system freshness on every report. Proptest `prevent_ranks_concerns_and_shows_freshness` (added in the pre-push check; fails when the severity order is reversed) |
| PRV-04 | Escalate significant concerns with acknowledgement | IMPLEMENTED AND VERIFIED | Unreviewed significant concerns escalate 45 min before STD: embedded timer, or Temporal preflightConcernWorkflow -> POST /api/preflight/{id}/escalate. Verified on a real Temporal server (live test) and in integration tests |
| PRV-05 | Disclaimer on report and screens | IMPLEMENTED AND VERIFIED | Asserted in tests |
| PRV-06 | Stale data shows Insufficient data | IMPLEMENTED AND VERIFIED | Asserted in tests |
| PRV-07 | Forward alerts to approved cockpit interface | BLOCKED | Needs an approved aircraft/cockpit interface and explicit approval before connecting to aircraft systems (approval rules in the production-readiness checklist). Not attempted |
| WRK-01 | Pre-flight retrieval and completeness | PARTIALLY IMPLEMENTED | Technical status, defects, MEL, documentation, plus departure/arrival weather and NOTAM context in every report. Weather and NOTAMs come from a deterministic synthetic provider; no live METAR/NOTAM feed is connected |
| WRK-02 | Maintenance workflow | IMPLEMENTED AND VERIFIED | Cases, assignment, corrective action, documentation check |
| WRK-03 | In-flight alerts, ground notification, acknowledgement | IMPLEMENTED AND VERIFIED | POST /api/alerts/{id}/acknowledge (attributed, atomic, persisted); unacknowledged Warnings escalate after 30 min (audited, published to aips.alerts); 40-way concurrent acknowledgement test: exactly one winner |
| WRK-04 | Post-flight processing | IMPLEMENTED AND VERIFIED | Post-flight summary per flight (new defects with on-tail history, corrective actions, flight events, status) in /api/flight/{id} and the Flight page |
| WRK-05 | Workflows survive restarts and escalate on timeout | IMPLEMENTED AND VERIFIED | Embedded escalation and restart recovery verified; Temporal caseSlaWorkflow and preflightConcernWorkflow verified on a real Temporal server, including worker stop/restart with no missed case (live test) |
| WRK-06 | Human override, reassignment, closure reasons | IMPLEMENTED AND VERIFIED |  |
| FLT-01 | Trajectory/speed/altitude monitoring | IMPLEMENTED AND VERIFIED | Trajectory summaries per flight (ADS-B/QAR substitute in Replay Lab): unstable approach, rapid descent, speed > 250 kt below FL100, cruise deviation, weak climb, data gaps, delayed data. All injected flight scenarios detected (integration test). Synthetic data only |
| FLT-02 | Flight-event correlation with weather | IMPLEMENTED AND VERIFIED | Each flight event carries the METAR-style weather at the relevant airport and links faults within 15 min on the same flight (graded Corroborated, never Confirmed). Weather is synthetic |
| FLT-03 | Flight safety event timeline | IMPLEMENTED AND VERIFIED | Flight intelligence board and per-flight timeline with track chart, weather, NOTAMs and events |
| FLT-04 | No intent inference | IMPLEMENTED AND VERIFIED | Every flight event carries a limitation statement (no intent or cause); asserted in tests. Guardian uses observable indicators only |
| GRD-01 | Multiple independent indicators | IMPLEMENTED AND VERIFIED | Noisy-OR fusion with >= 2 classes |
| GRD-02 | Confidence and contributing indicators | IMPLEMENTED AND VERIFIED |  |
| GRD-03 | Read-only door/security status | PARTIALLY IMPLEMENTED | Simulated door indicators in drills; no aircraft integration |
| GRD-04 | Cabin events (fire, medical, altercation) | PARTIALLY IMPLEMENTED | Drill scenarios only |
| GRD-05 | No face recognition or appearance classification | IMPLEMENTED AND VERIFIED | No camera analysis exists |
| GRD-06 | Notify crew on two independent channels | IMPLEMENTED AND VERIFIED |  |
| GRD-07 | Track acknowledgement; on board does not mean available; escalate | IMPLEMENTED AND VERIFIED |  |
| GRD-08 | Show procedures to acknowledging crew | IMPLEMENTED AND VERIFIED | Procedure cards loaded from AIPS_GUARDIAN_PROCEDURES (JSON) and attached to drills; shipped cards are labelled TRAINING EXAMPLE. Operator-approved content must come from the operations manual |
| GRD-09 | Cancel path recorded | IMPLEMENTED AND VERIFIED |  |
| AUT-01 | Only certified capability; documented per type | IMPLEMENTED AND VERIFIED | Capability registry; no fleet type has one |
| AUT-02 | Forward events to certified system, never command | NOT APPLICABLE (justified) | No certified system to integrate in Phase 0; no command interface exists |
| AUT-03 | Destination/weather info to ATC and ops | IMPLEMENTED AND VERIFIED | ATC/operations message composed at escalation with destination METAR and two alternates; logged and marked NOT transmitted in training (unit and integration tests) |
| AUT-04 | Preserve emergency timeline | IMPLEMENTED AND VERIFIED | Hash-chained journal |
| AUT-05 | Airborne development under DO-178C etc. | NOT APPLICABLE (justified) | No airborne component in Phase 0 |
| PSP-01 | Complete aircraft record | IMPLEMENTED AND VERIFIED |  |
| PSP-02 | Source, author, timestamp, grade; append-only with versioned corrections | IMPLEMENTED AND VERIFIED | Versioned, attributed corrections (POST /api/records/corrections, GET /api/records/{type}/{id}/history); original never overwritten; correction rows append-only in PostgreSQL (trigger); concurrent corrections serialised (bug B8 fixed) |
| PSP-03 | Per-field sharing permissions | IMPLEMENTED AND VERIFIED | Lessor isolation and redaction tested |
| PSP-04 | Open export format | IMPLEMENTED AND VERIFIED | aips.passport/1.0 |
| CMD-01 | Fleet view | IMPLEMENTED AND VERIFIED |  |
| CMD-02 | Emergency management view | IMPLEMENTED AND VERIFIED |  |
| CMD-03 | Workflow monitoring and case management | IMPLEMENTED AND VERIFIED |  |
| CMD-04 | RBAC, least privilege, organisation isolation | IMPLEMENTED AND VERIFIED |  |
| CMD-05 | Reports and analytics with export | IMPLEMENTED AND VERIFIED | CSV exports for alerts, cases and pre-departure reports (RFC 4180, formula-injection neutralised, audited) plus detection-quality and reliability analytics |
| CMD-06 | Investigation timeline per aircraft | IMPLEMENTED AND VERIFIED |  |
| CFM-01 | Match new events against history | IMPLEMENTED AND VERIFIED |  |
| CFM-02 | Recurrence after documented fix | IMPLEMENTED AND VERIFIED |  |
| CFM-03 | Evidence grades; only engineers promote | IMPLEMENTED AND VERIFIED |  |
| CFM-04 | Explain why events were linked | IMPLEMENTED AND VERIFIED |  |
| CFM-05 | Fleet patterns without exposing other customers | PARTIALLY IMPLEMENTED | k-anonymous fleet patterns (k = 3, no tails or record ids) for lessors via /api/memory/fleet-patterns; cross-customer opt-in sharing needs a multi-tenant deployment |
| CFM-06 | Track open case across later flights | IMPLEMENTED AND VERIFIED |  |
| CFM-07 | Measure own precision | IMPLEMENTED AND VERIFIED |  |
| EMG-01 | Separate deployment and data store | IMPLEMENTED AND VERIFIED | Works with its database down |
| EMG-02 | Two independent channels per recipient | IMPLEMENTED AND VERIFIED |  |
| EMG-03 | Data loss raises a state and never clears an emergency | IMPLEMENTED AND VERIFIED |  |
| EMG-04 | Show indicator disagreement | IMPLEMENTED AND VERIFIED | Contradicting indicators reduce fused confidence, never qualify, and show an explicit disagreement statement; conflicting-indicators drill scenario; unit and integration tests |
| EMG-05 | Tamper-evident timeline with local buffer | IMPLEMENTED AND VERIFIED |  |
| EMG-06 | Pilot/crew can always cancel or downgrade | IMPLEMENTED AND VERIFIED |  |
| EMG-07 | Training mode that cannot notify real recipients | IMPLEMENTED AND VERIFIED | Live mode refused |
| DAT-01 | Source, owner, timestamp, ingestion time | IMPLEMENTED AND VERIFIED |  |
| DAT-02 | Owner controls visibility per field | PARTIALLY IMPLEMENTED | Lessor redaction only |
| DAT-03 | Export and delete at contract end | IMPLEMENTED AND VERIFIED | Export implemented; erasure per aircraft with a two-administrator rule (requester cannot approve), atomic execution claim, PostgreSQL and in-memory removal, audit chain retained and intact (integration test incl. restart) |
| DAT-04 | Train only on shared, anonymised data | NOT APPLICABLE (justified) | Synthetic data only in Phase 0 |
| DAT-05 | Configurable data residency | IMPLEMENTED BUT NOT VERIFIED | Deployment-time choice; not exercised |
| DAT-06 | Versioned connectors with freshness alerts | PARTIALLY IMPLEMENTED | Ingest contract version enforced (x-aips-schema: aips.fault.v1 on REST and Kafka; others rejected and counted); per-aircraft freshness monitored. Real airline/OEM connectors not built |
| NFR-01 | Availability 99.9% / 99.95% | IMPLEMENTED BUT NOT VERIFIED | Needs long-run operation |
| NFR-02 | Ingest-to-alert p95 < 5 s | IMPLEMENTED AND VERIFIED | Ingest p95 23 ms plus analysis cadence <= 0.5 s and pass 0.12-0.6 s |
| NFR-03 | Emergency alert to crew p95 < 3 s | IMPLEMENTED BUT NOT VERIFIED | Simulated channels only |
| NFR-04 | 500 aircraft per tenant | IMPLEMENTED AND VERIFIED | 500-aircraft fleet (212,717 flights, 2.1 M samples): analysis pass 1.46 s (was 15.5 s; bugs B11/B12 fixed), reads 246 req/s p95 370 ms, ingest 1,021 req/s p95 36 ms, zero errors. Single node, synthetic data |
| NFR-05 | Encryption in transit and at rest | IMPLEMENTED BUT NOT VERIFIED | In transit: optional in-process TLS 1.3-only listener verified (TLS 1.2 and plaintext refused; tests/security/tls13.sh). At rest: delegated to PostgreSQL storage encryption and Vault; not verified here |
| NFR-06 | DO-355-aligned process and threat model | PARTIALLY IMPLEMENTED | STRIDE threat model written (docs/THREAT_MODEL.md) with controls mapped to tests; a DO-326A/DO-355 process and independent security assessment are outside this build |
| NFR-07 | RBAC, SSO, MFA | IMPLEMENTED AND VERIFIED | RBAC and token validation verified; MFA verified live with Keycloak 26: first login forces password change and TOTP enrolment, second login requires TOTP, wrong code refused (tests/live/keycloak.mjs 6/6) |
| NFR-08 | Tamper-evident audit | IMPLEMENTED AND VERIFIED |  |
| NFR-09 | Privacy and DPIA before Guardian pilot | NOT APPLICABLE (justified) | No personal or physiological data processed in Phase 0 |
| NFR-10 | Model lineage, evaluation, drift, rollback | IMPLEMENTED AND VERIFIED | Model registry with SHA-256-verified artefacts (fail closed), PSI drift per input feature, administrator activation/rollback that is audited and survives restarts; tests for drift, tamper, rollback and latency |
| NFR-11 | Explainable alerts | IMPLEMENTED AND VERIFIED |  |
| NFR-12 | No single failure stops emergency path | PARTIALLY IMPLEMENTED | Database failure verified; region and node failures not tested |
| NFR-13 | Traces, metrics, logs, freshness | IMPLEMENTED AND VERIFIED | OTLP trace export verified (request and analysis_pass spans received by otelcol-contrib 0.116 from both services; bug B18 fixed); Prometheus 3.0 scraping both services verified. Grafana dashboards provisioned but not run |
| NFR-14 | WCAG 2.2 AA | IMPLEMENTED BUT NOT VERIFIED | axe-core WCAG 2.0/2.1/2.2 A and AA automated rules: 16/16 screens pass after contrast fixes. Automated tools cover part of WCAG; a manual audit (screen reader, keyboard-only walkthrough) has not been done |
| P1 | Human authority | IMPLEMENTED AND VERIFIED | No approve/deny actions exist |
| P2 | Evidence before assertion | IMPLEMENTED AND VERIFIED |  |
| P3 | No single point of failure | PARTIALLY IMPLEMENTED | Emergency path independent; core runs as one replica |
| P4 | Fail safe and visible | IMPLEMENTED AND VERIFIED | Insufficient data state; stale indicators |
| P5 | Observable events only | IMPLEMENTED AND VERIFIED |  |
| P6 | Minimal personal data | IMPLEMENTED AND VERIFIED | None processed |
| P7 | Full audit trail | IMPLEMENTED AND VERIFIED |  |
| P8 | Honest capability claims | IMPLEMENTED AND VERIFIED | Autoland shown as not available; synthetic-data notices |
