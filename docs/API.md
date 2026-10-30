# AIPS API

All endpoints return JSON. Authentication: `Authorization: Bearer <Keycloak access token>` (production) or,
only with `AIPS_AUTH_MODE=dev`, the headers `x-aips-role`, `x-aips-user`, `x-aips-org`. Server-sent
event streams accept `access_token` (or `role` in dev mode) as a query parameter. Errors are
`{"error": "<message>"}` with 400, 401, 403, 404, 409, 413, 415, 422, 500 or 503.

After a human decision (grade, feedback, acknowledgement, correction, case action, escalation,
erasure, model activation) the response returns once an analysis pass that includes the decision has
committed, so an immediate read reflects it (bounded wait, 10 s).

Roles: `engineer`, `mcc_controller`, `flight_ops`, `safety_manager`, `cabin_crew`, `lessor`, `admin`.

## aips-core (port 8090)

| Method | Path | Roles | Purpose |
|---|---|---|---|
| GET | `/api/health`, `/livez`, `/readyz`, `/metrics` | public | Status, probes, Prometheus metrics |
| GET | `/config.json` | public | Runtime UI configuration |
| GET | `/api/me` | any | Current principal |
| GET | `/api/overview` | read roles | Fleet summary, alerts, departures |
| GET | `/api/fleet` | read roles (lessor: own aircraft) | Aircraft views sorted by health |
| GET | `/api/aircraft/{tail}` | read roles | Health, parameters, components, RUL, alerts, hypotheses, cases, MEL, pre-departure |
| GET | `/api/aircraft/{tail}/series?param=` | engineer, mcc, ops, safety | Parameter history with detector features |
| GET | `/api/aircraft/{tail}/timeline` | read roles | Safety record (45 days) |
| GET | `/api/aircraft/{tail}/passport/export` | read roles | `aips.passport/1.0` export (lessor redacted) |
| GET | `/api/alerts?tail=&module=` | engineer, mcc, ops, safety | Alerts with evidence |
| POST | `/api/alerts/{id}/feedback` `{useful, comment}` | engineer, mcc, safety | INT-06 feedback |
| POST | `/api/alerts/{id}/acknowledge` | engineer, mcc, safety | WRK-03 acknowledgement; 409 if already acknowledged. Unacknowledged Warnings escalate after 30 min |
| GET | `/api/flight/events?tail=&kind=` | engineer, mcc, ops, safety | FLT-01..03 flight-data events with weather, correlation and limitation |
| GET | `/api/flight/{flight_id}` | engineer, mcc, ops, safety | Flight view: track, weather, NOTAMs, events, data quality, post-flight summary |
| POST | `/api/records/corrections` `{record_type, record_id, field, new_value, reason}` | engineer, mcc | PSP-02 versioned correction (`fault_event`: message, code, ata, severity; `work_order`: note, signed_off) |
| GET | `/api/records/{type}/{id}/history` | engineer, mcc, safety, lessor (own) | Original record and every correction |
| GET | `/api/reliability` | engineer, mcc, safety | PRD-06 corrective-action effectiveness by ATA chapter and team |
| GET | `/api/reports/{alerts,cases,preflight}.csv` | engineer, mcc, safety, ops | CMD-05 CSV export (audited) |
| GET | `/api/memory/fleet-patterns` | engineer, safety, lessor | CFM-05 k-anonymous fleet patterns (k = 3), no tails or record ids |
| GET | `/api/models/rul` | engineer, safety, admin | Model registry, active version, input drift (PSI) |
| POST | `/api/models/rul/activate` `{version, reason}` | admin | NFR-10 activation or rollback (hash-verified, audited, survives restart) |
| GET/POST | `/api/admin/erasure-requests` `{tail, reason}` | admin | DAT-03 erasure requests |
| POST | `/api/admin/erasure-requests/{id}/approve` | admin (not the requester) | Executes the erasure; audit chain retained |
| GET | `/api/memory/hypotheses?tail=` | read roles | Hypotheses with linked events and repairs |
| POST | `/api/memory/hypotheses/{id}/grade` `{grade, note}` | engineer | Change evidence grade (confirmation needs a note) |
| GET | `/api/memory/graph?tail=&source=neo4j` | read roles | Memory graph (Neo4j when connected, else in-process) |
| GET | `/api/preflight`, `/api/preflight/{flight_id}` | engineer, mcc, ops, safety | Pre-departure reports |
| POST | `/api/preflight/{flight_id}/acknowledge` | ops, engineer, mcc | Record a review (not a dispatch decision) |
| POST | `/api/preflight/{flight_id}/escalate` | mcc, admin | PRV-04, called by the Temporal worker; aips-core re-checks the report and decides |
| GET | `/api/cases`, `/api/cases/{id}` | engineer, mcc, safety, ops | Investigations |
| POST | `/api/cases/{id}/assign` `{assignee, team?}` | mcc, engineer | Assign |
| POST | `/api/cases/{id}/status` `{status, note}` | engineer, safety, mcc | Status; closing needs a note of 10+ characters |
| POST | `/api/cases/{id}/escalate` `{reason, level?}` | mcc, admin | Called by the Temporal worker |
| GET | `/api/metrics` | engineer, mcc, safety | Detection quality and KPIs |
| GET | `/api/audit?limit=`, `/api/audit/verify` | safety, engineer (+mcc for list) | Audit trail and chain verification |
| GET/POST | `/api/replay`, `/api/replay/control` `{action, hours?, speed?}` | read / engineer, safety, admin | Simulation clock |
| POST | `/api/replay/regenerate` `{seed}` | admin | New synthetic fleet |
| POST | `/api/ingest/events` (FaultEvent) | engineer, admin | INT-01 ingestion (to Kafka when connected). Optional header `x-aips-schema: aips.fault.v1`; any other value is 415. Response includes `duplicate` (idempotent by event id) |
| GET | `/api/stream` | any | Server-sent events: `snapshot` messages |

