//! REST + SSE API for AIPS Command and integrations.
use crate::engine::{CaseHuman, GradeDecision};
use crate::graph;
use crate::state::Shared;
use aips_domain::intel::*;
use aips_domain::model::*;
use aips_platform::auth::roles::*;
use aips_platform::{ApiError, Principal};
use axum::extract::{Path, Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use chrono::{Duration, Utc};
use futures::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::convert::Infallible;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

type ApiResult = Result<Json<Value>, ApiError>;
const READ_TECH: [&str; 5] = [ENGINEER, MCC, OPS, SAFETY, LESSOR];

pub fn router(state: Shared) -> Router<Shared> {
    let protected = Router::new()
        .route("/api/me", get(me))
        .route("/api/overview", get(overview))
        .route("/api/fleet", get(fleet))
        .route("/api/aircraft/{tail}", get(aircraft))
        .route("/api/aircraft/{tail}/series", get(series))
        .route("/api/aircraft/{tail}/timeline", get(timeline))
        .route("/api/aircraft/{tail}/passport/export", get(passport_export))
        .route("/api/alerts", get(alerts))
        .route("/api/alerts/{id}/feedback", post(alert_feedback))
        .route("/api/memory/hypotheses", get(hypotheses))
        .route("/api/memory/hypotheses/{id}/grade", post(grade))
        .route("/api/memory/graph", get(memory_graph))
        .route("/api/preflight", get(preflight_list))
        .route("/api/preflight/{flight_id}", get(preflight_one))
        .route(
            "/api/preflight/{flight_id}/acknowledge",
            post(preflight_ack),
        )
        .route("/api/cases", get(cases))
        .route("/api/cases/{id}", get(case_one))
        .route("/api/cases/{id}/assign", post(case_assign))
        .route("/api/cases/{id}/status", post(case_status))
        .route("/api/cases/{id}/escalate", post(case_escalate))
        .route("/api/metrics", get(metrics))
        .route("/api/audit", get(audit))
        .route("/api/audit/verify", get(audit_verify))
        .route("/api/replay", get(replay_state))
        .route("/api/replay/control", post(replay_control))
        .route("/api/replay/regenerate", post(replay_regenerate))
        .route("/api/ingest/events", post(ingest))
        .route("/api/stream", get(stream))
        .merge(crate::api_ext::routes())
        .layer(axum::middleware::from_fn_with_state(
            state.auth.clone(),
            aips_platform::require_auth,
        ));
    Router::new()
        .route("/api/health", get(health))
        .route("/livez", get(|| async { "ok" }))
        .route("/readyz", get(readyz))
        .route("/metrics", get(prometheus))
        .route("/config.json", get(ui_config))
        .merge(protected)
}

pub fn visible_tail(p: &Principal, ac: &Aircraft) -> bool {
    // Lessors see only aircraft they own (PSP-03); operator roles see the whole fleet.
    if p.has(ADMIN) || p.any(&[ENGINEER, MCC, OPS, SAFETY]) {
        return true;
    }
    p.has(LESSOR) && ac.lessor.is_some() && ac.lessor == p.org
}

fn check_tail(s: &Shared, p: &Principal, tail: &str) -> Result<(), ApiError> {
    let snap = s.engine.read().snap.clone();
    let ac = snap
        .visible
        .aircraft
        .iter()
        .find(|a| a.tail == tail)
        .ok_or_else(|| ApiError::not_found("unknown aircraft"))?;
    if visible_tail(p, ac) {
        Ok(())
    } else {
        Err(ApiError::forbidden(
            "aircraft not shared with your organisation",
        ))
    }
}

async fn health(State(s): State<Shared>) -> Json<Value> {
    let e = s.engine.read();
    let snap = &e.snap;
    #[cfg(feature = "neo4j")]
    let neo = s.neo.is_some();
    #[cfg(not(feature = "neo4j"))]
    let neo = false;
    #[cfg(feature = "kafka")]
    let kafka = s.bus.is_some();
    #[cfg(not(feature = "kafka"))]
    let kafka = false;
    Json(json!({
        "status": "ok",
        "service": "aips-core",
        "version": env!("CARGO_PKG_VERSION"),
        "signal": aips_signal_sys::version(),
        "onnxruntime": aips_signal_sys::has_onnxruntime(),
        "rul_model_loaded": e.rul_model.is_some(),
        "cuda": aips_signal_sys::has_cuda(),
        "postgres": s.store.is_some(),
        "neo4j": neo,
        "kafka": kafka,
        "workflow": e.workflow,
        "auth_mode": s.auth.mode_name(),
        "sim_now": snap.clock.sim_now,
        "live": snap.clock.is_live(),
        "recompute_ms": snap.recompute_ms,
        "snapshot_version": snap.version,
    }))
}

async fn readyz(State(s): State<Shared>) -> (axum::http::StatusCode, &'static str) {
    if s.engine.read().snap.version > 0 {
        (axum::http::StatusCode::OK, "ready")
    } else {
        (axum::http::StatusCode::SERVICE_UNAVAILABLE, "warming up")
    }
}

/// Prometheus text exposition (scraped by Prometheus; dashboards in Grafana).
async fn prometheus(State(s): State<Shared>) -> String {
    let e = s.engine.read();
    let snap = &e.snap;
    let open_cases = snap
        .cases
        .iter()
        .filter(|c| c.status != CaseStatus::Closed)
        .count();
    let escalated = snap
        .cases
        .iter()
        .filter(|c| c.status == CaseStatus::Escalated)
        .count();
    let mut out = String::new();
    let mut g = |name: &str, help: &str, v: f64| {
        out.push_str(&format!(
            "# HELP {name} {help}\n# TYPE {name} gauge\n{name} {v}\n"
        ));
    };
    g(
        "aips_ingest_rejected_total",
        "Ingest messages rejected (malformed, wrong schema, unknown tail)",
        crate::metrics::INGEST_REJECTED.load(std::sync::atomic::Ordering::Relaxed) as f64,
    );
    g(
        "aips_ingest_duplicates_total",
        "Duplicate ingest deliveries ignored (idempotent by event id)",
        crate::metrics::INGEST_DUPLICATES.load(std::sync::atomic::Ordering::Relaxed) as f64,
    );
    g(
        "aips_recompute_milliseconds",
        "Duration of the last full analysis pass",
        snap.recompute_ms,
    );
    g(
        "aips_snapshot_version",
        "Analysis snapshots produced since start",
        snap.version as f64,
    );
    g("aips_alerts", "Current alerts", snap.alerts.len() as f64);
    g(
        "aips_hypotheses",
        "Current Cross-Flight Memory hypotheses",
        snap.hypotheses.len() as f64,
    );
    g(
        "aips_cases_open",
        "Open investigation cases",
        open_cases as f64,
    );
    g(
        "aips_cases_escalated",
        "Escalated investigation cases",
        escalated as f64,
    );
    g(
        "aips_detection_recall",
        "Recall against replay ground truth",
        snap.metrics.recall,
    );
    g(
        "aips_detection_precision",
        "Hypothesis precision against replay ground truth",
        snap.metrics.hypothesis_precision,
    );
    g(
        "aips_events_visible",
        "Fault events visible at the simulation clock",
        snap.visible.events.len() as f64,
    );
    g(
        "aips_audit_entries",
        "Audit chain length",
        e.audit.entries.len() as f64,
    );
    g(
        "aips_sse_subscribers",
        "Connected live-update streams",
        s.tx.receiver_count() as f64,
    );
    out
}

async fn ui_config(State(s): State<Shared>) -> Json<Value> {
    Json(s.ui_config.clone())
}

async fn me(Extension(p): Extension<Principal>) -> Json<Value> {
    Json(json!(p))
}

async fn overview(State(s): State<Shared>, Extension(p): Extension<Principal>) -> ApiResult {
    p.require(&READ_TECH)?;
    let snap = s.engine.read().snap.clone();
    let views: Vec<_> = snap
        .aircraft
        .iter()
        .filter(|v| {
            snap.visible
                .aircraft
                .iter()
                .any(|a| a.tail == v.tail && visible_tail(&p, a))
        })
        .collect();
    let count = |st: &str| views.iter().filter(|v| v.status == st).count();
    let tails: std::collections::HashSet<&str> = views.iter().map(|v| v.tail.as_str()).collect();
    let alerts: Vec<&Alert> = snap
        .alerts
        .iter()
        .filter(|a| tails.contains(a.tail.as_str()) && a.evidence_grade != EvidenceGrade::Rejected)
        .collect();
    let open_cases = snap
        .cases
        .iter()
        .filter(|c| tails.contains(c.tail.as_str()) && c.status != CaseStatus::Closed)
        .count();
    let escalated = snap
        .cases
        .iter()
        .filter(|c| tails.contains(c.tail.as_str()) && c.status == CaseStatus::Escalated)
        .count();
    let review = snap
        .preflight
        .iter()
        .filter(|r| {
            tails.contains(r.tail.as_str())
                && r.overall != aips_domain::prevent::OverallState::NoFindingsInAvailableData
        })
        .count();
    let upcoming: Vec<_> = snap.preflight.iter().filter(|r| tails.contains(r.tail.as_str())).take(8).map(|r| json!({
        "flight_id": r.flight_id, "tail": r.tail, "number": r.number, "origin": r.origin, "dest": r.dest,
        "std": r.std, "overall": r.overall, "counts": r.counts, "acknowledged_by": r.acknowledged_by,
    })).collect();
    let by_system: std::collections::BTreeMap<&str, usize> = System::ALL
        .iter()
        .map(|sys| {
            (
                sys.label(),
                alerts.iter().filter(|a| a.system == *sys).count(),
            )
        })
        .collect();
    Ok(Json(json!({
        "clock": snap.clock,
        "live": snap.clock.is_live(),
        "fleet": { "total": views.len(), "nominal": count("nominal"), "review": count("review"),
                   "attention": count("attention"), "insufficient_data": count("insufficient_data") },
        "alerts": { "total": alerts.len(),
                    "warning": alerts.iter().filter(|a| a.severity == Severity::Warning).count(),
                    "caution": alerts.iter().filter(|a| a.severity == Severity::Caution).count(),
                    "advisory": alerts.iter().filter(|a| a.severity <= Severity::Advisory).count(),
                    "by_system": by_system },
        "cases": { "open": open_cases, "escalated": escalated },
        "preflight": { "needing_review": review, "upcoming": upcoming },
        "top_alerts": alerts.iter().take(8).collect::<Vec<_>>(),
        "recompute_ms": snap.recompute_ms,
        "metrics": { "recall": snap.metrics.recall, "precision": snap.metrics.hypothesis_precision },
    })))
}

async fn fleet(State(s): State<Shared>, Extension(p): Extension<Principal>) -> ApiResult {
    p.require(&READ_TECH)?;
    let snap = s.engine.read().snap.clone();
    let v: Vec<_> = snap
        .aircraft
        .iter()
        .filter(|v| {
            snap.visible
                .aircraft
                .iter()
                .any(|a| a.tail == v.tail && visible_tail(&p, a))
        })
        .collect();
    Ok(Json(json!(v)))
}

async fn aircraft(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Path(tail): Path<String>,
) -> ApiResult {
    p.require(&READ_TECH)?;
    check_tail(&s, &p, &tail)?;
    let snap = s.engine.read().snap.clone();
    let ds = &snap.visible;
    let view = snap.aircraft.iter().find(|v| v.tail == tail);
    let ac = ds.aircraft.iter().find(|a| a.tail == tail);
    let params: Vec<_> = Param::ALL
        .iter()
        .map(|p| {
            let m = p.meta();
            let vals = snap.series.values(&tail, *p);
            json!({ "param": p, "label": m.label, "unit": m.unit, "lo": m.lo, "hi": m.hi, "system": p.system(),
                    "last": vals.last(), "spark": vals.iter().rev().take(40).rev().collect::<Vec<_>>() })
        })
        .collect();
    let systems: Vec<_> = System::ALL
        .iter()
        .map(|sys| {
            let al: Vec<&Alert> = snap
                .alerts
                .iter()
                .filter(|a| {
                    a.tail == tail
                        && a.system == *sys
                        && a.evidence_grade != EvidenceGrade::Rejected
                })
                .collect();
            let worst = al.iter().map(|a| a.severity).max();
            json!({ "system": sys, "label": sys.label(), "alerts": al.len(), "worst": worst })
        })
        .collect();
    Ok(Json(json!({
        "aircraft": ac, "view": view, "systems": systems, "params": params,
        "components": ds.components.iter().filter(|c| c.tail == tail).collect::<Vec<_>>(),
        "rul": snap.rul.iter().filter(|r| r.tail == tail).collect::<Vec<_>>(),
        "alerts": snap.alerts.iter().filter(|a| a.tail == tail).collect::<Vec<_>>(),
        "hypotheses": snap.hypotheses.iter().filter(|h| h.tail == tail).collect::<Vec<_>>(),
        "cases": snap.cases.iter().filter(|c| c.tail == tail).collect::<Vec<_>>(),
        "mel": ds.mel.iter().filter(|m| m.tail == tail).collect::<Vec<_>>(),
        "preflight": snap.preflight.iter().find(|r| r.tail == tail),
    })))
}

#[derive(Deserialize)]
struct SeriesQ {
    param: Param,
}

async fn series(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Path(tail): Path<String>,
    Query(q): Query<SeriesQ>,
) -> ApiResult {
    p.require(&[ENGINEER, MCC, SAFETY, OPS])?;
    check_tail(&s, &p, &tail)?;
    let snap = s.engine.read().snap.clone();
    let pts = snap
        .series
        .series
        .get(&(tail.clone(), q.param))
        .cloned()
        .unwrap_or_default();
    let vals: Vec<f64> = pts.iter().map(|x| x.1).collect();
    let feats = aips_signal_sys::extract(&vals, &crate::integrity::config_for(q.param)).ok();
    let m = q.param.meta();
    Ok(Json(json!({
        "param": q.param, "label": m.label, "unit": m.unit, "lo": m.lo, "hi": m.hi,
        "points": pts.iter().map(|(t, v, f)| json!({ "ts": t, "value": v, "flight_id": f })).collect::<Vec<_>>(),
        "features": feats.map(|f| json!({ "baseline_mean": f.baseline_mean, "baseline_stddev": f.baseline_stddev,
            "ewma_z": f.ewma_z, "cusum_pos": f.cusum_pos, "cusum_neg": f.cusum_neg, "cusum_alarm": f.cusum_alarm,
            "cusum_alarm_index": f.cusum_alarm_index, "frozen": f.frozen == 1, "slope_per_sample": f.slope })),
        "model_version": crate::integrity::model_version(),
    })))
}

async fn timeline(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Path(tail): Path<String>,
) -> ApiResult {
    p.require(&READ_TECH)?;
    check_tail(&s, &p, &tail)?;
    let snap = s.engine.read().snap.clone();
    let ds = &snap.visible;
    let since = snap.clock.sim_now - Duration::days(45);
    let lessor_only = !p.any(&[ENGINEER, MCC, OPS, SAFETY]);
    let mut items: Vec<Value> = Vec::new();
    if !lessor_only {
        let mut fl: Vec<&Flight> = ds.flights.iter().filter(|f| f.tail == tail).collect();
        fl.sort_by_key(|f| std::cmp::Reverse(f.arr));
        for f in fl.into_iter().take(12) {
            items.push(json!({ "kind": "flight", "ts": f.arr, "title": format!("{} {} → {}", f.number, f.origin, f.dest),
                               "detail": format!("Cycle {}", f.cycle), "ref": f.id }));
        }
    }
    for e in ds
        .events
        .iter()
        .filter(|e| e.tail == tail && e.ts >= since && e.severity >= Severity::Advisory)
    {
        items.push(json!({ "kind": "fault", "ts": e.ts, "title": format!("{} {}", e.code, e.message), "severity": e.severity,
                           "detail": format!("{} · {}", e.system.label(), e.source.label()), "ref": e.id }));
    }
    for w in ds
        .work_orders
        .iter()
        .filter(|w| w.tail == tail && w.opened >= since)
    {
        items.push(json!({ "kind": "work_order", "ts": w.closed.unwrap_or(w.opened),
                           "title": format!("{:?}: {}", w.action, w.note), "detail": format!("{} · {}", w.team, if w.signed_off { "signed off" } else { "no sign-off recorded" }),
                           "ref": w.id }));
    }
    for h in snap.hypotheses.iter().filter(|h| h.tail == tail) {
        items.push(json!({ "kind": "hypothesis", "ts": h.explanation.last_seen, "title": h.title, "grade": h.grade,
                           "detail": h.explanation.narrative, "ref": h.id }));
    }
    for c in snap.cases.iter().filter(|c| c.tail == tail) {
        items.push(json!({ "kind": "case", "ts": c.opened_at, "title": format!("{} opened: {}", c.id, c.title),
                           "detail": format!("{:?} · {}", c.status, c.team), "ref": c.id }));
    }
    for m in ds.mel.iter().filter(|m| m.tail == tail) {
        items.push(json!({ "kind": "mel", "ts": m.opened, "title": format!("MEL {} cat {}: {}", m.reference, m.category, m.title),
                           "detail": format!("Due {}", m.due.format("%Y-%m-%d %H:%M UTC")), "ref": m.id }));
    }
    items.sort_by(|a, b| b["ts"].as_str().cmp(&a["ts"].as_str()));
    Ok(Json(json!(items)))
}

async fn passport_export(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Path(tail): Path<String>,
) -> ApiResult {
    p.require(&READ_TECH)?;
    check_tail(&s, &p, &tail)?;
    let snap = s.engine.read().snap.clone();
    let ds = &snap.visible;
    let lessor_only = !p.any(&[ENGINEER, MCC, OPS, SAFETY]);
    s.audit(
        &p.subject,
        "passport.exported",
        &tail,
        json!({ "redacted_operations": lessor_only }),
    );
    Ok(Json(json!({
        "schema": "aips.passport/1.0",
        "exported_at": Utc::now(),
        "exported_by": p.subject,
        "tail": tail,
        "aircraft": ds.aircraft.iter().find(|a| a.tail == tail),
        "components": ds.components.iter().filter(|c| c.tail == tail).collect::<Vec<_>>(),
        "flights": if lessor_only { json!("redacted for this organisation (PSP-03)") } else {
            json!(ds.flights.iter().filter(|f| f.tail == tail).count()) },
        "fault_events": ds.events.iter().filter(|e| e.tail == tail).collect::<Vec<_>>(),
        "work_orders": ds.work_orders.iter().filter(|w| w.tail == tail).collect::<Vec<_>>(),
        "mel": ds.mel.iter().filter(|m| m.tail == tail).collect::<Vec<_>>(),
        "hypotheses": snap.hypotheses.iter().filter(|h| h.tail == tail).collect::<Vec<_>>(),
        "cases": snap.cases.iter().filter(|c| c.tail == tail).collect::<Vec<_>>(),
        "rul_estimates": snap.rul.iter().filter(|r| r.tail == tail).collect::<Vec<_>>(),
        "data_quality": snap.preflight.iter().find(|r| r.tail == tail).map(|r| &r.freshness),
    })))
}

#[derive(Deserialize)]
struct AlertQ {
    tail: Option<String>,
    module: Option<Module>,
}

async fn alerts(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Query(q): Query<AlertQ>,
) -> Result<axum::response::Response, ApiError> {
    p.require(&[ENGINEER, MCC, OPS, SAFETY])?;
    let snap = s.engine.read().snap.clone();
    let key = format!("alerts|{:?}|{:?}", q.tail, q.module);
    Ok(s.cached_json(key, snap.version, || {
        let v: Vec<&Alert> = snap
            .alerts
            .iter()
            .filter(|a| {
                q.tail.as_ref().is_none_or(|t| &a.tail == t)
                    && q.module.is_none_or(|m| a.module == m)
            })
            .collect();
        json!(v)
    }))
}

#[derive(Deserialize)]
struct FeedbackBody {
    useful: bool,
    #[serde(default)]
    comment: String,
}

async fn alert_feedback(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
    Json(b): Json<FeedbackBody>,
) -> ApiResult {
    p.require(&[ENGINEER, MCC, SAFETY])?;
    if !s.engine.read().snap.alerts.iter().any(|a| a.id == id) {
        return Err(ApiError::not_found("unknown alert"));
    }
    let fb = AlertFeedback {
        useful: b.useful,
        comment: b.comment,
        by: p.audit_actor(),
        at: Utc::now(),
    };
    // Persist first: a decision is applied only once it is durable (regression: bug B4).
    if let Some(st) = &s.store {
        st.save_feedback(&id, &fb).await.map_err(|e| {
            tracing::error!(error = %e, "decision not stored");
            ApiError::new(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "decision not stored",
            )
        })?;
    }
    s.engine
        .write()
        .decisions
        .feedback
        .insert(id.clone(), fb.clone());
    s.audit(
        &p.subject,
        "alert.feedback",
        &id,
        json!({ "useful": fb.useful, "comment": fb.comment }),
    );
    s.recompute_wait().await;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct HypQ {
    tail: Option<String>,
}

async fn hypotheses(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Query(q): Query<HypQ>,
) -> Result<axum::response::Response, ApiError> {
    p.require(&READ_TECH)?;
    let snap = s.engine.read().snap.clone();
    // Cache key: the principal's visibility scope (all aircraft, or one lessor org) and the filter.
    let scope = if p.has(ADMIN) || p.any(&[ENGINEER, MCC, OPS, SAFETY]) {
        "all".to_string()
    } else {
        format!("org:{:?}", p.org)
    };
    let key = format!("hyp|{scope}|{:?}", q.tail);
    Ok(s.cached_json(key, snap.version, || {
    let allowed: std::collections::HashSet<&str> = snap
        .visible
        .aircraft
        .iter()
        .filter(|a| visible_tail(&p, a))
        .map(|a| a.tail.as_str())
        .collect();
    let v: Vec<&Hypothesis> = snap
        .hypotheses
        .iter()
        .filter(|h| {
            allowed.contains(h.tail.as_str()) && q.tail.as_ref().is_none_or(|t| &h.tail == t)
        })
        .collect();
    let events: std::collections::HashMap<&str, &FaultEvent> = snap
        .visible
        .events
        .iter()
        .map(|e| (e.id.as_str(), e))
        .collect();
    let wos: std::collections::HashMap<&str, &WorkOrder> =
        snap.visible.work_orders.iter().map(|w| (w.id.as_str(), w)).collect();
    let enriched: Vec<Value> = v
        .into_iter()
        .map(|h| {
            json!({ "hypothesis": h, "events": h.event_ids.iter().filter_map(|id| events.get(id.as_str())).collect::<Vec<_>>(),
                    "work_orders": h.work_order_ids.iter().filter_map(|id| wos.get(id.as_str())).collect::<Vec<_>>() })
        })
        .collect();
    json!(enriched)
    }))
}

#[derive(Deserialize)]
struct GradeBody {
    grade: EvidenceGrade,
    #[serde(default)]
    note: String,
}

async fn grade(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
    Json(b): Json<GradeBody>,
) -> ApiResult {
    // CFM-03: only authorised engineers may change an evidence grade.
    p.require(&[ENGINEER])?;
    let current = s
        .engine
        .read()
        .snap
        .hypotheses
        .iter()
        .find(|h| h.id == id)
        .map(|h| h.grade);
    let Some(from) = current else {
        return Err(ApiError::not_found("unknown hypothesis"));
    };
    if b.grade == EvidenceGrade::EngineeringConfirmed && b.note.trim().len() < 10 {
        return Err(ApiError::bad_request(
            "an engineering confirmation needs a justification of at least 10 characters",
        ));
    }
    let d = GradeDecision {
        grade: b.grade,
        by: p.audit_actor(),
        at: Utc::now(),
        note: b.note.clone(),
    };
    // Persist first: a decision is applied only once it is durable (regression: bug B4).
    if let Some(st) = &s.store {
        st.save_grade(&id, &d).await.map_err(|e| {
            tracing::error!(error = %e, "decision not stored");
            ApiError::new(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "decision not stored",
            )
        })?;
    }
    s.engine
        .write()
        .decisions
        .grades
        .insert(id.clone(), d.clone());
    s.audit(
        &p.subject,
        "hypothesis.graded",
        &id,
        json!({ "from": from, "to": b.grade, "note": b.note }),
    );
    s.recompute_wait().await;
    Ok(Json(json!({ "ok": true, "grade": b.grade })))
}

#[derive(Deserialize)]
struct GraphQ {
    tail: String,
    source: Option<String>,
}

async fn memory_graph(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Query(q): Query<GraphQ>,
) -> ApiResult {
    p.require(&READ_TECH)?;
    check_tail(&s, &p, &q.tail)?;
    #[cfg(feature = "neo4j")]
    if q.source.as_deref() == Some("neo4j") {
        if let Some(neo) = &s.neo {
            match neo.view(&q.tail).await {
                Ok(v) => return Ok(Json(json!(v))),
                Err(e) => tracing::warn!(error = %e, "neo4j view failed; serving in-process graph"),
            }
        }
    }
    let _ = &q.source;
    let snap = s.engine.read().snap.clone();
    Ok(Json(json!(graph::local_view(&snap, &q.tail))))
}

async fn preflight_list(State(s): State<Shared>, Extension(p): Extension<Principal>) -> ApiResult {
    p.require(&[ENGINEER, MCC, OPS, SAFETY])?;
    let snap = s.engine.read().snap.clone();
    let v: Vec<Value> = snap
        .preflight
        .iter()
        .map(|r| json!({ "id": r.id, "flight_id": r.flight_id, "tail": r.tail, "type_code": r.type_code, "number": r.number,
                         "origin": r.origin, "dest": r.dest, "std": r.std, "overall": r.overall, "counts": r.counts,
                         "acknowledged_by": r.acknowledged_by, "generation_ms": r.generation_ms,
                         "top": r.ranked.first().map(|f| &f.title),
                         "categories": r.categories.iter().map(|c| json!({ "category": c.category, "label": c.label, "status": c.status })).collect::<Vec<_>>() }))
        .collect();
    Ok(Json(json!(v)))
}

async fn preflight_one(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Path(fid): Path<String>,
) -> ApiResult {
    p.require(&[ENGINEER, MCC, OPS, SAFETY])?;
    let snap = s.engine.read().snap.clone();
    let r = snap
        .preflight
        .iter()
        .find(|r| r.flight_id == fid)
        .ok_or_else(|| ApiError::not_found("no report for flight"))?;
    Ok(Json(json!(r)))
}

async fn preflight_ack(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Path(fid): Path<String>,
) -> ApiResult {
    // Acknowledging records that a person reviewed the findings. It is not a dispatch approval (PRV-05).
    p.require(&[OPS, ENGINEER, MCC])?;
    if !s
        .engine
        .read()
        .snap
        .preflight
        .iter()
        .any(|r| r.flight_id == fid)
    {
        return Err(ApiError::not_found("no report for flight"));
    }
    let at = Utc::now();
    // Persist first: a decision is applied only once it is durable (regression: bug B4).
    if let Some(st) = &s.store {
        st.save_ack(&fid, &p.audit_actor(), at).await.map_err(|e| {
            tracing::error!(error = %e, "decision not stored");
            ApiError::new(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "decision not stored",
            )
        })?;
    }
    s.engine
        .write()
        .decisions
        .preflight_acks
        .insert(fid.clone(), (p.audit_actor(), at));
    s.audit(
        &p.subject,
        "preflight.acknowledged",
        &fid,
        json!({ "meaning": "findings reviewed; not a dispatch decision" }),
    );
    s.recompute_wait().await;
    Ok(Json(json!({ "ok": true })))
}

async fn cases(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
) -> Result<axum::response::Response, ApiError> {
    p.require(&[ENGINEER, MCC, SAFETY, OPS])?;
    let snap = s.engine.read().snap.clone();
    Ok(s.cached_json("cases".into(), snap.version, || json!(snap.cases)))
}

async fn case_one(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
) -> ApiResult {
    p.require(&[ENGINEER, MCC, SAFETY, OPS])?;
    let snap = s.engine.read().snap.clone();
    let c = snap
        .cases
        .iter()
        .find(|c| c.id == id)
        .ok_or_else(|| ApiError::not_found("unknown case"))?;
    let hyps: Vec<&Hypothesis> = snap
        .hypotheses
        .iter()
        .filter(|h| c.hypothesis_ids.contains(&h.id))
        .collect();
    let evs: Vec<&FaultEvent> = snap
        .visible
        .events
        .iter()
        .filter(|e| c.event_ids.contains(&e.id))
        .collect();
    Ok(Json(
        json!({ "case": c, "hypotheses": hyps, "events": evs }),
    ))
}

async fn update_case(
    s: &Shared,
    p: &Principal,
    id: &str,
    f: impl FnOnce(&mut CaseHuman) -> CaseAction,
) -> ApiResult {
    if !s.engine.read().snap.cases.iter().any(|c| c.id == id) {
        return Err(ApiError::not_found("unknown case"));
    }
    // Serialise case decisions: two concurrent updates must not start from the same copy and
    // drop each other's change and history entry (lost update, same pattern as bug B8).
    static CASE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = CASE_LOCK.lock().await;
    // Work on a copy, persist it, then publish it (regression: bug B4).
    let (mut human, action, base_len) = {
        let e = s.engine.read();
        let mut h = e.decisions.cases.get(id).cloned().unwrap_or_default();
        let base_len = h.history.len();
        let action = f(&mut h);
        h.history.push(action.clone());
        (h, action, base_len)
    };
    if let Some(st) = &s.store {
        st.save_case(id, &human).await.map_err(|e| {
            tracing::error!(error = %e, case = id, "case decision not stored");
            ApiError::new(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "case decision not stored",
            )
        })?;
    }
    {
        let mut e = s.engine.write();
        let cur = e.decisions.cases.entry(id.to_string()).or_default();
        // Keep an escalation the engine committed while this decision was being saved.
        if cur.history.len() > base_len {
            let mine = human.history.pop();
            human
                .history
                .extend(cur.history[base_len..].iter().cloned());
            human.history.extend(mine);
            human.escalation_level = human.escalation_level.max(cur.escalation_level);
        }
        *cur = human;
    }
    s.audit(
        &p.subject,
        &format!("case.{}", action.action),
        id,
        json!({ "note": action.note }),
    );
    s.recompute_wait().await;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct AssignBody {
    assignee: String,
    team: Option<String>,
}

async fn case_assign(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
    Json(b): Json<AssignBody>,
) -> ApiResult {
    p.require(&[MCC, ENGINEER])?;
    let actor = p.audit_actor();
    update_case(&s, &p, &id, |h| {
        h.assignee = Some(b.assignee.clone());
        if let Some(t) = &b.team {
            h.team = Some(t.clone());
        }
        if h.status.is_none() || h.status == Some(CaseStatus::Open) {
            h.status = Some(CaseStatus::Assigned);
        }
        CaseAction {
            at: Utc::now(),
            actor,
            action: "assigned".into(),
            note: format!("Assigned to {}", b.assignee),
        }
    })
    .await
}

#[derive(Deserialize)]
struct StatusBody {
    status: CaseStatus,
    note: String,
}

async fn case_status(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
    Json(b): Json<StatusBody>,
) -> ApiResult {
    p.require(&[ENGINEER, SAFETY, MCC])?;
    if b.status == CaseStatus::Closed && b.note.trim().len() < 10 {
        return Err(ApiError::bad_request(
            "closing a case requires a corrective-action note (≥ 10 characters)",
        ));
    }
    let actor = p.audit_actor();
    update_case(&s, &p, &id, |h| {
        h.status = Some(b.status);
        CaseAction {
            at: Utc::now(),
            actor,
            action: format!("{:?}", b.status).to_lowercase(),
            note: b.note.clone(),
        }
    })
    .await
}

#[derive(Deserialize)]
struct EscalateBody {
    reason: String,
    #[serde(default)]
    level: Option<u8>,
}

/// Called by the Temporal workflow worker when an SLA timer fires (WRK-05).
async fn case_escalate(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
    Json(b): Json<EscalateBody>,
) -> ApiResult {
    p.require(&[MCC, ADMIN])?;
    let actor = p.audit_actor();
    update_case(&s, &p, &id, |h| {
        h.escalation_level = b.level.unwrap_or(h.escalation_level + 1);
        if h.status != Some(CaseStatus::Closed) {
            h.status = Some(CaseStatus::Escalated);
        }
        CaseAction {
            at: Utc::now(),
            actor,
            action: "escalated".into(),
            note: b.reason.clone(),
        }
    })
    .await
}

async fn metrics(State(s): State<Shared>, Extension(p): Extension<Principal>) -> ApiResult {
    p.require(&[ENGINEER, MCC, SAFETY])?;
    let e = s.engine.read();
    let snap = e.snap.clone();
    Ok(Json(json!({
        "metrics": snap.metrics,
        "drift": snap.drift,
        "stages_ms": snap.stages,
        "recompute_ms": snap.recompute_ms,
        "dataset": { "aircraft": snap.visible.aircraft.len(), "flights": snap.visible.flights.len(),
                     "events": snap.visible.events.len(), "work_orders": snap.visible.work_orders.len(),
                     "samples": snap.visible.samples.len(), "seed": snap.visible.seed },
        "models": [
            { "name": "Cross-Flight Memory", "version": aips_domain::memory::MODEL_VERSION, "kind": "rule + statistics (lift)" },
            { "name": "Parameter analytics", "version": crate::integrity::model_version(), "kind": "C++ signal library" },
            { "name": "Remaining useful life", "version": e.rul_model.as_ref().map(|m| m.label()),
              "kind": "quantile regression (ONNX)", "card": e.rul_model.as_ref().map(|m| m.card.clone()) },
        ],
    })))
}

#[derive(Deserialize)]
struct AuditQ {
    limit: Option<usize>,
}

async fn audit(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Query(q): Query<AuditQ>,
) -> ApiResult {
    p.require(&[SAFETY, ENGINEER, MCC])?;
    let e = s.engine.read();
    let n = q.limit.unwrap_or(200).min(2000);
    let v: Vec<_> = e.audit.entries.iter().rev().take(n).collect();
    Ok(Json(
        json!({ "total": e.audit.entries.len(), "entries": v }),
    ))
}

async fn audit_verify(State(s): State<Shared>, Extension(p): Extension<Principal>) -> ApiResult {
    p.require(&[SAFETY, ENGINEER])?;
    let e = s.engine.read();
    let broken = e.audit.verify();
    Ok(Json(
        json!({ "entries": e.audit.entries.len(), "intact": broken.is_none(), "first_broken_seq": broken,
                    "head": e.audit.entries.last().map(|x| &x.hash) }),
    ))
}

async fn replay_state(State(s): State<Shared>, Extension(p): Extension<Principal>) -> ApiResult {
    p.require(&READ_TECH)?;
    let e = s.engine.read();
    Ok(Json(
        json!({ "clock": e.clock, "live": e.clock.is_live(), "seed": e.full.seed, "scenarios": e.truth.len(),
                    "generated_at": e.full.generated_at }),
    ))
}

#[derive(Deserialize)]
struct ControlBody {
    action: String,
    hours: Option<f64>,
    speed: Option<f64>,
}

async fn replay_control(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Json(b): Json<ControlBody>,
) -> ApiResult {
    p.require(&[ADMIN, ENGINEER, SAFETY])?;
    {
        let mut e = s.engine.write();
        match b.action.as_str() {
            "replay" => {
                let hours = b.hours.unwrap_or(24.0).clamp(1.0, 24.0 * 30.0);
                e.clock.sim_now = e.clock.data_end - Duration::minutes((hours * 60.0) as i64);
                e.clock.speed = b.speed.unwrap_or(120.0).clamp(1.0, 3600.0);
                e.clock.running = true;
                e.go_live = e.clock.sim_now;
                e.case_opened.clear();
                e.case_opened.clear();
            }
            "pause" => e.clock.running = false,
            "resume" => e.clock.running = true,
            "speed" => e.clock.speed = b.speed.unwrap_or(60.0).clamp(1.0, 3600.0),
            "live" => {
                e.clock.sim_now = e.clock.data_end.max(e.clock.sim_now);
                e.clock.running = true;
            }
            _ => {
                return Err(ApiError::bad_request(
                    "action must be replay | pause | resume | speed | live",
                ))
            }
        }
    }
    s.audit(
        &p.subject,
        "replay.control",
        &b.action,
        json!({ "hours": b.hours, "speed": b.speed }),
    );
    if let Some(st) = &s.store {
        let clock = s.engine.read().clock.clone();
        if let Err(e) = st.save_clock(&clock).await {
            tracing::warn!(error = %e, "clock persistence failed");
        }
    }
    s.recompute_wait().await;
    let e = s.engine.read();
    Ok(Json(json!({ "clock": e.clock })))
}

#[derive(Deserialize)]
struct RegenBody {
    seed: u64,
}

async fn replay_regenerate(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Json(b): Json<RegenBody>,
) -> ApiResult {
    p.require(&[ADMIN])?;
    let now = Utc::now();
    let g = aips_replay::generate(b.seed, now);
    if let Some(st) = &s.store {
        st.clear().await.map_err(|e| {
            tracing::error!(error = %e, "replay data not stored");
            ApiError::new(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "replay data not stored",
            )
        })?;
        st.save_dataset(&g.dataset, &g.truth).await.map_err(|e| {
            tracing::error!(error = %e, "replay data not stored");
            ApiError::new(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "replay data not stored",
            )
        })?;
    }
    {
        // Same lock as an analysis pass: a pass computed from the old dataset must not commit
        // after this swap (it would refill case_opened and briefly serve the old snapshot).
        let _serial = s.recompute_lock.lock();
        let mut e = s.engine.write();
        e.full = std::sync::Arc::new(g.dataset);
        e.truth = g.truth;
        e.decisions = Default::default();
        e.clock.data_end = now;
        e.clock.sim_now = now - Duration::hours(3);
        e.clock.speed = 30.0;
        e.clock.running = true;
        e.go_live = e.clock.sim_now;
        e.case_opened.clear();
    }
    s.audit(
        &p.subject,
        "replay.regenerated",
        &b.seed.to_string(),
        json!({}),
    );
    s.recompute_wait().await;
    Ok(Json(json!({ "ok": true, "seed": b.seed })))
}

/// DAT-06: the only accepted ingest contract version. Absent header = v1 (backwards compatible).
pub const INGEST_SCHEMA: &str = "aips.fault.v1";

async fn ingest(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    headers: axum::http::HeaderMap,
    Json(mut ev): Json<FaultEvent>,
) -> ApiResult {
    p.require(&[ADMIN, ENGINEER])?;
    if let Some(v) = headers.get("x-aips-schema") {
        if v.to_str().ok() != Some(INGEST_SCHEMA) {
            return Err(ApiError::new(
                axum::http::StatusCode::UNSUPPORTED_MEDIA_TYPE,
                format!("unsupported schema; expected {INGEST_SCHEMA}"),
            ));
        }
    }
    if ev.id.trim().is_empty() || ev.code.trim().is_empty() || ev.ata == 0 {
        return Err(ApiError::bad_request("id, code and ata are required"));
    }
    if !s
        .engine
        .read()
        .full
        .aircraft
        .iter()
        .any(|a| a.tail == ev.tail)
    {
        return Err(ApiError::bad_request("unknown tail"));
    }
    ev.normalise();
    #[cfg(feature = "kafka")]
    if let Some(bus) = &s.bus {
        // An event the broker did not accept is reported as such, never as ok (the client retries).
        bus.publish_with_headers(
            crate::bus::TOPIC_INGEST,
            &ev.tail,
            &ev,
            &[
                ("x-aips-schema", INGEST_SCHEMA),
                ("x-aips-submitted-by", &p.subject),
            ],
        )
        .await
        .map_err(|_| {
            ApiError::new(
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "event not accepted: message bus unavailable",
            )
        })?;
        return Ok(Json(json!({ "ok": true, "via": "kafka" })));
    }
    // Persist first: an event that cannot be stored is rejected rather than analysed silently.
    if let Some(st) = &s.store {
        st.insert_event(&ev).await.map_err(|e| {
            tracing::error!(error = %e, id = %ev.id, "ingest: event not stored");
            ApiError::new(
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "event not stored: database unavailable",
            )
        })?;
    }
    let new = s.apply_ingest(ev, &format!("api:{}", p.subject));
    Ok(Json(
        json!({ "ok": true, "via": "direct", "duplicate": !new }),
    ))
}

async fn stream(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = s.tx.subscribe();
    let st = BroadcastStream::new(rx).filter_map(move |m| {
        let t = scope_stream_message(&s, &p, m.ok()?)?;
        Some(Ok(Event::default().data(t)))
    });
    Sse::new(st).keep_alive(KeepAlive::default())
}

/// PSP-03 / CMD-04: fleet roles receive stream messages unchanged; everyone else only sees
/// new alerts on aircraft visible to them. Unparseable messages are dropped (fail closed).
fn scope_stream_message(s: &Shared, p: &Principal, text: String) -> Option<String> {
    if p.has(ADMIN) || p.any(&[ENGINEER, MCC, OPS, SAFETY]) {
        return Some(text);
    }
    let mut v: Value = serde_json::from_str(&text).ok()?;
    if let Some(list) = v.get_mut("new_alerts").and_then(Value::as_array_mut) {
        let snap = s.engine.read().snap.clone();
        list.retain(|a| {
            snap.visible
                .aircraft
                .iter()
                .any(|ac| a["tail"].as_str() == Some(ac.tail.as_str()) && visible_tail(p, ac))
        });
    }
    Some(v.to_string())
}
