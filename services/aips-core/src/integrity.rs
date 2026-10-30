//! AIPS Integrity: parameter health analytics on top of the C++ signal library
//! (INT-01..INT-05). Produces alerts with source, data age, model version and reason.
use aips_domain::intel::{Alert, EvidenceGrade, Module, SourceRef};
use aips_domain::model::{Dataset, Param, Severity, Ts};
use aips_signal_sys::{self as signal, DetectorConfig, Features};
use std::collections::HashMap;
use uuid::Uuid;

pub const MAX_WINDOW: usize = 320;
pub const DISAGREE_THRESHOLD: f64 = 0.03;
pub const EGT_SLOPE_LIMIT: f64 = -3.0; // °C per 100 cycles

pub type SeriesKey = (String, Param);

#[derive(Default)]
pub struct SeriesIndex {
    pub series: HashMap<SeriesKey, Vec<(Ts, f64, String)>>,
}

impl SeriesIndex {
    pub fn build(ds: &Dataset) -> Self {
        let mut series: HashMap<SeriesKey, Vec<(Ts, f64, String)>> = HashMap::new();
        for s in &ds.samples {
            series.entry((s.tail.clone(), s.param)).or_default().push((
                s.ts,
                s.value,
                s.flight_id.clone(),
            ));
        }
        for v in series.values_mut() {
            v.sort_by_key(|x| x.0);
            if v.len() > MAX_WINDOW {
                v.drain(..v.len() - MAX_WINDOW);
            }
        }
        Self { series }
    }
    pub fn values(&self, tail: &str, p: Param) -> Vec<f64> {
        self.series
            .get(&(tail.to_string(), p))
            .map(|v| v.iter().map(|x| x.1).collect())
            .unwrap_or_default()
    }
}

pub fn config_for(p: Param) -> DetectorConfig {
    let m = p.meta();
    let mut c = DetectorConfig::default_for(m.lo, m.hi);
    // A long healthy baseline and a wide decision interval keep the in-control false-alarm
    // rate low when baseline mean and sigma are estimated from data (see METRICS.md).
    c.baseline_samples = 120;
    c.cusum_h = 12.0;
    if matches!(p, Param::EgtMarginEng1 | Param::EgtMarginEng2) {
        // EGT margin degrades by design; trend is judged by slope and the RUL model, not CUSUM.
        c.cusum_h = 1e12;
    }
    c
}

fn alert_id(tail: &str, kind: &str, p: Param) -> String {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("int|{tail}|{kind}|{p:?}").as_bytes(),
    )
    .to_string()
}

pub fn model_version() -> String {
    format!(
        "{} · EWMA α=0.2 · CUSUM k=0.5 h=12 (baseline 120)",
        signal::version()
    )
}

