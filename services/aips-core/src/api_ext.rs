//! Extension endpoints closing the partial requirements found in the technical audit:
//! Flight Intelligence (FLT-*), alert acknowledgement (WRK-03), record corrections (PSP-02),
//! maintenance reliability (PRD-06), CSV reports (CMD-05), fleet-level patterns for lessors (CFM-05),
//! worker-driven pre-departure escalation (PRV-04), two-person erasure (DAT-03) and the model
//! registry with rollback (NFR-10).
use crate::engine::ErasureRequest;
use crate::state::Shared;
use aips_domain::intel::*;
use aips_domain::model::*;
use aips_platform::auth::roles::*;
use aips_platform::{ApiError, Principal};
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use chrono::{Duration, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

type ApiResult = Result<Json<Value>, ApiError>;

fn internal(e: impl std::fmt::Display) -> ApiError {
    // Details go to the server log only; database error text is not returned to clients.
    tracing::error!(error = %e, "internal error");
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
}

pub fn routes() -> Router<Shared> {
    Router::new()
        .route("/api/flight/events", get(flight_events))
        .route("/api/flight/{flight_id}", get(flight_one))
        .route("/api/alerts/{id}/acknowledge", post(alert_ack))
        .route("/api/records/corrections", post(correction))
        .route("/api/records/{rtype}/{id}/history", get(history))
        .route("/api/reliability", get(reliability))
        .route("/api/reports/{name}", get(report_csv))
        .route("/api/memory/fleet-patterns", get(fleet_patterns))
        .route(
            "/api/preflight/{flight_id}/escalate",
            post(preflight_escalate),
        )
        .route(
            "/api/admin/erasure-requests",
            get(erasure_list).post(erasure_request),
        )
        .route(
            "/api/admin/erasure-requests/{id}/approve",
            post(erasure_approve),
        )
        .route("/api/models/rul", get(models))
        .route("/api/models/rul/activate", post(model_activate))
}

fn tail_ok(s: &Shared, p: &Principal, tail: &str) -> bool {
    // Same scoping rule as api::visible_tail (PSP-03): lessors see only their own aircraft.
    let e = s.engine.read();
    e.snap
        .visible
        .aircraft
        .iter()
        .find(|a| a.tail == tail)
        .is_some_and(|ac| crate::api::visible_tail(p, ac))
}

// ---------------------------------------------------------------- Flight Intelligence
#[derive(Deserialize)]
struct FlightQ {
    tail: Option<String>,
    kind: Option<String>,
}

async fn flight_events(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Query(q): Query<FlightQ>,
) -> ApiResult {
    p.require(&[ENGINEER, MCC, OPS, SAFETY])?;
    let snap = s.engine.read().snap.clone();
    let v: Vec<_> = snap
        .flight_events
        .iter()
        .filter(|e| q.tail.as_ref().is_none_or(|t| &e.tail == t))
        .filter(|e| {
            q.kind.as_ref().is_none_or(|k| {
                serde_json::to_value(e.kind)
                    .ok()
                    .and_then(|v| v.as_str().map(String::from))
                    .as_deref()
                    == Some(k)
            })
        })
        .filter(|e| tail_ok(&s, &p, &e.tail))
        .collect();
    Ok(Json(json!(v)))
}

async fn flight_one(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Path(fid): Path<String>,
) -> ApiResult {
    p.require(&[ENGINEER, MCC, OPS, SAFETY])?;
    let snap = s.engine.read().snap.clone();
    let v = crate::flight::view(&snap.visible, &snap.flight_events, &fid, snap.clock.sim_now)
        .ok_or_else(|| ApiError::not_found("unknown flight"))?;
    let tail = v["flight"]["tail"].as_str().unwrap_or_default().to_string();
    if !tail_ok(&s, &p, &tail) {
        return Err(ApiError::not_found("unknown flight"));
    }
    Ok(Json(v))
}

// ---------------------------------------------------------------- WRK-03 acknowledgement
async fn alert_ack(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
) -> ApiResult {
    p.require(&[ENGINEER, MCC, SAFETY])?;
    if !s.engine.read().snap.alerts.iter().any(|a| a.id == id) {
        return Err(ApiError::not_found("unknown alert"));
    }
    let ack = AlertAck {
        by: p.audit_actor(),
        at: Utc::now(),
    };
    let conflict = || ApiError::new(StatusCode::CONFLICT, "alert already acknowledged");
    match &s.store {
        // Durable and atomic: exactly one of several concurrent acknowledgements wins.
        Some(st) => {
            if !st
                .save_doc_once("alert_ack", &id, &ack)
                .await
                .map_err(internal)?
            {
                return Err(conflict());
            }
            s.engine
                .write()
                .decisions
                .alert_acks
                .insert(id.clone(), ack.clone());
        }
        None => {
            let mut e = s.engine.write();
            if e.decisions.alert_acks.contains_key(&id) {
                return Err(conflict());
            }
            e.decisions.alert_acks.insert(id.clone(), ack.clone());
        }
    }
    s.audit(
        &p.subject,
        "alert.acknowledged",
        &id,
        json!({ "by": ack.by, "meaning": "seen and owned; not a disposition" }),
    );
    s.recompute_wait().await;
    Ok(Json(json!({ "ok": true, "acknowledged": ack })))
}

// ---------------------------------------------------------------- PSP-02 corrections
const CORRECTABLE: &[(&str, &[&str])] = &[
    ("fault_event", &["message", "code", "ata", "severity"]),
    ("work_order", &["signed_off", "note"]),
];

#[derive(Deserialize)]
struct CorrectionBody {
    record_type: String,
    record_id: String,
    field: String,
    new_value: String,
    reason: String,
}

fn current_value(ds: &Dataset, rtype: &str, id: &str, field: &str) -> Option<String> {
    match rtype {
        "fault_event" => ds.events.iter().find(|e| e.id == id).map(|e| match field {
            "message" => e.message.clone(),
            "code" => e.code.clone(),
            "ata" => e.ata.to_string(),
            _ => serde_json::to_value(e.severity)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default(),
        }),
        "work_order" => ds
            .work_orders
            .iter()
            .find(|w| w.id == id)
            .map(|w| match field {
                "signed_off" => w.signed_off.to_string(),
                _ => w.note.clone(),
            }),
        _ => None,
    }
}

fn validate(field: &str, v: &str) -> Result<(), ApiError> {
    let ok = match field {
        "ata" => v.parse::<u8>().is_ok_and(|n| (5..=99).contains(&n)),
        "severity" => serde_json::from_value::<Severity>(Value::String(v.into())).is_ok(),
        "signed_off" => v == "true" || v == "false",
        _ => !v.trim().is_empty() && v.len() <= 500,
    };
    if ok {
        Ok(())
    } else {
        Err(ApiError::bad_request(format!("invalid value for {field}")))
    }
}

async fn correction(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Json(b): Json<CorrectionBody>,
) -> ApiResult {
    // Corrections never overwrite the original: each is a new, attributed version (PSP-02).
    p.require(&[ENGINEER, MCC])?;
    let fields = CORRECTABLE
        .iter()
        .find(|(t, _)| *t == b.record_type)
        .map(|(_, f)| *f)
        .ok_or_else(|| ApiError::bad_request("record_type must be fault_event or work_order"))?;
    if !fields.contains(&b.field.as_str()) {
        return Err(ApiError::bad_request(format!(
            "field must be one of {}",
            fields.join(", ")
        )));
    }
    if b.reason.trim().len() < 5 {
        return Err(ApiError::bad_request(
            "a reason of at least 5 characters is required",
        ));
    }
    validate(&b.field, &b.new_value)?;
    // Serialise corrections so version numbers and old values are computed against the latest
    // durable state (regression: bug B8, concurrent corrections collided on the same version and
    // took old_value from a stale snapshot).
    static CORRECTION_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = CORRECTION_LOCK.lock().await;
    let (old, version) = {
        let e = s.engine.read();
        // Original record value, then the newest correction of this field if any.
        let original = current_value(&e.full, &b.record_type, &b.record_id, &b.field)
            .ok_or_else(|| ApiError::not_found("unknown record"))?;
        let old = e
            .decisions
            .corrections
            .iter()
            .rev()
            .find(|c| {
                c.record_type == b.record_type && c.record_id == b.record_id && c.field == b.field
            })
            .map(|c| c.new_value.clone())
            .unwrap_or(original);
        let version = 1 + e
            .decisions
            .corrections
            .iter()
            .filter(|c| c.record_type == b.record_type && c.record_id == b.record_id)
            .count() as u32;
        (old, version)
    };
    if !tail_ok(&s, &p, &record_tail(&s, &b.record_type, &b.record_id)) {
        return Err(ApiError::not_found("unknown record"));
    }
    if old == b.new_value {
        return Err(ApiError::bad_request("new value equals current value"));
    }
    let c = Correction {
        id: format!("cor-{}-{}-v{}", b.record_type, b.record_id, version),
        version,
        record_type: b.record_type,
        record_id: b.record_id,
        field: b.field,
        old_value: old,
        new_value: b.new_value,
        reason: b.reason,
        by: p.audit_actor(),
        at: Utc::now(),
    };
    if let Some(st) = &s.store {
        if !st
            .save_doc_once("correction", &c.id, &c)
            .await
            .map_err(internal)?
        {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "a concurrent correction took this version; retry",
            ));
        }
    }
    s.engine.write().decisions.corrections.push(c.clone());
    s.audit(&p.subject, "record.corrected", &c.record_id, json!(c));
    drop(_guard); // the version is claimed; do not hold the lock while the analysis pass runs
    s.recompute_wait().await;
    Ok(Json(json!(c)))
}

