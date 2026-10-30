//! AIPS Prevent: pre-departure safety assurance (PRV-01..PRV-06).
//!
//! The report classifies evidence; it never states that an aircraft is airworthy and it
//! has no approve/deny output (principle P1). Stale data yields "Insufficient data",
//! never a clear result (PRV-06, principle P4).
use crate::intel::{
    Alert, Case, CaseStatus, EvidenceGrade, Hypothesis, HypothesisKind, RulEstimate,
};
use crate::model::{Component, Dataset, MaintAction, ScheduledDeparture, Severity, System, Ts};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const DISCLAIMER: &str = "AIPS is advisory. It does not declare airworthiness and does not authorise or prohibit take-off. Those decisions remain with legally authorised personnel and existing procedures.";
pub const STALE_AFTER_HOURS: i64 = 12;
pub const RUL_REVIEW_CYCLES: f32 = 800.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Engines,
    Electrical,
    Fuel,
    Hydraulics,
    FlightControls,
    Configuration,
    Maintenance,
    SafetyRestrictions,
    Documentation,
}

impl Category {
    pub const ALL: [Category; 9] = [
        Category::Engines,
        Category::Electrical,
        Category::Fuel,
        Category::Hydraulics,
        Category::FlightControls,
        Category::Configuration,
        Category::Maintenance,
        Category::SafetyRestrictions,
        Category::Documentation,
    ];
    pub fn label(&self) -> &'static str {
        match self {
            Category::Engines => "Engines",
            Category::Electrical => "Electrical",
            Category::Fuel => "Fuel",
            Category::Hydraulics => "Hydraulics",
            Category::FlightControls => "Flight controls",
            Category::Configuration => "Configuration",
            Category::Maintenance => "Maintenance",
            Category::SafetyRestrictions => "Safety restrictions",
            Category::Documentation => "Documentation",
        }
    }
    fn systems(&self) -> &'static [System] {
        match self {
            Category::Engines => &[System::Engine],
            Category::Electrical => &[System::Electrical, System::Avionics],
            Category::Fuel => &[System::Fuel],
            Category::Hydraulics => &[System::Hydraulic, System::LandingGear],
            Category::FlightControls => &[System::FlightControls],
            // Airframe catch-all: every System must belong to a category, or its faults (e.g.
            // ATA 26 fire protection, 35 oxygen, 49 APU map to Other) never reach the report (P4).
            Category::Configuration => &[System::CabinPressure, System::Other],
            _ => &[],
        }
    }
    fn technical(&self) -> bool {
        !self.systems().is_empty()
    }
}

/// PRV-02 classifications plus PRV-06 "Insufficient data".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    Complete,
    RequiresApproval,
    Missing,
    Inconsistent,
    Outstanding,
    InsufficientData,
}

