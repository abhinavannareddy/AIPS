//! AIPS Flight in the core: runs the flight rules over visible flights and builds the
//! per-flight view (track, weather, NOTAMs, correlated faults, post-flight summary).
use aips_domain::flight::{self, FlightEvent, FlightInput, FlightSummary};
use aips_domain::intel::{Alert, EvidenceGrade, Module, SourceRef};
use aips_domain::model::{Dataset, Flight, Severity, System, Ts};
use serde_json::{json, Value};
use std::collections::HashMap;

pub const ALERT_WINDOW_DAYS: i64 = 14;

fn wide(type_code: &str) -> bool {
    matches!(type_code, "B787-9" | "A350-900")
}

pub fn analyse(ds: &Dataset, now: Ts) -> Vec<FlightEvent> {
    let flights: HashMap<&str, &Flight> = ds.flights.iter().map(|f| (f.id.as_str(), f)).collect();
    let types: HashMap<&str, &str> = ds
        .aircraft
        .iter()
        .map(|a| (a.tail.as_str(), a.type_code.as_str()))
        .collect();
    let faults = flight::index_faults(&ds.events);
    let mut out = Vec::new();
    for s in &ds.flight_summaries {
        let Some(f) = flights.get(s.flight_id.as_str()) else {
            continue;
        };
        let input = FlightInput {
            flight_id: &f.id,
            tail: &f.tail,
            number: &f.number,
            origin: &f.origin,
            dest: &f.dest,
            dep: f.dep,
            arr: f.arr,
            wide: wide(types.get(f.tail.as_str()).copied().unwrap_or("")),
        };
        let fl = faults.get(f.id.as_str()).cloned().unwrap_or_default();
        out.extend(flight::analyse(&input, s, &fl, now));
    }
    out.sort_by_key(|e| std::cmp::Reverse(e.at));
    out
}

/// Recent flight events at Advisory or above become alerts (Info-level data-quality events stay
/// in the Flight view so they never raise unsupported safety conclusions).
pub fn alerts(events: &[FlightEvent], now: Ts) -> Vec<Alert> {
    events
        .iter()
        .filter(|e| {
            e.severity >= Severity::Advisory
                && now - e.at <= chrono::Duration::days(ALERT_WINDOW_DAYS)
        })
        .map(|e| Alert {
            id: format!("A-{}", e.id),
            tail: e.tail.clone(),
            module: Module::Flight,
            system: System::Other,
            severity: e.severity,
            title: format!("{} on {}", e.kind.label(), e.number),
            reason: format!(
                "{}: observed {} (threshold {}). {}{}",
                e.kind.label(),
                e.observed,
                e.threshold,
                e.correlation_note
                    .clone()
                    .map(|c| format!("{c} "))
                    .unwrap_or_default(),
                e.limitation
            ),
            sources: vec![SourceRef {
                source: match e.source {
                    flight::TrackSource::AdsB => "ADS-B trajectory".into(),
                    flight::TrackSource::Qar => "QAR / FDM trajectory".into(),
                },
                data_age_seconds: e.data_age_seconds,
                record_ids: std::iter::once(e.flight_id.clone())
                    .chain(e.correlated_events.iter().cloned())
                    .collect(),
            }],
            model_version: e.model_version.clone(),
            evidence_grade: if e.evidence_grade == EvidenceGrade::EngineeringConfirmed {
                EvidenceGrade::Corroborated
            } else {
                e.evidence_grade
            },
            hypothesis_id: None,
            created_at: e.at,
            feedback: None,
            acknowledged: None,
            escalated_at: None,
        })
        .collect()
}

/// Full flight view with WRK-04 post-flight summary.
pub fn view(ds: &Dataset, events: &[FlightEvent], flight_id: &str, now: Ts) -> Option<Value> {
    let f = ds.flights.iter().find(|f| f.id == flight_id)?;
    let s: Option<&FlightSummary> = ds
        .flight_summaries
        .iter()
        .find(|s| s.flight_id == flight_id);
    let faults: Vec<_> = ds
        .events
        .iter()
        .filter(|e| e.flight_id.as_deref() == Some(flight_id))
        .collect();
    let history: Vec<Value> = faults
        .iter()
        .map(|e| {
            let prior = ds.events.iter().filter(|x| x.tail == f.tail && x.code == e.code && x.ts < e.ts).count();
            json!({ "event": e, "prior_occurrences_on_tail": prior,
                    "assessment": if prior >= 2 { "Recurring on this aircraft" } else if prior == 1 { "Seen once before" } else { "First occurrence" } })
        })
        .collect();
    let wos: Vec<_> = ds
        .work_orders
        .iter()
        .filter(|w| {
            w.event_ids
                .iter()
                .any(|id| faults.iter().any(|e| &e.id == id))
        })
        .collect();
    let fe: Vec<&FlightEvent> = events.iter().filter(|e| e.flight_id == flight_id).collect();
    let quality = s.map(|s| {
        json!({ "source": s.source, "samples": s.samples, "max_gap_s": s.max_gap_s,
                "received_at": s.received_at, "delay_minutes": (s.received_at - f.arr).num_minutes(),
                "complete": s.max_gap_s <= 300 })
    });
    Some(json!({
        "flight": f,
        "summary": s,
        "track": s.map(|s| flight::track(f.dep, f.arr, s)),
        "weather": { "departure": flight::weather(&f.origin, f.dep), "arrival": flight::weather(&f.dest, f.arr) },
        "notams": { "departure": flight::notams(&f.origin, f.dep), "arrival": flight::notams(&f.dest, f.arr) },
        "flight_events": fe,
        "data_quality": quality,
        "post_flight": {
            "new_defects": history,
            "corrective_actions": wos,
            "flight_events": fe.len(),
            "status": if fe.iter().any(|e| e.severity >= Severity::Caution) || faults.iter().any(|e| e.severity >= Severity::Caution) {
                "Findings need engineering review"
            } else if s.is_none() {
                "No trajectory data received"
            } else {
                "No findings in the available data"
            },
            "generated_at": now,
        },
    }))
}