fn record_tail(s: &Shared, rtype: &str, id: &str) -> String {
    let e = s.engine.read();
    let ds = &e.full;
    match rtype {
        "fault_event" => ds
            .events
            .iter()
            .find(|x| x.id == id)
            .map(|x| x.tail.clone()),
        "work_order" => ds
            .work_orders
            .iter()
            .find(|x| x.id == id)
            .map(|x| x.tail.clone()),
        _ => None,
    }
    .unwrap_or_default()
}

async fn history(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Path((rtype, id)): Path<(String, String)>,
) -> ApiResult {
    p.require(&[ENGINEER, MCC, SAFETY, LESSOR])?;
    if !tail_ok(&s, &p, &record_tail(&s, &rtype, &id)) {
        return Err(ApiError::not_found("unknown record"));
    }
    let e = s.engine.read();
    let original = match rtype.as_str() {
        "fault_event" => e.full.events.iter().find(|x| x.id == id).map(|x| json!(x)),
        "work_order" => e
            .full
            .work_orders
            .iter()
            .find(|x| x.id == id)
            .map(|x| json!(x)),
        _ => None,
    }
    .ok_or_else(|| ApiError::not_found("unknown record"))?;
    let versions: Vec<&Correction> = e
        .decisions
        .corrections
        .iter()
        .filter(|c| c.record_type == rtype && c.record_id == id)
        .collect();
    Ok(Json(
        json!({ "record_type": rtype, "record_id": id, "original": original, "corrections": versions,
                    "current_version": versions.len() + 1 }),
    ))
}

