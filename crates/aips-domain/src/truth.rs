//! Ground truth for synthetic replay scenarios, used to measure detection quality
//! against the PRD section 4 targets (recall >= 90%, precision >= 70%).
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TruthKind {
    Recurrence,
    RepeatAfterFix,
    CrossSystem,
    Degradation,
    FrozenSensor,
    SensorDisagreement,
    TrendShift,
    StaleFeed,
    ConfigMismatch,
    MelOverdue,
    UnsignedWorkOrder,
    FlightUnstableApproach,
    FlightRapidDescent,
    FlightSpeedExceedance,
    FlightDataGap,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioTruth {
    pub id: String,
    pub kind: TruthKind,
    pub tail: String,
    pub codes: Vec<String>,
    pub params: Vec<String>,
    pub description: String,
}
