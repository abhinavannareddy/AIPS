//! AIPS Cross-Flight Memory engine (PRD section 9; CFM-01..CFM-06, PRD-03, PRD-04, INT-04).
//!
//! The engine never asserts a root cause. It proposes evidence-graded hypotheses
//! ("Suspected" or "Corroborated"); only an authorised engineer can promote one to
//! "Engineering-confirmed" (CFM-03).
use crate::intel::{EvidenceGrade, Explanation, Hypothesis, HypothesisKind};
use crate::model::{Dataset, FaultEvent, System, Ts, WorkOrder};
use std::collections::{BTreeMap, HashMap, HashSet};
use uuid::Uuid;

pub const MODEL_VERSION: &str = "cfm-1.0.0";

#[derive(Debug, Clone)]
pub struct MemoryConfig {
    /// Maximum gap between consecutive occurrences of the same fault to stay in one cluster.
    pub recurrence_gap_days: i64,
    pub min_recurrence: usize,
    /// Two events in different systems within this window count as co-occurring.
    pub cross_window_minutes: i64,
    pub min_cooccurrence: u32,
    pub suspect_lift: f64,
    pub corroborate_lift: f64,
    pub corroborate_cooccurrence: u32,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            recurrence_gap_days: 21,
            min_recurrence: 3,
            cross_window_minutes: 120,
            min_cooccurrence: 3,
            suspect_lift: 8.0,
            corroborate_lift: 20.0,
            corroborate_cooccurrence: 4,
        }
    }
}

fn stable_id(parts: &[&str]) -> String {
    Uuid::new_v5(&Uuid::NAMESPACE_OID, parts.join("|").as_bytes()).to_string()
}

fn tokens(s: &str) -> HashSet<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() > 2)
        .map(|t| t.to_ascii_lowercase())
        .collect()
}

fn jaccard(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let inter = a.intersection(b).count() as f64;
    let union = a.union(b).count() as f64;
    inter / union
}

fn hours(a: Ts, b: Ts) -> f64 {
    (b - a).num_seconds() as f64 / 3600.0
}

/// Analyse the full dataset and return all hypotheses, ordered by tail then first occurrence.
pub fn analyse(ds: &Dataset, cfg: &MemoryConfig) -> Vec<Hypothesis> {
    let mut out = recurrences(ds, cfg);
    out.extend(cross_system(ds, cfg));
    out.sort_by(|a, b| {
        a.tail
            .cmp(&b.tail)
            .then(a.explanation.first_seen.cmp(&b.explanation.first_seen))
    });
    out
}

fn recurrences(ds: &Dataset, cfg: &MemoryConfig) -> Vec<Hypothesis> {
    let mut groups: BTreeMap<(&str, &str), Vec<&FaultEvent>> = BTreeMap::new();
    for e in &ds.events {
        groups
            .entry((e.tail.as_str(), e.code.as_str()))
            .or_default()
            .push(e);
    }
    let mut wo_by_tail: HashMap<&str, Vec<&WorkOrder>> = HashMap::new();
    for w in &ds.work_orders {
        wo_by_tail.entry(w.tail.as_str()).or_default().push(w);
    }

    let gap = chrono::Duration::days(cfg.recurrence_gap_days);
    let mut out = Vec::new();
    for ((tail, code), mut evs) in groups {
        evs.sort_by_key(|e| e.ts);
        let mut clusters: Vec<Vec<&FaultEvent>> = Vec::new();
        for e in evs {
            match clusters.last_mut() {
                Some(c) if e.ts - c.last().unwrap().ts <= gap => c.push(e),
                _ => clusters.push(vec![e]),
            }
        }
        for cluster in clusters
            .into_iter()
            .filter(|c| c.len() >= cfg.min_recurrence)
        {
            out.push(build_recurrence(tail, code, &cluster, wo_by_tail.get(tail)));
        }
    }
    out
}