// ---------------------------------------------------------------- PRD-06 reliability
#[derive(Default)]
struct Rel {
    actions: u32,
    repeats: u32,
    open: u32,
    unsigned: u32,
    days_to_close: Vec<f64>,
}

/// A corrective action is "effective" when no fault on the same tail and ATA chapter
/// recurs within 30 days of closure. This is a reliability statistic, not a finding on any person.
pub fn reliability_stats(ds: &Dataset) -> Value {
    let mut by_ata: BTreeMap<u8, Rel> = BTreeMap::new();
    let mut by_team: BTreeMap<String, Rel> = BTreeMap::new();
    let mut ev: HashMap<(&str, u8), Vec<chrono::DateTime<Utc>>> = HashMap::new();
    for e in &ds.events {
        ev.entry((e.tail.as_str(), e.ata)).or_default().push(e.ts);
    }
    for w in &ds.work_orders {
        let repeat = w.closed.is_some_and(|c| {
            ev.get(&(w.tail.as_str(), w.ata))
                .is_some_and(|ts| ts.iter().any(|t| *t > c && *t <= c + Duration::days(30)))
        });
        for r in [
            by_ata.entry(w.ata).or_default(),
            by_team.entry(w.team.clone()).or_default(),
        ] {
            r.actions += 1;
            if repeat {
                r.repeats += 1;
            }
            match w.closed {
                Some(c) => r
                    .days_to_close
                    .push((c - w.opened).num_minutes() as f64 / 1440.0),
                None => r.open += 1,
            }
            if w.closed.is_some() && !w.signed_off {
                r.unsigned += 1;
            }
        }
    }
    let row = |k: Value, r: &Rel| {
        let closed = r.days_to_close.len().max(1) as f64;
        let mut d = r.days_to_close.clone();
        d.sort_by(f64::total_cmp);
        json!({ "key": k, "corrective_actions": r.actions, "repeat_within_30d": r.repeats,
                "effectiveness": if r.actions > r.open { 1.0 - r.repeats as f64 / (r.actions - r.open) as f64 } else { 0.0 },
                "open": r.open, "closed_without_signoff": r.unsigned,
                "median_days_to_close": d.get(d.len() / 2).copied(),
                "mean_days_to_close": r.days_to_close.iter().sum::<f64>() / closed })
    };
    json!({
        "definition": "Effectiveness = share of closed corrective actions with no fault on the same tail and ATA chapter within 30 days of closure.",
        "limitation": "Statistical indicator for reliability programmes. It does not assess individual performance or establish cause.",
        "by_ata": by_ata.iter().map(|(k, r)| row(json!(k), r)).collect::<Vec<_>>(),
        "by_team": by_team.iter().map(|(k, r)| row(json!(k), r)).collect::<Vec<_>>(),
    })
}