impl ItemStatus {
    fn rank(&self) -> u8 {
        match self {
            ItemStatus::Complete => 0,
            ItemStatus::RequiresApproval => 1,
            ItemStatus::Missing => 2,
            ItemStatus::Inconsistent => 3,
            ItemStatus::Outstanding => 4,
            ItemStatus::InsufficientData => 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub category: Category,
    pub status: ItemStatus,
    pub severity: Severity,
    pub title: String,
    pub detail: String,
    pub evidence_grade: Option<EvidenceGrade>,
    pub record_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategorySummary {
    pub category: Category,
    pub label: String,
    pub status: ItemStatus,
    pub findings: Vec<Finding>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverallState {
    /// Critical data is stale or missing: never presented as clear (PRV-06).
    InsufficientData,
    /// One or more findings need authorised review.
    ReviewRequired,
    /// No findings in the available data. This is not an airworthiness statement.
    NoFindingsInAvailableData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Freshness {
    pub source: String,
    pub last_received: Ts,
    pub age_seconds: i64,
    pub stale: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflightReport {
    pub id: String,
    pub flight_id: String,
    pub tail: String,
    pub type_code: String,
    pub number: String,
    pub origin: String,
    pub dest: String,
    pub std: Ts,
    pub generated_at: Ts,
    pub generation_ms: f64,
    pub overall: OverallState,
    pub counts: HashMap<String, u32>,
    pub categories: Vec<CategorySummary>,
    pub ranked: Vec<Finding>,
    pub freshness: Vec<Freshness>,
    pub disclaimer: String,
    pub acknowledged_by: Option<String>,
    pub acknowledged_at: Option<Ts>,
    /// WRK-01 operational context: weather and NOTAMs at departure and arrival.
    pub context: OperationalContext,
    /// PRV-04: set when an unreviewed significant concern was escalated before departure.
    #[serde(default)]
    pub escalated_at: Option<Ts>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationalContext {
    pub departure_weather: crate::flight::Weather,
    pub arrival_weather: crate::flight::Weather,
    pub notams: Vec<crate::flight::Notam>,
    pub source: String,
}

pub struct Inputs<'a> {
    pub ds: &'a Dataset,
    pub hypotheses: &'a [Hypothesis],
    pub alerts: &'a [Alert],
    pub cases: &'a [Case],
    pub rul: &'a [RulEstimate],
    /// False when no remaining-life model is loaded: then the Engines category says so (P4)
    /// instead of silently showing no remaining-life concern.
    pub rul_available: bool,
    pub now: Ts,
}

pub fn build(inp: &Inputs, dep: &ScheduledDeparture) -> PreflightReport {
    let started = std::time::Instant::now();
    let ds = inp.ds;
    let tail = dep.tail.as_str();
    let aircraft = ds.aircraft.iter().find(|a| a.tail == tail);
    let feed_last = aircraft
        .map(|a| a.data_feed_last)
        .unwrap_or(inp.now - chrono::Duration::days(365));
    let feed_age = (inp.now - feed_last).num_seconds().max(0);
    let stale = feed_age > STALE_AFTER_HOURS * 3600;

    let latest_wo = ds
        .work_orders
        .iter()
        .filter(|w| w.tail == tail)
        .map(|w| w.closed.unwrap_or(w.opened))
        .max()
        .unwrap_or(feed_last);
    let freshness = vec![
        Freshness {
            source: "Aircraft health feed (ACARS / CMC)".into(),
            last_received: feed_last,
            age_seconds: feed_age,
            stale,
        },
        Freshness {
            source: "Maintenance information system".into(),
            last_received: latest_wo,
            age_seconds: (inp.now - latest_wo).num_seconds().max(0),
            stale: false,
        },
    ];

    let mut findings: Vec<Finding> = Vec::new();
    let closed_event_ids: std::collections::HashSet<&str> = ds
        .work_orders
        .iter()
        .filter(|w| w.tail == tail && w.closed.is_some() && w.action != MaintAction::Deferred)
        .flat_map(|w| w.event_ids.iter().map(|s| s.as_str()))
        .collect();

    for cat in Category::ALL.iter().filter(|c| c.technical()) {
        let systems = cat.systems();
        if stale {
            findings.push(Finding {
                category: *cat,
                status: ItemStatus::InsufficientData,
                severity: Severity::Caution,
                title: "Aircraft data is stale".into(),
                detail: format!(
                    "Latest aircraft data is {:.1} h old (limit {STALE_AFTER_HOURS} h). AIPS cannot assess this category.",
                    feed_age as f64 / 3600.0
                ),
                evidence_grade: None,
                record_ids: vec![],
            });
        }
        // Unresolved recent faults (last 72 h, no closing corrective action).
        let recent: Vec<_> = ds
            .events
            .iter()
            .filter(|e| {
                e.tail == tail
                    && systems.contains(&e.system)
                    && inp.now - e.ts <= chrono::Duration::hours(72)
                    && e.ts <= inp.now
                    && !closed_event_ids.contains(e.id.as_str())
                    && e.severity >= Severity::Advisory
            })
            .collect();
        if !recent.is_empty() {
            let sev = recent.iter().map(|e| e.severity).max().unwrap();
            let codes: Vec<_> = recent.iter().map(|e| e.code.as_str()).collect();
            findings.push(Finding {
                category: *cat,
                status: ItemStatus::Outstanding,
                severity: sev,
                title: format!(
                    "{} unresolved fault report(s) in the last 72 h",
                    recent.len()
                ),
                detail: format!("Codes: {}", dedup_join(&codes)),
                evidence_grade: None,
                record_ids: recent.iter().map(|e| e.id.clone()).collect(),
            });
        }
        for h in inp.hypotheses.iter().filter(|h| {
            h.tail == tail
                && h.grade != EvidenceGrade::Rejected
                && h.systems.iter().any(|s| systems.contains(s))
        }) {
            findings.push(Finding {
                category: *cat,
                status: ItemStatus::Outstanding,
                severity: if h.kind == HypothesisKind::RepeatAfterFix {
                    Severity::Caution
                } else {
                    Severity::Advisory
                },
                title: h.title.clone(),
                detail: h.explanation.narrative.clone(),
                evidence_grade: Some(h.grade),
                record_ids: vec![h.id.clone()],
            });
        }
        for a in inp
            .alerts
            .iter()
            .filter(|a| a.tail == tail && systems.contains(&a.system) && a.hypothesis_id.is_none())
        {
            let inconsistent = a.title.contains("frozen") || a.title.contains("disagree");
            findings.push(Finding {
                category: *cat,
                status: if inconsistent {
                    ItemStatus::Inconsistent
                } else {
                    ItemStatus::Outstanding
                },
                severity: a.severity,
                title: a.title.clone(),
                detail: a.reason.clone(),
                evidence_grade: Some(a.evidence_grade),
                record_ids: vec![a.id.clone()],
            });
        }
        if *cat == Category::Engines {
            if !inp.rul_available {
                findings.push(Finding {
                    category: *cat,
                    status: ItemStatus::InsufficientData,
                    severity: Severity::Caution,
                    title: "Remaining-life model unavailable".into(),
                    detail:
                        "No remaining-life model is loaded, so component life cannot be assessed."
                            .into(),
                    evidence_grade: None,
                    record_ids: vec![],
                });
            }
            for r in inp
                .rul
                .iter()
                .filter(|r| r.tail == tail && (r.p10.is_nan() || r.p10 < RUL_REVIEW_CYCLES))
            // NaN shown, not dropped
            {
                findings.push(Finding {
                    category: *cat,
                    status: ItemStatus::Outstanding,
                    severity: if r.p10 < 300.0 { Severity::Warning } else { Severity::Caution },
                    title: format!("Engine {} EGT margin trend: low remaining life estimate", r.position),
                    detail: format!(
                        "Estimated remaining cycles p10 {:.0} / p50 {:.0} / p90 {:.0} (margin {:.1} °C, trend {:.2} °C per 100 cycles).",
                        r.p10, r.p50, r.p90, r.margin_c, r.slope_per_100cyc
                    ),
                    evidence_grade: Some(EvidenceGrade::Suspected),
                    record_ids: vec![r.component_id.clone()],
                });
            }
        }
        for w in ds.work_orders.iter().filter(|w| {
            w.tail == tail
                && w.action == MaintAction::Deferred
                && w.closed.is_none()
                && systems.contains(&System::from_ata(w.ata))
        }) {
            findings.push(Finding {
                category: *cat,
                status: ItemStatus::RequiresApproval,
                severity: Severity::Advisory,
                title: "Deferred defect".into(),
                detail: format!(
                    "{}. Deferral requires authorised review before departure.",
                    w.note
                ),
                evidence_grade: None,
                record_ids: vec![w.id.clone()],
            });
        }
    }

    // Configuration: a component serial recorded as installed on more than one aircraft.
    let mut serials: HashMap<(&str, &str), Vec<&str>> = HashMap::new();
    for c in &ds.components {
        serials
            .entry((c.part_number.as_str(), c.serial.as_str()))
            .or_default()
            .push(c.tail.as_str());
    }
    for c in ds.components.iter().filter(|c| c.tail == tail) {
        if let Some(tails) = serials.get(&(c.part_number.as_str(), c.serial.as_str())) {
            if tails.len() > 1 {
                findings.push(Finding {
                    category: Category::Configuration,
                    status: ItemStatus::Inconsistent,
                    severity: Severity::Caution,
                    title: "Component recorded on more than one aircraft".into(),
                    detail: format!(
                        "{} P/N {} S/N {} ({}) is also recorded on {}.",
                        c.description,
                        c.part_number,
                        c.serial,
                        c.position,
                        tails
                            .iter()
                            .filter(|t| **t != tail)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    evidence_grade: None,
                    record_ids: vec![c.id.clone()],
                });
            }
        }
    }

    // Maintenance: open investigation cases on this aircraft.
    for c in inp
        .cases
        .iter()
        .filter(|c| c.tail == tail && c.status != CaseStatus::Closed)
    {
        findings.push(Finding {
            category: Category::Maintenance,
            status: ItemStatus::Outstanding,
            severity: c.priority,
            title: format!("Open investigation: {}", c.title),
            detail: format!("Assigned to {}, status {:?}.", c.team, c.status),
            evidence_grade: None,
            record_ids: vec![c.id.clone()],
        });
    }

    // Safety restrictions: MEL items.
    for m in ds
        .mel
        .iter()
        .filter(|m| m.tail == tail && m.opened <= inp.now)
    {
        let overdue = m.due < inp.now;
        findings.push(Finding {
            category: Category::SafetyRestrictions,
            status: if overdue {
                ItemStatus::Outstanding
            } else {
                ItemStatus::RequiresApproval
            },
            severity: if overdue {
                Severity::Warning
            } else {
                Severity::Advisory
            },
            title: format!("MEL {} cat {}: {}", m.reference, m.category, m.title),
            detail: if overdue {
                format!(
                    "Rectification interval expired {}.",
                    m.due.format("%Y-%m-%d %H:%M UTC")
                )
            } else {
                format!(
                    "Open until {}; operational restrictions apply.",
                    m.due.format("%Y-%m-%d %H:%M UTC")
                )
            },
            evidence_grade: None,
            record_ids: vec![m.id.clone()],
        });
    }

    // Documentation: closed corrective actions without certifying sign-off.
    for w in ds
        .work_orders
        .iter()
        .filter(|w| w.tail == tail && w.closed.is_some() && !w.signed_off)
    {
        findings.push(Finding {
            category: Category::Documentation,
            status: ItemStatus::Missing,
            severity: Severity::Caution,
            title: "Corrective action lacks sign-off record".into(),
            detail: format!(
                "Work order {} ({:?}) is closed without a recorded certifying signature.",
                w.id, w.action
            ),
            evidence_grade: None,
            record_ids: vec![w.id.clone()],
        });
    }

    let categories: Vec<CategorySummary> = Category::ALL
        .iter()
        .map(|c| {
            let fs: Vec<Finding> = findings
                .iter()
                .filter(|f| f.category == *c)
                .cloned()
                .collect();
            let status = fs
                .iter()
                .map(|f| f.status)
                .max_by_key(|s| s.rank())
                .unwrap_or(ItemStatus::Complete);
            CategorySummary {
                category: *c,
                label: c.label().into(),
                status,
                findings: fs,
            }
        })
        .collect();

    let mut ranked = findings.clone();
    ranked.sort_by(|a, b| {
        b.status
            .rank()
            .cmp(&a.status.rank())
            .then(b.severity.cmp(&a.severity))
            .then(b.evidence_grade.cmp(&a.evidence_grade))
    });
    let mut counts = HashMap::new();
    for f in &findings {
        *counts
            .entry(format!("{:?}", f.status).to_lowercase())
            .or_insert(0u32) += 1;
    }
    let overall = if stale {
        OverallState::InsufficientData
    } else if findings.is_empty() {
        OverallState::NoFindingsInAvailableData
    } else {
        OverallState::ReviewRequired
    };

    PreflightReport {
        id: format!("PF-{}", dep.flight_id),
        flight_id: dep.flight_id.clone(),
        tail: tail.into(),
        type_code: aircraft.map(|a| a.type_code.clone()).unwrap_or_default(),
        number: dep.number.clone(),
        origin: dep.origin.clone(),
        dest: dep.dest.clone(),
        std: dep.std,
        generated_at: inp.now,
        generation_ms: started.elapsed().as_secs_f64() * 1000.0,
        overall,
        counts,
        categories,
        ranked,
        freshness,
        disclaimer: DISCLAIMER.into(),
        acknowledged_by: None,
        acknowledged_at: None,
        context: OperationalContext {
            departure_weather: crate::flight::weather(&dep.origin, dep.std),
            arrival_weather: crate::flight::weather(
                &dep.dest,
                dep.std + chrono::Duration::hours(2),
            ),
            notams: crate::flight::notams(&dep.origin, dep.std)
                .into_iter()
                .chain(crate::flight::notams(&dep.dest, dep.std))
                .collect(),
            source: "Synthetic METAR/NOTAM feed (Phase 0)".into(),
        },
        escalated_at: None,
    }
}

fn dedup_join(v: &[&str]) -> String {
    let mut u: Vec<&str> = v.to_vec();
    u.sort();
    u.dedup();
    u.join(", ")
}

/// PRV-04: a report needs escalation when it carries a Warning or cannot be assessed.
/// Build all reports in one pass. Equivalent to calling `build` for each departure (see the
/// `build_all_matches_build` property test) but indexes the dataset by aircraft first, so the
/// cost is linear in fleet size instead of departures x records (NFR-04: 500 aircraft).
pub fn build_all(inp: &Inputs, deps: &[ScheduledDeparture]) -> Vec<PreflightReport> {
    let ds = inp.ds;
    let mut by_tail: HashMap<&str, Dataset> = HashMap::new();
    for a in &ds.aircraft {
        by_tail
            .entry(a.tail.as_str())
            .or_default()
            .aircraft
            .push(a.clone());
    }
    for e in &ds.events {
        by_tail
            .entry(e.tail.as_str())
            .or_default()
            .events
            .push(e.clone());
    }
    for w in &ds.work_orders {
        by_tail
            .entry(w.tail.as_str())
            .or_default()
            .work_orders
            .push(w.clone());
    }
    for m in &ds.mel {
        by_tail
            .entry(m.tail.as_str())
            .or_default()
            .mel
            .push(m.clone());
    }
    // Components: own, plus any other aircraft's component sharing a part/serial pair, so the
    // fleet-wide configuration consistency check sees exactly what it would see on the full set.
    let mut serials: HashMap<(&str, &str), Vec<&Component>> = HashMap::new();
    for c in &ds.components {
        serials
            .entry((c.part_number.as_str(), c.serial.as_str()))
            .or_default()
            .push(c);
    }
    for c in &ds.components {
        let sub = by_tail.entry(c.tail.as_str()).or_default();
        for other in &serials[&(c.part_number.as_str(), c.serial.as_str())] {
            if !sub.components.iter().any(|x| x.id == other.id) {
                sub.components.push((*other).clone());
            }
        }
    }
    let empty = Dataset::default();
    let mut h_by: HashMap<&str, Vec<Hypothesis>> = HashMap::new();
    for h in inp.hypotheses {
        h_by.entry(h.tail.as_str()).or_default().push(h.clone());
    }
    let mut a_by: HashMap<&str, Vec<Alert>> = HashMap::new();
    for a in inp.alerts {
        a_by.entry(a.tail.as_str()).or_default().push(a.clone());
    }
    let mut c_by: HashMap<&str, Vec<Case>> = HashMap::new();
    for c in inp.cases {
        c_by.entry(c.tail.as_str()).or_default().push(c.clone());
    }
    let mut r_by: HashMap<&str, Vec<RulEstimate>> = HashMap::new();
    for r in inp.rul {
        r_by.entry(r.tail.as_str()).or_default().push(r.clone());
    }
    deps.iter()
        .map(|d| {
            let t = d.tail.as_str();
            let sub = Inputs {
                ds: by_tail.get(t).unwrap_or(&empty),
                hypotheses: h_by.get(t).map(Vec::as_slice).unwrap_or(&[]),
                alerts: a_by.get(t).map(Vec::as_slice).unwrap_or(&[]),
                cases: c_by.get(t).map(Vec::as_slice).unwrap_or(&[]),
                rul: r_by.get(t).map(Vec::as_slice).unwrap_or(&[]),
                rul_available: inp.rul_available,
                now: inp.now,
            };
            build(&sub, d)
        })
        .collect()
}

pub fn needs_escalation(r: &PreflightReport) -> bool {
    r.overall == OverallState::InsufficientData
        || r.ranked.iter().any(|f| f.severity == Severity::Warning)
}
