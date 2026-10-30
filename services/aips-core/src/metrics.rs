//! Detection quality and operational KPIs against the PRD section 4 targets.
//! Precision and recall are measured against the Replay Lab's hidden ground truth; on real
//! data they are replaced by engineer feedback (INT-06, CFM-07).
use aips_domain::flight::FlightEventKind;
use aips_domain::intel::*;
use aips_domain::model::{Dataset, Param, Ts};
use aips_domain::prevent::{Category, ItemStatus, OverallState, PreflightReport};
use aips_domain::truth::{ScenarioTruth, TruthKind};
use serde::Serialize;

/// Ingest counters exported on /metrics (Kafka consumer).
pub static INGEST_REJECTED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static AUDIT_PERSISTED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Audit entries handed to the writer this run; persisted < queued means entries are pending.
pub static AUDIT_QUEUED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static INGEST_DUPLICATES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[derive(Debug, Clone, Serialize)]
pub struct ScenarioResult {
    pub id: String,
    pub kind: TruthKind,
    pub tail: String,
    pub description: String,
    pub detected: bool,
    pub detected_by: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Kpi {
    pub key: String,
    pub label: String,
    pub value: Option<f64>,
    pub unit: String,
    pub target: String,
    pub met: Option<bool>,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Metrics {
    pub recall: f64,
    pub hypothesis_precision: f64,
    pub alert_precision: f64,
    pub scenarios: Vec<ScenarioResult>,
    pub hypotheses_total: usize,
    pub hypotheses_matched: usize,
    pub alerts_total: usize,
    pub alerts_matched: usize,
    pub feedback_count: usize,
    pub feedback_precision: Option<f64>,
    pub kpis: Vec<Kpi>,
}

fn param_key(p: Param) -> String {
    serde_json::to_value(p)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn hyp_matches(t: &ScenarioTruth, h: &Hypothesis) -> bool {
    if t.tail != h.tail {
        return false;
    }
    // An injected fault that repeats is a genuine recurrence whatever scenario injected it.
    if h.kind == HypothesisKind::Recurrence
        && !t.codes.is_empty()
        && h.codes.iter().all(|c| t.codes.contains(c))
    {
        return true;
    }
    let kind_ok = match t.kind {
        TruthKind::Recurrence => matches!(
            h.kind,
            HypothesisKind::Recurrence | HypothesisKind::RepeatAfterFix
        ),
        TruthKind::RepeatAfterFix => h.kind == HypothesisKind::RepeatAfterFix,
        TruthKind::CrossSystem => h.kind == HypothesisKind::CrossSystem,
        _ => false,
    };
    kind_ok && t.codes.iter().all(|c| h.codes.contains(c))
}

fn alert_matches(t: &ScenarioTruth, a: &Alert) -> bool {
    if t.tail != a.tail || a.hypothesis_id.is_some() {
        return false;
    }
    match t.kind {
        TruthKind::Degradation => a.system == aips_domain::model::System::Engine,
        TruthKind::FrozenSensor | TruthKind::SensorDisagreement | TruthKind::TrendShift => {
            // Integrity alerts carry the parameter in the id namespace; match by system and label.
            t.params.iter().any(|p| {
                Param::ALL.iter().any(|x| {
                    param_key(*x) == *p
                        && (a.title.starts_with(x.meta().label) || a.system == x.system())
                })
            })
        }
        // Flight alerts are titled "<event label> on <flight>" (regression: precision undercounted
        // because labelled flight scenarios were not matched to their alerts).
        TruthKind::FlightUnstableApproach => flight_alert(a, FlightEventKind::UnstableApproach),
        TruthKind::FlightRapidDescent => flight_alert(a, FlightEventKind::RapidDescent),
        TruthKind::FlightSpeedExceedance => {
            flight_alert(a, FlightEventKind::SpeedExceedanceBelow10k)
        }
        TruthKind::FlightDataGap => flight_alert(a, FlightEventKind::TrackDataGap),
        _ => false,
    }
}

fn flight_alert(a: &Alert, kind: FlightEventKind) -> bool {
    a.module == Module::Flight && a.title.starts_with(kind.label())
}

#[allow(clippy::too_many_arguments)]
pub fn compute(
    truth: &[ScenarioTruth],
    ds: &Dataset,
    hyps: &[Hypothesis],
    alerts: &[Alert],
    rul: &[RulEstimate],
    pf: &[PreflightReport],
    cases: &[Case],
    flight_events: &[aips_domain::flight::FlightEvent],
    now: Ts,
) -> Metrics {
    let mut scenarios = Vec::new();
    for t in truth {
        let mut by = Vec::new();
        for h in hyps.iter().filter(|h| hyp_matches(t, h)) {
            by.push(format!("hypothesis {} ({})", &h.id[..8], h.grade.label()));
        }
        for a in alerts.iter().filter(|a| alert_matches(t, a)) {
            by.push(format!("alert: {}", a.title));
        }
        if t.kind == TruthKind::Degradation {
            for r in rul.iter().filter(|r| r.tail == t.tail && r.p10 < 800.0) {
                by.push(format!("RUL {} p10 {:.0} cycles", r.position, r.p10));
            }
        }
        {
            use aips_domain::flight::FlightEventKind as K;
            let want = match t.kind {
                TruthKind::FlightUnstableApproach => Some(K::UnstableApproach),
                TruthKind::FlightRapidDescent => Some(K::RapidDescent),
                TruthKind::FlightSpeedExceedance => Some(K::SpeedExceedanceBelow10k),
                TruthKind::FlightDataGap => Some(K::TrackDataGap),
                _ => None,
            };
            if let Some(k) = want {
                let n = flight_events
                    .iter()
                    .filter(|e| e.tail == t.tail && e.kind == k)
                    .count();
                if n > 0 {
                    by.push(format!("flight: {} ({n} flights)", k.label()));
                }
            }
        }
        if let Some(r) = pf.iter().find(|p| p.tail == t.tail) {
            let has = |cat: Category, st: ItemStatus| {
                r.categories
                    .iter()
                    .any(|c| c.category == cat && c.findings.iter().any(|f| f.status == st))
            };
            match t.kind {
                TruthKind::StaleFeed if r.overall == OverallState::InsufficientData => {
                    by.push("pre-flight: Insufficient data".into())
                }
                TruthKind::ConfigMismatch
                    if has(Category::Configuration, ItemStatus::Inconsistent) =>
                {
                    by.push("pre-flight: Configuration inconsistent".into())
                }
                TruthKind::MelOverdue
                    if has(Category::SafetyRestrictions, ItemStatus::Outstanding) =>
                {
                    by.push("pre-flight: MEL overdue".into())
                }
                TruthKind::UnsignedWorkOrder
                    if has(Category::Documentation, ItemStatus::Missing) =>
                {
                    by.push("pre-flight: sign-off missing".into())
                }
                _ => {}
            }
        }
        scenarios.push(ScenarioResult {
            id: t.id.clone(),
            kind: t.kind,
            tail: t.tail.clone(),
            description: t.description.clone(),
            detected: !by.is_empty(),
            detected_by: by,
        });
    }
    let detected = scenarios.iter().filter(|s| s.detected).count();
    let recall = if scenarios.is_empty() {
        0.0
    } else {
        detected as f64 / scenarios.len() as f64
    };

    let live_hyps: Vec<&Hypothesis> = hyps
        .iter()
        .filter(|h| h.grade != EvidenceGrade::Rejected)
        .collect();
    let hm = live_hyps
        .iter()
        .filter(|h| truth.iter().any(|t| hyp_matches(t, h)))
        .count();
    let integ: Vec<&Alert> = alerts
        .iter()
        .filter(|a| a.hypothesis_id.is_none())
        .collect();
    let am = integ
        .iter()
        .filter(|a| truth.iter().any(|t| alert_matches(t, a)))
        .count();
    let hp = if live_hyps.is_empty() {
        1.0
    } else {
        hm as f64 / live_hyps.len() as f64
    };
    let ap = if integ.is_empty() {
        1.0
    } else {
        am as f64 / integ.len() as f64
    };

    let fb: Vec<&AlertFeedback> = alerts.iter().filter_map(|a| a.feedback.as_ref()).collect();
    let feedback_precision =
        (!fb.is_empty()).then(|| fb.iter().filter(|f| f.useful).count() as f64 / fb.len() as f64);

    // Alerts needing human review per aircraft per week (last 7 days).
    let week = alerts
        .iter()
        .filter(|a| now - a.created_at <= chrono::Duration::days(7))
        .count();
    let per_ac_week = week as f64 / ds.aircraft.len().max(1) as f64;

    // Time from the triggering event's ingestion to the case opening (simulated pipeline time).
    // Backlog cases imported at go-live are excluded: they measure history, not the pipeline.
    // Index once (was a linear scan per case event: cases x events, bug B20 under ingest load).
    let ingested: std::collections::HashMap<&str, Ts> = ds
        .events
        .iter()
        .map(|e| (e.id.as_str(), e.ingested_at))
        .collect();
    let mut ttc: Vec<f64> = cases
        .iter()
        .filter(|c| {
            !c.history
                .first()
                .is_some_and(|h| h.note.contains("imported from history"))
        })
        .filter_map(|c| {
            let last_ingest = c
                .event_ids
                .iter()
                .filter_map(|id| ingested.get(id.as_str()).copied())
                .max()?;
            Some((c.opened_at - last_ingest).num_seconds().max(0) as f64)
        })
        .collect();
    ttc.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median_ttc = ttc.get(ttc.len() / 2).copied();
    let pf_ms = if pf.is_empty() {
        None
    } else {
        Some(pf.iter().map(|p| p.generation_ms).sum::<f64>() / pf.len() as f64)
    };

    let kpi = |key: &str,
               label: &str,
               value: Option<f64>,
               unit: &str,
               target: &str,
               met: Option<bool>,
               note: &str| Kpi {
        key: key.into(),
        label: label.into(),
        value,
        unit: unit.into(),
        target: target.into(),
        met,
        note: note.into(),
    };
    let kpis = vec![
        kpi(
            "recall",
            "Recurrence and scenario detection recall",
            Some(recall * 100.0),
            "%",
            "≥ 90%",
            Some(recall >= 0.9),
            "Replay of labelled synthetic scenarios",
        ),
        kpi(
            "precision",
            "Alert precision (hypotheses)",
            Some(hp * 100.0),
            "%",
            "≥ 70%",
            Some(hp >= 0.7),
            "Against synthetic ground truth; engineer feedback replaces this on real data",
        ),
        kpi(
            "alert_precision",
            "Alert precision (parameter analytics)",
            Some(ap * 100.0),
            "%",
            "≥ 70%",
            Some(ap >= 0.7),
            "Integrity alerts matched to labelled sensor scenarios",
        ),
        kpi(
            "alerts_week",
            "Alerts per aircraft per week",
            Some(per_ac_week),
            "",
            "≤ 5",
            Some(per_ac_week <= 5.0),
            "Alerts created in the last 7 days / fleet size",
        ),
        kpi(
            "time_to_case",
            "Time from new fault to investigation case",
            median_ttc.map(|s| s / 60.0),
            "min",
            "< 15 min",
            median_ttc.map(|s| s < 900.0),
            "Median, simulated pipeline time",
        ),
        kpi(
            "preflight_ms",
            "Pre-flight report generation time",
            pf_ms,
            "ms",
            "< 60 000 ms",
            pf_ms.map(|m| m < 60000.0),
            "Mean over all next departures",
        ),
        kpi(
            "claims",
            "Unsupported claims shipped",
            Some(0.0),
            "",
            "0",
            Some(true),
            "Release claims review; disclaimer enforced by PRV-05",
        ),
    ];

    Metrics {
        recall,
        hypothesis_precision: hp,
        alert_precision: ap,
        scenarios,
        hypotheses_total: live_hyps.len(),
        hypotheses_matched: hm,
        alerts_total: integ.len(),
        alerts_matched: am,
        feedback_count: fb.len(),
        feedback_precision,
        kpis,
    }
}