async fn reliability(State(s): State<Shared>, Extension(p): Extension<Principal>) -> ApiResult {
    p.require(&[ENGINEER, MCC, SAFETY])?;
    let snap = s.engine.read().snap.clone();
    Ok(Json(reliability_stats(&snap.visible)))
}

// ---------------------------------------------------------------- CMD-05 CSV export
fn csv_field(v: &str) -> String {
    // RFC 4180 quoting plus formula-injection neutralisation for spreadsheet consumers.
    let v = if v.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{v}")
    } else {
        v.to_string()
    };
    if v.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", v.replace('"', "\"\""))
    } else {
        v
    }
}

fn csv(header: &[&str], rows: Vec<Vec<String>>) -> String {
    let mut out = header.join(",");
    out.push_str("\r\n");
    for r in rows {
        out.push_str(&r.iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(","));
        out.push_str("\r\n");
    }
    out
}

fn sv<T: serde::Serialize>(v: &T) -> String {
    match serde_json::to_value(v) {
        Ok(Value::String(s)) => s,
        Ok(v) => v.to_string(),
        Err(_) => String::new(),
    }
}

async fn report_csv(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Path(name): Path<String>,
) -> Result<Response, ApiError> {
    p.require(&[ENGINEER, MCC, SAFETY, OPS])?;
    let snap = s.engine.read().snap.clone();
    let body = match name.as_str() {
        "alerts.csv" => csv(
            &[
                "id",
                "tail",
                "module",
                "system",
                "severity",
                "title",
                "evidence_grade",
                "created_at",
                "acknowledged_by",
                "escalated_at",
            ],
            snap.alerts
                .iter()
                .filter(|a| tail_ok(&s, &p, &a.tail))
                .map(|a| {
                    vec![
                        a.id.clone(),
                        a.tail.clone(),
                        sv(&a.module),
                        sv(&a.system),
                        sv(&a.severity),
                        a.title.clone(),
                        sv(&a.evidence_grade),
                        a.created_at.to_rfc3339(),
                        a.acknowledged
                            .as_ref()
                            .map(|x| x.by.clone())
                            .unwrap_or_default(),
                        a.escalated_at.map(|t| t.to_rfc3339()).unwrap_or_default(),
                    ]
                })
                .collect(),
        ),
        "cases.csv" => csv(
            &[
                "id",
                "tail",
                "title",
                "system",
                "priority",
                "status",
                "team",
                "assignee",
                "opened_at",
                "sla_due",
                "escalation_level",
            ],
            snap.cases
                .iter()
                .filter(|c| tail_ok(&s, &p, &c.tail))
                .map(|c| {
                    vec![
                        c.id.clone(),
                        c.tail.clone(),
                        c.title.clone(),
                        sv(&c.system),
                        sv(&c.priority),
                        sv(&c.status),
                        c.team.clone(),
                        c.assignee.clone().unwrap_or_default(),
                        c.opened_at.to_rfc3339(),
                        c.sla_due.to_rfc3339(),
                        c.escalation_level.to_string(),
                    ]
                })
                .collect(),
        ),
        "preflight.csv" => csv(
            &[
                "flight_id",
                "tail",
                "number",
                "origin",
                "dest",
                "std",
                "overall",
                "warnings",
                "cautions",
                "acknowledged_by",
                "escalated_at",
            ],
            snap.preflight
                .iter()
                .filter(|r| tail_ok(&s, &p, &r.tail))
                .map(|r| {
                    vec![
                        r.flight_id.clone(),
                        r.tail.clone(),
                        r.number.clone(),
                        r.origin.clone(),
                        r.dest.clone(),
                        r.std.to_rfc3339(),
                        sv(&r.overall),
                        r.counts.get("warning").copied().unwrap_or(0).to_string(),
                        r.counts.get("caution").copied().unwrap_or(0).to_string(),
                        r.acknowledged_by.clone().unwrap_or_default(),
                        r.escalated_at.map(|t| t.to_rfc3339()).unwrap_or_default(),
                    ]
                })
                .collect(),
        ),
        _ => {
            return Err(ApiError::not_found(
                "reports: alerts.csv, cases.csv, preflight.csv",
            ))
        }
    };
    s.audit(
        &p.subject,
        "report.exported",
        &name,
        json!({ "bytes": body.len() }),
    );
    Ok((
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"aips-{name}\""),
            ),
        ],
        body,
    )
        .into_response())
}