fn build_recurrence(
    tail: &str,
    code: &str,
    cluster: &[&FaultEvent],
    wos: Option<&Vec<&WorkOrder>>,
) -> Hypothesis {
    let first = cluster.first().unwrap();
    let last = cluster.last().unwrap();
    let ids: HashSet<&str> = cluster.iter().map(|e| e.id.as_str()).collect();

    // Corrective actions that addressed this fault and were closed before a later recurrence.
    let mut fixes = Vec::new();
    if let Some(wos) = wos {
        for w in wos.iter() {
            let addresses = w.event_ids.iter().any(|id| ids.contains(id.as_str()))
                || (w.ata == first.ata
                    && w.component_id.is_some()
                    && w.component_id == first.component_id);
            if let (true, Some(closed)) = (addresses, w.closed) {
                if closed >= first.ts && closed < last.ts {
                    fixes.push(w.id.clone());
                }
            }
        }
    }
    fixes.sort();
    fixes.dedup();

    let max_gap = cluster
        .windows(2)
        .map(|w| hours(w[0].ts, w[1].ts))
        .fold(0.0, f64::max);
    let toks: Vec<HashSet<String>> = cluster.iter().map(|e| tokens(&e.message)).collect();
    let sim = if toks.len() > 1 {
        toks.windows(2).map(|w| jaccard(&w[0], &w[1])).sum::<f64>() / (toks.len() - 1) as f64
    } else {
        1.0
    };

    let kind = if fixes.is_empty() {
        HypothesisKind::Recurrence
    } else {
        HypothesisKind::RepeatAfterFix
    };
    let grade = match kind {
        // A documented fix followed by recurrence is two independent records agreeing.
        HypothesisKind::RepeatAfterFix => EvidenceGrade::Corroborated,
        _ if cluster.len() >= 4 && sim >= 0.5 => EvidenceGrade::Corroborated,
        _ => EvidenceGrade::Suspected,
    };
    let n = cluster.len();
    let days = (last.ts - first.ts).num_hours() as f64 / 24.0;
    let narrative = match kind {
        HypothesisKind::RepeatAfterFix => format!(
            "Fault {code} ({}) recurred {n} times over {days:.1} days on {tail}, including after {} documented corrective action(s). The fix may not have addressed the cause.",
            first.system.label(),
            fixes.len()
        ),
        _ => format!(
            "Fault {code} ({}) occurred {n} times over {days:.1} days on {tail}; longest gap {max_gap:.0} h.",
            first.system.label()
        ),
    };
    let title = match kind {
        HypothesisKind::RepeatAfterFix => format!("Repeat defect after repair: {code}"),
        _ => format!("Recurring defect: {code}"),
    };
    Hypothesis {
        id: stable_id(&["rec", tail, code, &first.id]),
        tail: tail.to_string(),
        kind,
        systems: vec![first.system],
        codes: vec![code.to_string()],
        event_ids: cluster.iter().map(|e| e.id.clone()).collect(),
        work_order_ids: fixes.clone(),
        machine_grade: grade,
        grade,
        graded_by: None,
        graded_at: None,
        explanation: Explanation {
            matching_fields: vec![
                "tail".into(),
                "fault_code".into(),
                "ata_chapter".into(),
                "symptom_text".into(),
            ],
            occurrences: n as u32,
            first_seen: first.ts,
            last_seen: last.ts,
            max_gap_hours: max_gap,
            median_gap_minutes: None,
            lift: None,
            expected_by_chance: None,
            symptom_similarity: Some(sim),
            fixes_between: fixes.len() as u32,
            narrative,
        },
        title,
        model_version: MODEL_VERSION.into(),
    }
}

struct PairStats<'a> {
    flights: HashSet<&'a str>,
    events: Vec<&'a FaultEvent>,
    gaps_min: Vec<f64>,
    systems: (System, System),
}

