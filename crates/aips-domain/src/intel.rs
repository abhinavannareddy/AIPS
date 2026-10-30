//! Intelligence outputs: evidence-graded hypotheses, alerts, cases and RUL estimates.
use crate::model::{Severity, System, Ts};
use serde::{Deserialize, Serialize};

/// PRD principle P2 / CFM-03. Only an engineer may promote to EngineeringConfirmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceGrade {
    Rejected,
    Suspected,
    Corroborated,
    EngineeringConfirmed,
}

impl EvidenceGrade {
    pub fn label(&self) -> &'static str {
        match self {
            EvidenceGrade::Rejected => "Rejected",
            EvidenceGrade::Suspected => "Suspected",
            EvidenceGrade::Corroborated => "Corroborated",
            EvidenceGrade::EngineeringConfirmed => "Engineering-confirmed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HypothesisKind {
    /// Same fault on the same aircraft repeating within the look-back window (PRD-03, CFM-01).
    Recurrence,
    /// Fault recurs after a documented corrective action (PRD-04, CFM-02).
    RepeatAfterFix,
    /// Faults in different systems co-occur far more than the fleet base rate predicts (INT-04).
    CrossSystem,
}

/// Why two or more events were linked (CFM-04, NFR-11).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Explanation {
    pub matching_fields: Vec<String>,
    pub occurrences: u32,
    pub first_seen: Ts,
    pub last_seen: Ts,
    pub max_gap_hours: f64,
    pub median_gap_minutes: Option<f64>,
    pub lift: Option<f64>,
    pub expected_by_chance: Option<f64>,
    pub symptom_similarity: Option<f64>,
    pub fixes_between: u32,
    pub narrative: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hypothesis {
    pub id: String,
    pub tail: String,
    pub kind: HypothesisKind,
    pub systems: Vec<System>,
    pub codes: Vec<String>,
    pub event_ids: Vec<String>,
    pub work_order_ids: Vec<String>,
    /// Grade proposed by the engine.
    pub machine_grade: EvidenceGrade,
    /// Effective grade (machine grade unless a human decided otherwise).
    pub grade: EvidenceGrade,
    pub graded_by: Option<String>,
    pub graded_at: Option<Ts>,
    pub explanation: Explanation,
    pub title: String,
    pub model_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRef {
    pub source: String,
    pub data_age_seconds: i64,
    pub record_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Module {
    Integrity,
    Predict,
    Prevent,
    Flight,
    Guardian,
    Workflow,
}

/// INT-05: every alert shows source data, data age, model version and the reason it fired.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub id: String,
    pub tail: String,
    pub module: Module,
    pub system: System,
    pub severity: Severity,
    pub title: String,
    pub reason: String,
    pub sources: Vec<SourceRef>,
    pub model_version: String,
    pub evidence_grade: EvidenceGrade,
    pub hypothesis_id: Option<String>,
    pub created_at: Ts,
    pub feedback: Option<AlertFeedback>,
    /// WRK-03: who acknowledged the alert, and when.
    #[serde(default)]
    pub acknowledged: Option<AlertAck>,
    /// WRK-03: set when a Warning stayed unacknowledged past the escalation window.
    #[serde(default)]
    pub escalated_at: Option<Ts>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertAck {
    pub by: String,
    pub at: Ts,
}

/// PSP-02: an append-only, versioned correction to a source record. The original stays intact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Correction {
    pub id: String,
    pub version: u32,
    pub record_type: String,
    pub record_id: String,
    pub field: String,
    pub old_value: String,
    pub new_value: String,
    pub reason: String,
    pub by: String,
    pub at: Ts,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertFeedback {
    pub useful: bool,
    pub comment: String,
    pub by: String,
    pub at: Ts,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RulEstimate {
    pub tail: String,
    pub component_id: String,
    pub position: String,
    pub p10: f32,
    pub p50: f32,
    pub p90: f32,
    pub margin_c: f64,
    pub slope_per_100cyc: f64,
    pub cycles_since_sv: u32,
    pub model_version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaseStatus {
    Open,
    Assigned,
    InProgress,
    Escalated,
    Closed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseAction {
    pub at: Ts,
    pub actor: String,
    pub action: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Case {
    pub id: String,
    pub tail: String,
    pub title: String,
    pub kind: String,
    pub system: System,
    pub priority: Severity,
    pub status: CaseStatus,
    pub team: String,
    pub assignee: Option<String>,
    pub opened_at: Ts,
    pub sla_due: Ts,
    pub hypothesis_ids: Vec<String>,
    pub event_ids: Vec<String>,
    pub escalation_level: u8,
    pub history: Vec<CaseAction>,
}