// ---------------------------------------------------------------- CFM-05 fleet patterns
pub const K_ANON: usize = 3;

/// (type code, pattern kind, systems) -> (contributing aircraft, patterns, engineering-confirmed)
type PatternGroups<'a> = BTreeMap<(String, String, String), (HashSet<&'a str>, usize, usize)>;

/// Aggregated recurrence patterns across aircraft. A pattern is only disclosed when at least
/// `K_ANON` distinct aircraft contribute, and tails / record ids are never returned.
pub fn fleet_patterns_of(ds: &Dataset, hyps: &[Hypothesis]) -> Value {
    let type_of: HashMap<&str, &str> = ds
        .aircraft
        .iter()
        .map(|a| (a.tail.as_str(), a.type_code.as_str()))
        .collect();
    let mut groups: PatternGroups = BTreeMap::new();
    for h in hyps.iter().filter(|h| h.grade != EvidenceGrade::Rejected) {
        let key = (
            type_of
                .get(h.tail.as_str())
                .copied()
                .unwrap_or("?")
                .to_string(),
            sv(&h.kind),
            h.systems.iter().map(sv).collect::<Vec<_>>().join("+"),
        );
        let g = groups.entry(key).or_default();
        g.0.insert(h.tail.as_str());
        g.1 += 1;
        if h.grade == EvidenceGrade::EngineeringConfirmed {
            g.2 += 1;
        }
    }
    let total = groups.len();
    let shown: Vec<Value> = groups
        .into_iter()
        .filter(|(_, g)| g.0.len() >= K_ANON)
        .map(|((t, k, sys), g)| {
            json!({ "type_code": t, "kind": k, "systems": sys, "aircraft": g.0.len(),
                                        "patterns": g.1, "engineering_confirmed": g.2 })
        })
        .collect();
    json!({ "k_anonymity": K_ANON, "suppressed_groups": total - shown.len(), "patterns": shown,
            "note": "Aggregates only. Groups with fewer than k aircraft are suppressed; no tail numbers or record ids are disclosed." })
}

async fn fleet_patterns(State(s): State<Shared>, Extension(p): Extension<Principal>) -> ApiResult {
    p.require(&[ENGINEER, SAFETY, LESSOR])?;
    let snap = s.engine.read().snap.clone();
    Ok(Json(fleet_patterns_of(&snap.visible, &snap.hypotheses)))
}

