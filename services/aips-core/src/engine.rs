//! The AIPS intelligence core: projects the operational dataset at the simulation clock,
//! runs Integrity, Predict, Cross-Flight Memory and Prevent, opens and escalates cases,
//! and merges human decisions (grades, feedback, case actions) that always take priority.
use crate::integrity::{self, SeriesIndex};
use crate::metrics::{self, Metrics};
use crate::predict;
use crate::predict::ActiveModel;
use aips_domain::audit::AuditChain;
use aips_domain::intel::*;
use aips_domain::memory::{self, MemoryConfig};
use aips_domain::model::*;
use aips_domain::prevent::{self, OverallState, PreflightReport};
use aips_domain::truth::ScenarioTruth;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowMode {
    /// SLA escalation evaluated in-process (single-node / demonstrator).
    Embedded,
    /// SLA escalation delegated to Temporal workflows (aips-workflow worker).
    Temporal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GradeDecision {
    pub grade: EvidenceGrade,
    pub by: String,
    pub at: Ts,
    pub note: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CaseHuman {
    pub status: Option<CaseStatus>,
    pub assignee: Option<String>,
    pub team: Option<String>,
    pub history: Vec<CaseAction>,
    pub escalation_level: u8,
}

/// Human decisions. These are the system of record and are persisted (P1, CFM-03, INT-06).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Decisions {
    pub grades: HashMap<String, GradeDecision>,
    pub feedback: HashMap<String, AlertFeedback>,
    pub cases: HashMap<String, CaseHuman>,
    pub preflight_acks: HashMap<String, (String, Ts)>,
    #[serde(default)]
    pub alert_acks: HashMap<String, AlertAck>,
    /// PSP-02 append-only corrections, in the order they were made.
    #[serde(default)]
    pub corrections: Vec<Correction>,
    /// PRV-04 escalations recorded by the workflow worker (Temporal mode).
    #[serde(default)]
    pub preflight_escalations: HashMap<String, Ts>,
    #[serde(default)]
    pub erasure_requests: Vec<ErasureRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clock {
    pub sim_now: Ts,
    pub data_end: Ts,
    pub speed: f64,
    pub running: bool,
}

impl Clock {
    pub fn tick(&mut self, real_secs: f64) {
        if !self.running {
            return;
        }
        let real_now = Utc::now();
        if self.sim_now < self.data_end {
            let step = Duration::milliseconds((real_secs * self.speed * 1000.0) as i64);
            self.sim_now = (self.sim_now + step).min(self.data_end);
        } else {
            // Past the end of recorded data the clock follows wall time (feeds age honestly).
            self.sim_now = self
                .sim_now
                .max(real_now.min(self.data_end + Duration::days(3)));
            self.speed = 1.0;
        }
    }
    pub fn is_live(&self) -> bool {
        self.sim_now >= self.data_end
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AircraftView {
    pub tail: String,
    pub type_code: String,
    pub operator: String,
    pub lessor: Option<String>,
    pub base: String,
    pub msn: u32,
    pub health_score: u32,
    pub status: String,
    pub open_alerts: usize,
    pub warnings: usize,
    pub cautions: usize,
    pub open_cases: usize,
    pub hypotheses: usize,
    pub next_departure: Option<ScheduledDeparture>,
    pub preflight: Option<OverallState>,
    pub data_age_seconds: i64,
    pub cycles: u32,
    pub rul_min_p10: Option<f32>,
    pub last_flight: Option<String>,
}

#[derive(Clone)]
pub struct Snapshot {
    pub clock: Clock,
    pub visible: Arc<Dataset>,
    pub series: Arc<SeriesIndex>,
    pub hypotheses: Vec<Hypothesis>,
    pub alerts: Vec<Alert>,
    pub rul: Vec<RulEstimate>,
    pub cases: Vec<Case>,
    pub preflight: Vec<PreflightReport>,
    pub aircraft: Vec<AircraftView>,
    pub metrics: Metrics,
    pub flight_events: Vec<aips_domain::flight::FlightEvent>,
    pub drift: predict::Drift,
    pub stages: Vec<(String, f64)>,
    pub recompute_ms: f64,
    pub version: u64,
}

pub struct Computed {
    pub snap: Arc<Snapshot>,
    pub opened_at: Vec<(String, Ts)>,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErasureRequest {
    pub id: String,
    pub tail: String,
    pub reason: String,
    pub requested_by: String,
    pub requested_at: Ts,
    pub approved_by: Option<String>,
    pub executed_at: Option<Ts>,
    pub rows_erased: Option<u64>,
}

/// WRK-03: an unacknowledged Warning escalates after this long.
pub const ALERT_ACK_WINDOW_MIN: i64 = 30;
/// PRV-04: unreviewed significant pre-departure concerns escalate this long before departure.
pub const PREFLIGHT_ESCALATE_BEFORE_MIN: i64 = 45;

pub struct Outcome {
    pub new_alerts: Vec<Alert>,
    pub opened_cases: Vec<Case>,
    pub escalated_cases: Vec<Case>,
    pub escalated_alerts: Vec<Alert>,
    pub escalated_preflight: Vec<String>,
    pub new_preflight_concerns: Vec<PreflightReport>,
}

pub struct Engine {
    /// Shared so an analysis pass can work on a consistent copy without holding the engine lock;
    /// writers copy-on-write via `Arc::make_mut` (at most one dataset copy per pass).
    pub full: Arc<Dataset>,
    pub truth: Vec<ScenarioTruth>,
    pub clock: Clock,
    pub decisions: Decisions,
    pub audit: AuditChain,
    pub snap: Arc<Snapshot>,
    pub rul_model: Option<Arc<ActiveModel>>,
    pub workflow: WorkflowMode,
    pub mem_cfg: MemoryConfig,
    /// First simulation time each case was opened (time-to-case KPI).
    pub case_opened: HashMap<String, Ts>,
    /// When AIPS started analysing this history. Faults recorded before go-live are imported
    /// as a backlog and their cases open at go-live rather than in the past.
    pub go_live: Ts,
}

impl Engine {
    pub fn new(
        full: Dataset,
        truth: Vec<ScenarioTruth>,
        clock: Clock,
        rul_model: Option<Arc<ActiveModel>>,
        workflow: WorkflowMode,
    ) -> Self {
        let empty = Snapshot {
            stages: Vec::new(),
            clock: clock.clone(),
            visible: Arc::new(Dataset::default()),
            series: Arc::new(SeriesIndex::default()),
            hypotheses: vec![],
            alerts: vec![],
            rul: vec![],
            cases: vec![],
            preflight: vec![],
            aircraft: vec![],
            metrics: Metrics::default(),
            flight_events: vec![],
            drift: Default::default(),
            recompute_ms: 0.0,
            version: 0,
        };
        let go_live = clock.sim_now;
        Self {
            full: Arc::new(full),
            truth,
            clock,
            decisions: Decisions::default(),
            audit: AuditChain::default(),
            snap: Arc::new(empty),
            rul_model,
            workflow,
            mem_cfg: MemoryConfig::default(),
            case_opened: HashMap::new(),
            go_live,
        }
    }

    /// Cheap copy of everything an analysis pass reads (the dataset is shared, not copied).
    /// Lets `recompute_and_publish` release the engine lock before the expensive pass, so
    /// ingestion and readers are never queued behind analysis (bug B12, 500-aircraft fleet).
    pub fn pass_view(&self) -> Engine {
        Engine {
            full: self.full.clone(),
            truth: self.truth.clone(),
            clock: self.clock.clone(),
            decisions: self.decisions.clone(),
            audit: AuditChain::default(),
            snap: self.snap.clone(),
            rul_model: self.rul_model.clone(),
            workflow: self.workflow,
            mem_cfg: self.mem_cfg.clone(),
            case_opened: self.case_opened.clone(),
            go_live: self.go_live,
        }
    }

    /// The dataset as it was known at the simulation clock (no future data leaks in).
    fn project(&self) -> Dataset {
        let now = self.clock.sim_now;
        let f = &self.full;
        let flights: Vec<Flight> = f.flights.iter().filter(|x| x.arr <= now).cloned().collect();
        // Latest data per aircraft: arrival summaries, plus in-flight ACARS position/status
        // reports (modelled as every 10 minutes while airborne).
        let mut last_arr: HashMap<&str, Ts> = HashMap::new();
        for fl in f.flights.iter().filter(|x| x.dep <= now) {
            let seen = if fl.arr <= now {
                fl.arr
            } else {
                fl.dep + Duration::minutes(((now - fl.dep).num_minutes() / 10) * 10)
            };
            let e = last_arr.entry(fl.tail.as_str()).or_insert(seen);
            if seen > *e {
                *e = seen;
            }
        }
        let aircraft = f
            .aircraft
            .iter()
            .map(|a| {
                let mut a = a.clone();
                if let Some(t) = last_arr.get(a.tail.as_str()) {
                    a.data_feed_last = *t;
                }
                a
            })
            .collect();
        let work_orders = f
            .work_orders
            .iter()
            .filter(|w| w.opened <= now)
            .map(|w| {
                let mut w = self.corrected_work_order(w);
                if w.closed.is_some_and(|c| c > now) {
                    w.closed = None;
                }
                w
            })
            .collect();
        // Next departure: the first recorded flight after `now`, else the planned departure.
        // One pass over flights (was departures x flights: 12 s at 500 aircraft, bug B11).
        let mut next_by_tail: HashMap<&str, &Flight> = HashMap::new();
        for x in f.flights.iter().filter(|x| x.dep > now) {
            let e = next_by_tail.entry(x.tail.as_str()).or_insert(x);
            if x.dep < e.dep {
                *e = x;
            }
        }
        let mut departures = Vec::new();
        for d in &f.departures {
            let next = next_by_tail.get(d.tail.as_str()).copied();
            departures.push(match next {
                Some(n) => ScheduledDeparture {
                    flight_id: n.id.clone(),
                    tail: n.tail.clone(),
                    number: n.number.clone(),
                    origin: n.origin.clone(),
                    dest: n.dest.clone(),
                    std: n.dep,
                },
                None => d.clone(),
            });
        }
        Dataset {
            generated_at: f.generated_at,
            seed: f.seed,
            aircraft,
            components: f.components.clone(),
            flights,
            events: f
                .events
                .iter()
                .filter(|e| e.ingested_at <= now)
                .map(|e| self.corrected_event(e))
                .collect(),
            work_orders,
            mel: f.mel.iter().filter(|m| m.opened <= now).cloned().collect(),
            samples: f.samples.iter().filter(|s| s.ts <= now).cloned().collect(),
            departures,
            flight_summaries: f
                .flight_summaries
                .iter()
                .filter(|s| s.received_at <= now)
                .cloned()
                .collect(),
        }
    }

    /// Pure analysis over the current state. Runs under a read lock so API readers are not
    /// blocked while it executes (performance fix: see ENGINEERING_REPORT, bottleneck B1).
    pub fn compute(&self) -> Computed {
        let started = std::time::Instant::now();
        // Per-stage timings, exposed on /api/metrics for capacity work (NFR-04).
        let mut stages: Vec<(&'static str, f64)> = Vec::new();
        let mut lap = std::time::Instant::now();
        let mut mark = |name: &'static str| {
            stages.push((name, lap.elapsed().as_secs_f64() * 1000.0));
            lap = std::time::Instant::now();
        };
        let now = self.clock.sim_now;
        let visible = self.project();
        mark("project");
        let series = SeriesIndex::build(&visible);
        mark("series_index");

        // ---- Integrity and Predict ----
        let (mut alerts, _features) = integrity::analyse(&series, now);
        mark("integrity");
        let (rul, drift) = predict::estimate(&visible, &series, self.rul_model.as_ref());
        mark("predict_rul");

        // ---- Cross-Flight Memory, with human grades applied over machine grades ----
        let mut hypotheses = memory::analyse(&visible, &self.mem_cfg);
        mark("memory");
        for h in hypotheses.iter_mut() {
            if let Some(d) = self.decisions.grades.get(&h.id) {
                h.grade = d.grade;
                h.graded_by = Some(d.by.clone());
                h.graded_at = Some(d.at);
            }
        }
        let ev_by_id: HashMap<&str, &FaultEvent> =
            visible.events.iter().map(|e| (e.id.as_str(), e)).collect();
        for h in hypotheses
            .iter()
            .filter(|h| h.grade != EvidenceGrade::Rejected)
        {
            let evs: Vec<&&FaultEvent> = h
                .event_ids
                .iter()
                .filter_map(|id| ev_by_id.get(id.as_str()))
                .collect();
            let newest = evs.iter().map(|e| e.ingested_at).max().unwrap_or(now);
            let mut by_source: HashMap<&str, Vec<String>> = HashMap::new();
            for e in &evs {
                by_source
                    .entry(e.source.label())
                    .or_default()
                    .push(e.id.clone());
            }
            let severity = match h.kind {
                HypothesisKind::RepeatAfterFix | HypothesisKind::CrossSystem => Severity::Caution,
                HypothesisKind::Recurrence if h.explanation.occurrences >= 4 => Severity::Caution,
                HypothesisKind::Recurrence => Severity::Advisory,
            };
            alerts.push(Alert {
                id: format!("A-{}", &h.id[..13]),
                tail: h.tail.clone(),
                module: if h.kind == HypothesisKind::CrossSystem {
                    Module::Integrity
                } else {
                    Module::Predict
                },
                system: h.systems[0],
                severity,
                title: h.title.clone(),
                reason: h.explanation.narrative.clone(),
                sources: by_source
                    .into_iter()
                    .map(|(s, ids)| SourceRef {
                        source: s.into(),
                        data_age_seconds: (now - newest).num_seconds().max(0),
                        record_ids: ids,
                    })
                    .collect(),
                model_version: h.model_version.clone(),
                evidence_grade: h.grade,
                hypothesis_id: Some(h.id.clone()),
                created_at: newest,
                feedback: None,
                acknowledged: None,
                escalated_at: None,
            });
        }
        for r in rul.iter().filter(|r| r.p10 < prevent::RUL_REVIEW_CYCLES) {
            alerts.push(Alert {
                id: format!("A-RUL-{}", r.component_id),
                tail: r.tail.clone(),
                module: Module::Predict,
                system: System::Engine,
                severity: if r.p10 < 300.0 { Severity::Warning } else { Severity::Caution },
                title: format!("Engine {} low remaining-life estimate", &r.position[3..]),
                reason: format!(
                    "Remaining cycles p10 {:.0} / p50 {:.0} / p90 {:.0}; EGT margin {:.1} °C, trend {:.2} °C per 100 cycles. Estimate only; plan an engineering review.",
                    r.p10, r.p50, r.p90, r.margin_c, r.slope_per_100cyc
                ),
                sources: vec![SourceRef { source: "QAR / FDM parameter summary".into(), data_age_seconds: 0, record_ids: vec![r.component_id.clone()] }],
                model_version: r.model_version.clone(),
                evidence_grade: EvidenceGrade::Suspected,
                hypothesis_id: None,
                created_at: now,
                feedback: None,
                acknowledged: None,
                escalated_at: None,
            });
        }
        // ---- Flight (FLT-01..04): observable trajectory facts, recent ones become alerts ----
        mark("alerts");
        let flight_events = crate::flight::analyse(&visible, now);
        alerts.extend(crate::flight::alerts(&flight_events, now));
        mark("flight");
        let prev_escalated: HashSet<&str> = self
            .snap
            .alerts
            .iter()
            .filter(|a| a.escalated_at.is_some())
            .map(|a| a.id.as_str())
            .collect();
        for a in alerts.iter_mut() {
            a.feedback = self.decisions.feedback.get(&a.id).cloned();
            a.acknowledged = self.decisions.alert_acks.get(&a.id).cloned();
            let due = a.created_at + Duration::minutes(ALERT_ACK_WINDOW_MIN);
            if a.severity == Severity::Warning && a.acknowledged.is_none() && now >= due {
                a.escalated_at = Some(due);
            }
        }
        let escalated_alerts: Vec<Alert> = alerts
            .iter()
            .filter(|a| a.escalated_at.is_some() && !prev_escalated.contains(a.id.as_str()))
            .cloned()
            .collect();
        alerts.sort_by(|a, b| {
            b.severity
                .cmp(&a.severity)
                .then(b.created_at.cmp(&a.created_at))
        });

        mark("alert_workflow");
        // ---- Investigation cases (PRD-05, WRK-02) ----
        let mut cases = self.build_cases(&hypotheses, &alerts, &rul, now);
        let mut escalated = Vec::new();
        if self.workflow == WorkflowMode::Embedded {
            for c in cases.iter_mut() {
                if c.status != CaseStatus::Closed && now > c.sla_due && c.escalation_level == 0 {
                    let action = CaseAction {
                        at: c.sla_due,
                        actor: "aips-workflow".into(),
                        action: "escalated".into(),
                        note: "SLA breached; escalated to duty engineering manager (embedded workflow)".into(),
                    };
                    c.escalation_level = 1;
                    c.status = CaseStatus::Escalated;
                    c.history.push(action);
                    escalated.push(c.clone());
                }
            }
        }
        let prev_case_ids: HashSet<&str> = self.snap.cases.iter().map(|c| c.id.as_str()).collect();
        let opened: Vec<Case> = cases
            .iter()
            .filter(|c| !prev_case_ids.contains(c.id.as_str()))
            .cloned()
            .collect();
        let prev_alert_ids: HashSet<&str> =
            self.snap.alerts.iter().map(|a| a.id.as_str()).collect();
        let new_alerts: Vec<Alert> = alerts
            .iter()
            .filter(|a| !prev_alert_ids.contains(a.id.as_str()))
            .cloned()
            .collect();
        let mut opened_at: Vec<(String, Ts)> = Vec::new();
        for c in &cases {
            opened_at.push((c.id.clone(), c.opened_at));
        }

        mark("cases");
        // ---- Prevent: one report per next departure ----
        let inputs = prevent::Inputs {
            ds: &visible,
            hypotheses: &hypotheses,
            alerts: &alerts,
            cases: &cases,
            rul: &rul,
            rul_available: self.rul_model.is_some(),
            now,
        };
        let mut preflight: Vec<PreflightReport> = prevent::build_all(&inputs, &visible.departures);
        let prev_pf: HashMap<&str, &PreflightReport> = self
            .snap
            .preflight
            .iter()
            .map(|p| (p.flight_id.as_str(), p))
            .collect();
        let mut escalated_preflight = Vec::new();
        let mut new_preflight_concerns = Vec::new();
        for p in preflight.iter_mut() {
            if let Some((by, at)) = self.decisions.preflight_acks.get(&p.flight_id) {
                p.acknowledged_by = Some(by.clone());
                p.acknowledged_at = Some(*at);
            }
            if prevent::needs_escalation(p) && p.acknowledged_by.is_none() {
                let was = prev_pf.get(p.flight_id.as_str());
                if was.is_none_or(|w| !prevent::needs_escalation(w)) {
                    new_preflight_concerns.push(p.clone());
                }
                let due = p.std - Duration::minutes(PREFLIGHT_ESCALATE_BEFORE_MIN);
                p.escalated_at = match self.workflow {
                    WorkflowMode::Temporal => self
                        .decisions
                        .preflight_escalations
                        .get(&p.flight_id)
                        .copied(),
                    WorkflowMode::Embedded => (now >= due).then_some(due),
                };
                if p.escalated_at.is_some() && was.is_none_or(|w| w.escalated_at.is_none()) {
                    escalated_preflight.push(p.flight_id.clone());
                }
            }
        }
        preflight.sort_by_key(|p| p.std);

        mark("prevent_reports");
        let aircraft = self.views(
            &visible,
            &alerts,
            &cases,
            &hypotheses,
            &preflight,
            &rul,
            now,
        );
        mark("aircraft_views");
        let metrics = metrics::compute(
            &self.truth,
            &visible,
            &hypotheses,
            &alerts,
            &rul,
            &preflight,
            &cases,
            &flight_events,
            now,
        );

        cases.sort_by(|a, b| {
            (a.status == CaseStatus::Closed)
                .cmp(&(b.status == CaseStatus::Closed))
                .then(b.priority.cmp(&a.priority))
                .then(a.sla_due.cmp(&b.sla_due))
        });
        let version = self.snap.version + 1;
        mark("metrics_and_views");
        let snap = Arc::new(Snapshot {
            stages: stages.iter().map(|(n, ms)| (n.to_string(), *ms)).collect(),
            clock: self.clock.clone(),
            visible: Arc::new(visible),
            series: Arc::new(series),
            hypotheses,
            alerts,
            rul,
            cases,
            preflight,
            aircraft,
            metrics,
            flight_events,
            drift,
            recompute_ms: started.elapsed().as_secs_f64() * 1000.0,
            version,
        });
        Computed {
            snap,
            opened_at,
            outcome: Outcome {
                new_alerts,
                opened_cases: opened,
                escalated_cases: escalated,
                escalated_alerts,
                escalated_preflight,
                new_preflight_concerns,
            },
        }
    }

    /// Apply a computed pass. Idempotent for escalations: a case already escalated by a
    /// concurrent decision is not escalated twice.
    pub fn commit(&mut self, c: Computed) -> Outcome {
        for case in &c.outcome.escalated_cases {
            let human = self.decisions.cases.entry(case.id.clone()).or_default();
            if human.escalation_level == 0 {
                human.escalation_level = 1;
                human.status = Some(CaseStatus::Escalated);
                if let Some(a) = case.history.last() {
                    human.history.push(a.clone());
                }
            }
        }
        for (id, at) in c.opened_at {
            self.case_opened.entry(id).or_insert(at);
        }
        self.snap = c.snap;
        c.outcome
    }

    fn build_cases(
        &self,
        hyps: &[Hypothesis],
        alerts: &[Alert],
        rul: &[RulEstimate],
        now: Ts,
    ) -> Vec<Case> {
        let mut cases = Vec::new();
        let go_live = self.go_live;
        let sla = |sev: Severity| match sev {
            Severity::Warning => Duration::hours(4),
            Severity::Caution => Duration::hours(24),
            _ => Duration::hours(72),
        };
        let mut mk = |id: String,
                      tail: &str,
                      title: String,
                      kind: &str,
                      system: System,
                      prio: Severity,
                      opened: Ts,
                      hyp_ids: Vec<String>,
                      ev_ids: Vec<String>,
                      reason: String| {
            let human = self.decisions.cases.get(&id).cloned().unwrap_or_default();
            // `opened` is the trigger time. A case is opened when the analysis pass first sees it
            // (measured, not assumed); triggers before go-live are imported backlog.
            let backlog = opened < go_live;
            let opened = match self.case_opened.get(&id) {
                Some(first) => *first,
                None if backlog => go_live,
                None => now,
            };
            let mut history = vec![CaseAction {
                at: opened,
                actor: "aips-core".into(),
                action: "opened".into(),
                note: if backlog {
                    format!("{reason} (imported from history at AIPS go-live)")
                } else {
                    reason
                },
            }];
            history.extend(human.history.iter().cloned());
            let auto_closed = !hyp_ids.is_empty()
                && hyp_ids.iter().all(|h| {
                    self.decisions
                        .grades
                        .get(h)
                        .is_some_and(|d| d.grade == EvidenceGrade::Rejected)
                });
            let mut status = human.status.unwrap_or(if human.assignee.is_some() {
                CaseStatus::Assigned
            } else {
                CaseStatus::Open
            });
            if auto_closed && status != CaseStatus::Closed {
                status = CaseStatus::Closed;
                history.push(CaseAction {
                    at: now,
                    actor: "aips-core".into(),
                    action: "closed".into(),
                    note: "All linked hypotheses rejected by engineering".into(),
                });
            }
            cases.push(Case {
                id,
                tail: tail.into(),
                title,
                kind: kind.into(),
                system,
                priority: prio,
                status,
                team: human.team.clone().unwrap_or_else(|| system.team().into()),
                assignee: human.assignee.clone(),
                opened_at: opened,
                sla_due: opened + sla(prio),
                hypothesis_ids: hyp_ids,
                event_ids: ev_ids,
                escalation_level: human.escalation_level,
                history,
            });
        };
        for h in hyps {
            // A recurrence already explained by a cross-system hypothesis is investigated there.
            let covered = h.kind == HypothesisKind::Recurrence
                && hyps.iter().any(|x| {
                    x.kind == HypothesisKind::CrossSystem
                        && x.tail == h.tail
                        && x.codes.iter().any(|c| h.codes.contains(c))
                });
            if covered {
                continue;
            }
            let open = match h.kind {
                HypothesisKind::Recurrence | HypothesisKind::RepeatAfterFix => true,
                HypothesisKind::CrossSystem => {
                    h.machine_grade >= EvidenceGrade::Corroborated
                        || h.grade >= EvidenceGrade::Corroborated
                }
            };
            if !open {
                continue;
            }
            let prio = if h.kind == HypothesisKind::Recurrence && h.explanation.occurrences < 4 {
                Severity::Advisory
            } else {
                Severity::Caution
            };
            mk(
                format!("CASE-{}", h.id[..8].to_uppercase()),
                &h.tail,
                h.title.clone(),
                match h.kind {
                    HypothesisKind::Recurrence => "recurring_defect",
                    HypothesisKind::RepeatAfterFix => "repeat_repair",
                    HypothesisKind::CrossSystem => "cross_system",
                },
                h.systems[0],
                prio,
                h.explanation.last_seen + Duration::seconds(30),
                vec![h.id.clone()],
                h.event_ids.clone(),
                format!("Opened automatically: {}", h.explanation.narrative),
            );
        }
        for r in rul.iter().filter(|r| r.p10 < prevent::RUL_REVIEW_CYCLES) {
            mk(
                format!("CASE-RUL-{}", r.component_id),
                &r.tail,
                format!("Engine {} EGT margin review", &r.position[3..]),
                "degradation",
                System::Engine,
                if r.p10 < 300.0 { Severity::Warning } else { Severity::Caution },
                now.min(self.snap.cases.iter().find(|c| c.id == format!("CASE-RUL-{}", r.component_id)).map(|c| c.opened_at).unwrap_or(now)),
                vec![],
                vec![],
                format!("Remaining-life estimate p10 {:.0} cycles is below the {} cycle review threshold.", r.p10, prevent::RUL_REVIEW_CYCLES),
            );
        }
        for a in alerts.iter().filter(|a| {
            a.hypothesis_id.is_none()
                && (a.title.contains("frozen") || a.title.contains("disagree"))
        }) {
            mk(
                format!("CASE-SNS-{}", a.id[..8].to_uppercase()),
                &a.tail,
                format!("Sensor reliability: {}", a.title),
                "sensor_reliability",
                a.system,
                Severity::Caution,
                a.created_at + Duration::seconds(30),
                vec![],
                vec![],
                a.reason.clone(),
            );
        }
        cases
    }

    #[allow(clippy::too_many_arguments)]
    fn views(
        &self,
        ds: &Dataset,
        alerts: &[Alert],
        cases: &[Case],
        hyps: &[Hypothesis],
        pf: &[PreflightReport],
        rul: &[RulEstimate],
        now: Ts,
    ) -> Vec<AircraftView> {
        // Group once by tail (was aircraft x records; part of bug B11 at 500 aircraft).
        let mut al_by: HashMap<&str, Vec<&Alert>> = HashMap::new();
        for x in alerts
            .iter()
            .filter(|x| x.evidence_grade != EvidenceGrade::Rejected)
        {
            al_by.entry(x.tail.as_str()).or_default().push(x);
        }
        let mut cases_by: HashMap<&str, usize> = HashMap::new();
        for c in cases.iter().filter(|c| c.status != CaseStatus::Closed) {
            *cases_by.entry(c.tail.as_str()).or_default() += 1;
        }
        let mut hyp_by: HashMap<&str, usize> = HashMap::new();
        for h in hyps.iter().filter(|h| h.grade != EvidenceGrade::Rejected) {
            *hyp_by.entry(h.tail.as_str()).or_default() += 1;
        }
        let mut pf_by: HashMap<&str, &PreflightReport> = HashMap::new();
        for p in pf {
            pf_by.entry(p.tail.as_str()).or_insert(p);
        }
        let mut last_by: HashMap<&str, &Flight> = HashMap::new();
        for f in &ds.flights {
            let e = last_by.entry(f.tail.as_str()).or_insert(f);
            if f.arr > e.arr {
                *e = f;
            }
        }
        let mut dep_by: HashMap<&str, &ScheduledDeparture> = HashMap::new();
        for d in &ds.departures {
            dep_by.entry(d.tail.as_str()).or_insert(d);
        }
        let mut rul_by: HashMap<&str, f32> = HashMap::new();
        for r in rul {
            let e = rul_by.entry(r.tail.as_str()).or_insert(r.p10);
            *e = e.min(r.p10);
        }
        let no_alerts: Vec<&Alert> = Vec::new();
        let mut out = Vec::new();
        for a in &ds.aircraft {
            let al: &Vec<&Alert> = al_by.get(a.tail.as_str()).unwrap_or(&no_alerts);
            let warnings = al
                .iter()
                .filter(|x| x.severity == Severity::Warning)
                .count();
            let cautions = al
                .iter()
                .filter(|x| x.severity == Severity::Caution)
                .count();
            let advisories = al
                .iter()
                .filter(|x| x.severity <= Severity::Advisory)
                .count();
            let open_cases = cases_by.get(a.tail.as_str()).copied().unwrap_or(0);
            let report = pf_by.get(a.tail.as_str()).copied();
            let age = (now - a.data_feed_last).num_seconds().max(0);
            let penalty = warnings * 22 + cautions * 9 + advisories * 3 + open_cases * 4;
            let score = 100u32.saturating_sub(penalty as u32).max(5);
            let stale = age > prevent::STALE_AFTER_HOURS * 3600;
            let status = if stale {
                "insufficient_data"
            } else if warnings > 0 {
                "attention"
            } else if cautions > 0 || open_cases > 0 {
                "review"
            } else {
                "nominal"
            };
            let last = last_by.get(a.tail.as_str()).copied();
            out.push(AircraftView {
                tail: a.tail.clone(),
                type_code: a.type_code.clone(),
                operator: a.operator.clone(),
                lessor: a.lessor.clone(),
                base: a.base.clone(),
                msn: a.msn,
                health_score: if stale { 0 } else { score },
                status: status.into(),
                open_alerts: al.len(),
                warnings,
                cautions,
                open_cases,
                hypotheses: hyp_by.get(a.tail.as_str()).copied().unwrap_or(0),
                next_departure: dep_by.get(a.tail.as_str()).map(|d| (*d).clone()),
                preflight: report.map(|r| r.overall),
                data_age_seconds: age,
                cycles: last.map(|f| f.cycle).unwrap_or(0),
                rul_min_p10: rul_by.get(a.tail.as_str()).copied(),
                last_flight: last.map(|f| format!("{} {}→{}", f.number, f.origin, f.dest)),
            });
        }
        out.sort_by(|a, b| {
            a.health_score
                .cmp(&b.health_score)
                .then(a.tail.cmp(&b.tail))
        });
        out
    }

    /// Latest correction value for one field of one record (PSP-02).
    fn corrected(&self, record_type: &str, id: &str, field: &str) -> Option<&str> {
        self.decisions
            .corrections
            .iter()
            .rev()
            .find(|c| c.record_type == record_type && c.record_id == id && c.field == field)
            .map(|c| c.new_value.as_str())
    }

    fn corrected_event(&self, e: &FaultEvent) -> FaultEvent {
        let mut e = e.clone();
        if self.decisions.corrections.is_empty() {
            return e;
        }
        if let Some(v) = self.corrected("fault_event", &e.id, "message") {
            e.message = v.to_string();
        }
        if let Some(v) = self.corrected("fault_event", &e.id, "code") {
            e.code = v.to_string();
        }
        if let Some(v) = self
            .corrected("fault_event", &e.id, "ata")
            .and_then(|v| v.parse().ok())
        {
            e.ata = v;
            e.system = System::from_ata(v);
        }
        if let Some(v) = self.corrected("fault_event", &e.id, "severity") {
            if let Ok(s) = serde_json::from_value(serde_json::Value::String(v.into())) {
                e.severity = s;
            }
        }
        e
    }

    fn corrected_work_order(&self, w: &WorkOrder) -> WorkOrder {
        let mut w = w.clone();
        if let Some(v) = self.corrected("work_order", &w.id, "signed_off") {
            w.signed_off = v == "true";
        }
        if let Some(v) = self.corrected("work_order", &w.id, "note") {
            w.note = v.to_string();
        }
        w
    }

    /// DAT-03: remove one aircraft's operational records from memory.
    pub fn erase_tail(&mut self, tail: &str) {
        let f = Arc::make_mut(&mut self.full);
        f.aircraft.retain(|a| a.tail != tail);
        f.components.retain(|c| c.tail != tail);
        f.flights.retain(|x| x.tail != tail);
        f.events.retain(|x| x.tail != tail);
        f.work_orders.retain(|x| x.tail != tail);
        f.mel.retain(|x| x.tail != tail);
        f.samples.retain(|x| x.tail != tail);
        f.departures.retain(|x| x.tail != tail);
        f.flight_summaries.retain(|x| x.tail != tail);
        self.truth.retain(|t| t.tail != tail);
    }

    /// INT-01 ingestion path: append an externally received fault event.
    /// Idempotent by event id (regression: bug B7, duplicate deliveries inflated recurrence counts).
    pub fn ingest(&mut self, mut e: FaultEvent) -> bool {
        if self.full.events.iter().any(|x| x.id == e.id) {
            return false;
        }
        e.normalise();
        // O(log n) position + O(n) shift instead of a full re-sort per event.
        let pos = self.full.events.partition_point(|x| x.ts <= e.ts);
        Arc::make_mut(&mut self.full).events.insert(pos, e);
        true
    }
}