FaultEvent: `{id, tail, flight_id|null, ts, ata, system, code, component_id|null, severity, message, source, ingested_at}`
with `severity` in `info|advisory|caution|warning`, `system` one of the domain systems (the server recomputes it from `ata`; use `other` if unsure), and `source` in
`acars|central_maintenance_computer|tech_log|qar|pilot_report|maintenance_system`.

## aips-emergency (port 8091): training only

| Method | Path | Roles | Purpose |
|---|---|---|---|
| GET | `/api/emergency/health`, `/livez`, `/readyz`, `/metrics` | public | Status and probes |
| GET/POST | `/api/emergency/drills` `{tail, type_code, flight?, dest?, scenario, training: true}` | list: safety, ops, engineer, mcc, crew; create: safety, ops, engineer, mcc | Drills; `training:false` is refused (403) |
| GET | `/api/emergency/drills/{id}` | as list | Incident with journal |
| POST | `/api/emergency/drills/{id}/ack` `{crew_id, available}` | cabin_crew, safety, ops | Crew acknowledgement |
| POST | `/api/emergency/drills/{id}/simulate` `{action: feed_loss|feed_restore|channel_failure, channel?}` | trainers | Fault injection |
| POST | `/api/emergency/drills/{id}/cancel` / `resolve` `{reason}` | crew, safety, ops / safety, ops | Close with a reason |
| GET | `/api/emergency/drills/{id}/verify` | safety, ops, engineer | Journal hash-chain check |
| GET | `/api/emergency/capabilities/{type_code}` | trainers | Certified emergency landing capability (none in Phase 0) |
| GET | `/api/emergency/procedures` | safety, ops, engineer, mcc, crew | GRD-08 procedure cards (training examples unless configured) |
| GET | `/api/emergency/stream` | any | Server-sent events |

Scenarios: `dual_incapacitation`, `single_incapacitation`, `cockpit_security`, `cabin_fire`, `medical_emergency`, `false_indicator`, `conflicting_indicators`.

Incidents include `disagreement` (EMG-04), `procedure` (GRD-08) and, after level-2 escalation, `atc_message` with destination METAR and alternates (AUT-03, never transmitted in training).

## Kafka topics

| Topic | Producer | Consumer | Payload |
|---|---|---|---|
| `aips.ingest` | connectors | aips-core | FaultEvent JSON, header `x-aips-schema: aips.fault.v1`; at-least-once, committed after persistence |
| `aips.cases` | aips-core | worker | `case.opened`, `preflight.concern` |
| `aips.alerts` | aips-core | integrations | new and escalated alerts |