// ---------------------------------------------------------------- PRV-04 worker escalation
async fn preflight_escalate(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Path(fid): Path<String>,
) -> ApiResult {
    // Called by the Temporal worker when a significant concern is still unreviewed close to departure.
    p.require(&[ADMIN, MCC])?;
    let (exists, needs, already) = {
        let e = s.engine.read();
        let r = e.snap.preflight.iter().find(|r| r.flight_id == fid);
        (
            r.is_some(),
            r.is_some_and(|r| {
                aips_domain::prevent::needs_escalation(r) && r.acknowledged_by.is_none()
            }),
            e.decisions.preflight_escalations.contains_key(&fid),
        )
    };
    if !exists {
        return Err(ApiError::not_found("no report for flight"));
    }
    if already {
        return Ok(Json(
            json!({ "ok": true, "escalated": false, "reason": "already escalated" }),
        ));
    }
    if !needs {
        return Ok(Json(
            json!({ "ok": true, "escalated": false, "reason": "reviewed or no significant concern" }),
        ));
    }
    let at = s.engine.read().snap.clock.sim_now;
    if let Some(st) = &s.store {
        st.save_doc("preflight_escalation", &fid, &at)
            .await
            .map_err(internal)?;
    }
    s.engine
        .write()
        .decisions
        .preflight_escalations
        .insert(fid.clone(), at);
    s.audit(
        &p.subject,
        "preflight.escalation_requested",
        &fid,
        json!({ "at": at, "to": "MCC duty controller" }),
    );
    s.recompute_wait().await;
    Ok(Json(json!({ "ok": true, "escalated": true, "at": at })))
}

// ---------------------------------------------------------------- DAT-03 erasure (two-person rule)
#[derive(Deserialize)]
struct ErasureBody {
    tail: String,
    reason: String,
}

async fn erasure_list(State(s): State<Shared>, Extension(p): Extension<Principal>) -> ApiResult {
    p.require(&[ADMIN])?;
    Ok(Json(json!(s.engine.read().decisions.erasure_requests)))
}

async fn erasure_request(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Json(b): Json<ErasureBody>,
) -> ApiResult {
    p.require(&[ADMIN])?;
    if b.reason.trim().len() < 10 {
        return Err(ApiError::bad_request(
            "a reason of at least 10 characters is required",
        ));
    }
    if !s
        .engine
        .read()
        .full
        .aircraft
        .iter()
        .any(|a| a.tail == b.tail)
    {
        return Err(ApiError::not_found("unknown tail"));
    }
    let r = ErasureRequest {
        id: format!("era-{}", uuid::Uuid::new_v4()),
        tail: b.tail,
        reason: b.reason,
        requested_by: p.subject.clone(),
        requested_at: Utc::now(),
        approved_by: None,
        executed_at: None,
        rows_erased: None,
    };
    if let Some(st) = &s.store {
        st.save_doc("erasure", &r.id, &r).await.map_err(internal)?;
    }
    s.engine.write().decisions.erasure_requests.push(r.clone());
    s.audit(
        &p.subject,
        "erasure.requested",
        &r.id,
        json!({ "tail": r.tail, "reason": r.reason }),
    );
    Ok(Json(json!(r)))
}