pub fn analyse(idx: &SeriesIndex, now: Ts) -> (Vec<Alert>, HashMap<SeriesKey, Features>) {
    let keys: Vec<&SeriesKey> = idx.series.keys().collect();
    let values: Vec<Vec<f64>> = keys
        .iter()
        .map(|k| idx.series[*k].iter().map(|x| x.1).collect())
        .collect();
    let slices: Vec<&[f64]> = values.iter().map(|v| v.as_slice()).collect();
    let cfgs: Vec<DetectorConfig> = keys.iter().map(|k| config_for(k.1)).collect();
    let feats = signal::extract_batch(&slices, &cfgs).unwrap_or_else(|e| {
        tracing::error!(?e, "signal batch extraction failed");
        vec![Features::default(); keys.len()]
    });

    let mv = model_version();
    let mut alerts = Vec::new();
    let mut out = HashMap::new();
    for ((key, f), vals) in keys.iter().zip(feats.iter()).zip(values.iter()) {
        out.insert((*key).clone(), *f);
        if f.n == 0 {
            continue;
        }
        let (tail, p) = (&key.0, key.1);
        let meta = p.meta();
        let series = &idx.series[*key];
        let last = series.last().unwrap();
        let src = SourceRef {
            source: "QAR / FDM parameter summary".into(),
            data_age_seconds: (now - last.0).num_seconds().max(0),
            record_ids: series.iter().rev().take(3).map(|x| x.2.clone()).collect(),
        };
        let mut push = |kind: &str, sev: Severity, title: String, reason: String| {
            alerts.push(Alert {
                id: alert_id(tail, kind, p),
                tail: tail.clone(),
                module: Module::Integrity,
                system: p.system(),
                severity: sev,
                title,
                reason,
                sources: vec![src.clone()],
                model_version: mv.clone(),
                evidence_grade: EvidenceGrade::Suspected,
                hypothesis_id: None,
                created_at: last.0,
                feedback: None,
                acknowledged: None,
                escalated_at: None,
            })
        };
        if f.frozen == 1 {
            push(
                "frozen",
                Severity::Caution,
                format!("{} sensor frozen", meta.label),
                format!(
                    "Last 12 readings identical at {:.1} {} (σ ≤ 1e-6). Sensor or acquisition fault suspected (INT-03).",
                    f.last, meta.unit
                ),
            );
        }
        if f.out_of_range > 0 {
            push(
                "range",
                Severity::Warning,
                format!("{} outside physical range", meta.label),
                format!(
                    "{} reading(s) outside [{}, {}] {}.",
                    f.out_of_range, meta.lo, meta.hi, meta.unit
                ),
            );
        }
        if f.cusum_alarm != 0 && f.frozen == 0 {
            let dir = if f.cusum_alarm > 0 {
                "upward"
            } else {
                "downward"
            };
            let shift = vals.iter().rev().take(10).sum::<f64>() / 10.0 - f.baseline_mean;
            push(
                "cusum",
                Severity::Caution,
                format!("{} {dir} shift", meta.label),
                format!(
                    "CUSUM drift alarm ({dir}); recent mean differs from baseline {:.1} {} by {:+.1} {} ({:.1}σ).",
                    f.baseline_mean,
                    meta.unit,
                    shift,
                    meta.unit,
                    shift / f.baseline_stddev.max(1e-9)
                ),
            );
        }
        if matches!(p, Param::EgtMarginEng1 | Param::EgtMarginEng2) {
            let tail_vals: Vec<f64> = vals
                .iter()
                .rev()
                .take(crate::predict::WINDOW)
                .rev()
                .copied()
                .collect();
            if let Ok(recent) = signal::extract(&tail_vals, &config_for(p)) {
                let per100 = recent.slope * 100.0;
                if per100 < EGT_SLOPE_LIMIT {
                    push(
                        "egt_slope",
                        Severity::Caution,
                        format!("{} deteriorating", meta.label),
                        format!(
                            "Margin trend {per100:.1} °C per 100 cycles over the last 120 cycles (fleet normal −0.2 to −0.6); current margin {:.1} °C.",
                            recent.last
                        ),
                    );
                }
            }
        }
    }

    // Redundant-sensor cross-check: green hydraulic sensors A and B.
    let tails: std::collections::BTreeSet<&String> = idx.series.keys().map(|k| &k.0).collect();
    for tail in tails {
        let a = idx.values(tail, Param::HydPressGreen);
        let b = idx.values(tail, Param::HydPressGreenAlt);
        let n = a.len().min(b.len()).min(20);
        if n < 10 {
            continue;
        }
        let d = signal::cross_sensor_disagreement(&a[a.len() - n..], &b[b.len() - n..]);
        if d > DISAGREE_THRESHOLD {
            let series = &idx.series[&(tail.clone(), Param::HydPressGreenAlt)];
            let last = series.last().unwrap();
            alerts.push(Alert {
                id: alert_id(tail, "disagree", Param::HydPressGreenAlt),
                tail: tail.clone(),
                module: Module::Integrity,
                system: Param::HydPressGreen.system(),
                severity: Severity::Caution,
                title: "Green hydraulic pressure sensors disagree".into(),
                reason: format!(
                    "Sensors A and B differ by {:.1}% on average over the last {n} flights (limit {:.0}%). One sensor may be drifting (INT-03).",
                    d * 100.0,
                    DISAGREE_THRESHOLD * 100.0
                ),
                sources: vec![SourceRef {
                    source: "QAR / FDM parameter summary".into(),
                    data_age_seconds: (now - last.0).num_seconds().max(0),
                    record_ids: series.iter().rev().take(3).map(|x| x.2.clone()).collect(),
                }],
                model_version: model_version(),
                evidence_grade: EvidenceGrade::Corroborated,
                hypothesis_id: None,
                created_at: last.0,
                feedback: None,
                acknowledged: None,
                escalated_at: None,
            });
        }
    }
    (alerts, out)
}