fn cross_system(ds: &Dataset, cfg: &MemoryConfig) -> Vec<Hypothesis> {
    // Fleet base rates: share of flights on which each fault code appears.
    let total_flights = ds.flights.len().max(1) as f64;
    let mut code_flights: HashMap<&str, HashSet<&str>> = HashMap::new();
    let mut tail_flights: HashMap<&str, usize> = HashMap::new();
    for f in &ds.flights {
        *tail_flights.entry(f.tail.as_str()).or_default() += 1;
    }
    let mut by_flight: BTreeMap<(&str, &str), Vec<&FaultEvent>> = BTreeMap::new();
    for e in &ds.events {
        if let Some(fid) = &e.flight_id {
            code_flights
                .entry(e.code.as_str())
                .or_default()
                .insert(fid.as_str());
            by_flight
                .entry((e.tail.as_str(), fid.as_str()))
                .or_default()
                .push(e);
        }
    }

    let window = cfg.cross_window_minutes as f64;
    let mut pairs: BTreeMap<(&str, &str, &str), PairStats> = BTreeMap::new();
    for ((tail, fid), evs) in &by_flight {
        for (i, a) in evs.iter().enumerate() {
            for b in evs.iter().skip(i + 1) {
                if a.system == b.system || a.code == b.code {
                    continue;
                }
                let gap = (b.ts - a.ts).num_seconds().abs() as f64 / 60.0;
                if gap > window {
                    continue;
                }
                let (x, y) = if a.code <= b.code { (*a, *b) } else { (*b, *a) };
                let st = pairs
                    .entry((tail, x.code.as_str(), y.code.as_str()))
                    .or_insert_with(|| PairStats {
                        flights: HashSet::new(),
                        events: Vec::new(),
                        gaps_min: Vec::new(),
                        systems: (x.system, y.system),
                    });
                if st.flights.insert(fid) {
                    st.events.push(x);
                    st.events.push(y);
                    st.gaps_min.push(gap);
                }
            }
        }
    }

    let mut out = Vec::new();
    for ((tail, ca, cb), mut st) in pairs {
        let n = st.flights.len() as u32;
        if n < cfg.min_cooccurrence {
            continue;
        }
        let pa = code_flights.get(ca).map_or(0, |s| s.len()) as f64 / total_flights;
        let pb = code_flights.get(cb).map_or(0, |s| s.len()) as f64 / total_flights;
        let tf = *tail_flights.get(tail).unwrap_or(&1) as f64;
        let expected = (tf * pa * pb).max(1e-6);
        let lift = n as f64 / expected;
        if lift < cfg.suspect_lift {
            continue;
        }
        st.events.sort_by_key(|e| e.ts);
        st.gaps_min.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median = st.gaps_min[st.gaps_min.len() / 2];
        let grade = if n >= cfg.corroborate_cooccurrence && lift >= cfg.corroborate_lift {
            EvidenceGrade::Corroborated
        } else {
            EvidenceGrade::Suspected
        };
        let first = st.events.first().unwrap();
        let last = st.events.last().unwrap();
        let max_gap = st
            .events
            .windows(2)
            .map(|w| hours(w[0].ts, w[1].ts))
            .fold(0.0, f64::max);
        out.push(Hypothesis {
            id: stable_id(&["xsys", tail, ca, cb, &first.id]),
            tail: tail.to_string(),
            kind: HypothesisKind::CrossSystem,
            systems: vec![st.systems.0, st.systems.1],
            codes: vec![ca.to_string(), cb.to_string()],
            event_ids: st.events.iter().map(|e| e.id.clone()).collect(),
            work_order_ids: vec![],
            machine_grade: grade,
            grade,
            graded_by: None,
            graded_at: None,
            explanation: Explanation {
                matching_fields: vec!["tail".into(), "flight".into(), "time_window".into()],
                occurrences: n,
                first_seen: first.ts,
                last_seen: last.ts,
                max_gap_hours: max_gap,
                median_gap_minutes: Some(median),
                lift: Some(lift),
                expected_by_chance: Some(expected),
                symptom_similarity: None,
                fixes_between: 0,
                narrative: format!(
                    "{ca} ({}) and {cb} ({}) occurred together on {n} flights of {tail}, median {median:.0} min apart. Fleet base rates predict {expected:.2} such flights by chance (lift {lift:.0}x). This is a suspected relationship, not an established cause.",
                    st.systems.0.label(),
                    st.systems.1.label()
                ),
            },
            title: format!(
                "Possible link: {} ↔ {}",
                st.systems.0.label(),
                st.systems.1.label()
            ),
            model_version: MODEL_VERSION.into(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use chrono::{Duration, TimeZone, Utc};

    fn ev(id: &str, tail: &str, fid: &str, ts: Ts, ata: u8, code: &str) -> FaultEvent {
        FaultEvent {
            id: id.into(),
            tail: tail.into(),
            flight_id: Some(fid.into()),
            ts,
            ata,
            system: System::from_ata(ata),
            code: code.into(),
            component_id: Some("C1".into()),
            severity: Severity::Caution,
            message: format!("{code} pressure low green system"),
            source: DataSource::Acars,
            ingested_at: ts,
        }
    }

    fn flight(id: &str, tail: &str, t: Ts) -> Flight {
        Flight {
            id: id.into(),
            tail: tail.into(),
            number: "XX1".into(),
            origin: "ARN".into(),
            dest: "CPH".into(),
            dep: t,
            arr: t + Duration::hours(1),
            cycle: 1,
        }
    }

    #[test]
    fn detects_recurrence_and_repeat_after_fix() {
        let t0 = Utc.with_ymd_and_hms(2026, 1, 1, 8, 0, 0).unwrap();
        let mut ds = Dataset::default();
        for i in 0..4 {
            let t = t0 + Duration::days(i * 3);
            ds.flights.push(flight(&format!("F{i}"), "SE-AAA", t));
            ds.events.push(ev(
                &format!("E{i}"),
                "SE-AAA",
                &format!("F{i}"),
                t,
                29,
                "29-11-01",
            ));
        }
        ds.work_orders.push(WorkOrder {
            id: "W1".into(),
            tail: "SE-AAA".into(),
            event_ids: vec!["E0".into()],
            component_id: Some("C1".into()),
            ata: 29,
            action: MaintAction::Replaced,
            opened: t0 + Duration::hours(2),
            closed: Some(t0 + Duration::hours(6)),
            signed_off: true,
            team: "Hydraulics".into(),
            note: "".into(),
        });
        let hs = analyse(&ds, &MemoryConfig::default());
        assert_eq!(hs.len(), 1);
        assert_eq!(hs[0].kind, HypothesisKind::RepeatAfterFix);
        assert_eq!(hs[0].grade, EvidenceGrade::Corroborated);
        assert_eq!(hs[0].explanation.fixes_between, 1);
    }

    #[test]
    fn two_occurrences_are_not_a_recurrence() {
        let t0 = Utc.with_ymd_and_hms(2026, 1, 1, 8, 0, 0).unwrap();
        let mut ds = Dataset::default();
        for i in 0..2 {
            let t = t0 + Duration::days(i);
            ds.flights.push(flight(&format!("F{i}"), "SE-AAA", t));
            ds.events.push(ev(
                &format!("E{i}"),
                "SE-AAA",
                &format!("F{i}"),
                t,
                29,
                "29-11-01",
            ));
        }
        assert!(analyse(&ds, &MemoryConfig::default()).is_empty());
    }

    #[test]
    fn cross_system_link_needs_lift() {
        let t0 = Utc.with_ymd_and_hms(2026, 1, 1, 8, 0, 0).unwrap();
        let mut ds = Dataset::default();
        // 200 flights; the pair co-occurs on 5 of them and never otherwise.
        for i in 0..200 {
            let t = t0 + Duration::hours(i * 6);
            ds.flights.push(flight(&format!("F{i}"), "SE-BBB", t));
            if i % 40 == 0 {
                ds.events.push(ev(
                    &format!("A{i}"),
                    "SE-BBB",
                    &format!("F{i}"),
                    t,
                    24,
                    "24-22-05",
                ));
                ds.events.push(ev(
                    &format!("B{i}"),
                    "SE-BBB",
                    &format!("F{i}"),
                    t + Duration::minutes(9),
                    77,
                    "77-11-03",
                ));
            }
        }
        let hs = analyse(&ds, &MemoryConfig::default());
        let x: Vec<_> = hs
            .iter()
            .filter(|h| h.kind == HypothesisKind::CrossSystem)
            .collect();
        assert_eq!(x.len(), 1);
        assert!(x[0].explanation.lift.unwrap() > 20.0);
        assert_eq!(x[0].grade, EvidenceGrade::Corroborated);
        // Stable identity across recomputation (grades persist by id).
        let again = analyse(&ds, &MemoryConfig::default());
        assert_eq!(
            again
                .iter()
                .find(|h| h.kind == HypothesisKind::CrossSystem)
                .unwrap()
                .id,
            x[0].id
        );
    }
}