async fn erasure_approve(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
) -> ApiResult {
    p.require(&[ADMIN])?;
    let mut r = s
        .engine
        .read()
        .decisions
        .erasure_requests
        .iter()
        .find(|r| r.id == id)
        .cloned()
        .ok_or_else(|| ApiError::not_found("unknown request"))?;
    if r.executed_at.is_some() {
        return Err(ApiError::new(StatusCode::CONFLICT, "already executed"));
    }
    if r.requested_by == p.subject {
        return Err(ApiError::forbidden(
            "a second administrator must approve (two-person rule)",
        ));
    }
    // Claim the execution atomically so two concurrent approvals cannot both execute.
    if let Some(st) = &s.store {
        if !st
            .save_doc_once("erasure_exec", &id, &json!({ "by": p.subject }))
            .await
            .map_err(internal)?
        {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "already executed or in progress",
            ));
        }
    }
    let rows = match &s.store {
        Some(st) => match st.erase_tail(&r.tail).await {
            Ok(n) => n,
            Err(e) => {
                // Release the claim so the approved erasure can be retried (DAT-03).
                if let Err(e2) = st.delete_doc("erasure_exec", &id).await {
                    tracing::error!(error = %e2, id = %id, "erasure claim not released");
                }
                return Err(internal(e));
            }
        },
        None => 0,
    };
    #[cfg(feature = "neo4j")]
    if let Some(neo) = &s.neo {
        // Graph copy too (DAT-03). PostgreSQL is already erased, so a failure here is logged
        // loudly rather than undoing the erasure.
        if let Err(e) = neo.erase_tail(&r.tail).await {
            tracing::error!(error = %e, tail = %r.tail, "Neo4j erasure failed; graph still holds the aircraft");
        }
    }
    r.approved_by = Some(p.subject.clone());
    r.executed_at = Some(Utc::now());
    r.rows_erased = Some(rows);
    if let Some(st) = &s.store {
        st.save_doc("erasure", &r.id, &r).await.map_err(internal)?;
    }
    {
        let mut e = s.engine.write();
        e.erase_tail(&r.tail);
        if let Some(x) = e.decisions.erasure_requests.iter_mut().find(|x| x.id == id) {
            *x = r.clone();
        }
    }
    // The audit chain itself is append-only and retained; it records that erasure happened.
    s.audit(
        &p.subject,
        "erasure.executed",
        &r.id,
        json!({ "tail": r.tail, "rows": rows, "requested_by": r.requested_by }),
    );
    s.recompute_wait().await;
    Ok(Json(json!(r)))
}

// ---------------------------------------------------------------- NFR-10 model registry
fn registry_dir() -> std::path::PathBuf {
    std::env::var("AIPS_MODEL_REGISTRY")
        .unwrap_or_else(|_| "ml/models/registry".into())
        .into()
}

async fn models(State(s): State<Shared>, Extension(p): Extension<Principal>) -> ApiResult {
    p.require(&[ENGINEER, SAFETY, ADMIN])?;
    let (active, drift) = {
        let e = s.engine.read();
        (
            e.rul_model.as_ref().map(|m| m.version.clone()),
            e.snap.drift.clone(),
        )
    };
    let active_path = s
        .engine
        .read()
        .rul_model
        .as_ref()
        .map(|m| m.path.display().to_string());
    let reg: Vec<Value> = crate::predict::registry(&registry_dir())
        .into_iter()
        .map(|r| json!({ "version": r.version, "active": Some(&r.version) == active.as_ref(),
                         "evaluation": r.card.get("evaluation"), "window_cycles": r.card.get("window_cycles"), "approval": r.card.get("approval") }))
        .collect();
    Ok(Json(
        json!({ "active": active, "active_path": active_path, "registry": reg, "drift": drift }),
    ))
}

#[derive(Deserialize)]
struct ActivateBody {
    version: String,
    reason: String,
}

async fn model_activate(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Json(b): Json<ActivateBody>,
) -> ApiResult {
    p.require(&[ADMIN])?;
    if b.reason.trim().len() < 5 {
        return Err(ApiError::bad_request("a reason is required"));
    }
    let entry = crate::predict::registry(&registry_dir())
        .into_iter()
        .find(|r| r.version == b.version)
        .ok_or_else(|| ApiError::not_found("unknown model version"))?;
    // Load + hash-verify off the async runtime; the swap is atomic under the write lock.
    let m = tokio::task::spawn_blocking(move || crate::predict::load(&entry))
        .await
        .map_err(internal)?
        .map_err(|e| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, e))?;
    let label = m.label();
    if let Some(st) = &s.store {
        st.save_doc(
            "model_activation",
            "rul",
            &json!({ "version": b.version, "by": p.subject, "reason": b.reason }),
        )
        .await
        .map_err(internal)?;
    }
    let prev = {
        let mut e = s.engine.write();
        let prev = e.rul_model.as_ref().map(|m| m.version.clone());
        e.rul_model = Some(Arc::new(m));
        prev
    };
    s.audit(
        &p.subject,
        "model.activated",
        &b.version,
        json!({ "from": prev, "to": b.version, "reason": b.reason }),
    );
    s.recompute_wait().await;
    Ok(Json(
        json!({ "ok": true, "active": label, "previous": prev }),
    ))
}
