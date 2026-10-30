//! Core domain model shared by every AIPS service.
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

pub type Ts = DateTime<Utc>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum System {
    Engine,
    Electrical,
    Hydraulic,
    Fuel,
    LandingGear,
    FlightControls,
    CabinPressure,
    Avionics,
    Other,
}

impl System {
    /// Map an ATA iSpec 2200 chapter to the AIPS system taxonomy.
    pub fn from_ata(ata: u8) -> Self {
        match ata {
            21 => System::CabinPressure,
            24 => System::Electrical,
            27 => System::FlightControls,
            28 => System::Fuel,
            29 => System::Hydraulic,
            32 => System::LandingGear,
            22 | 23 | 31 | 34 | 45 => System::Avionics,
            71..=80 => System::Engine,
            _ => System::Other,
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            System::Engine => "Engines",
            System::Electrical => "Electrical",
            System::Hydraulic => "Hydraulics",
            System::Fuel => "Fuel",
            System::LandingGear => "Landing gear",
            System::FlightControls => "Flight controls",
            System::CabinPressure => "Cabin pressure & environment",
            System::Avionics => "Avionics",
            System::Other => "Other",
        }
    }
    pub fn team(&self) -> &'static str {
        match self {
            System::Engine => "Powerplant Engineering",
            System::Electrical | System::Avionics => "Avionics & Electrical",
            System::Hydraulic | System::LandingGear => "Hydraulics & Landing Gear",
            System::Fuel => "Fuel Systems",
            System::FlightControls => "Flight Controls",
            System::CabinPressure | System::Other => "Structures & Cabin",
        }
    }
    pub const ALL: [System; 8] = [
        System::Engine,
        System::Electrical,
        System::Hydraulic,
        System::Fuel,
        System::LandingGear,
        System::FlightControls,
        System::CabinPressure,
        System::Avionics,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Advisory,
    Caution,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataSource {
    Acars,
    CentralMaintenanceComputer,
    TechLog,
    Qar,
    PilotReport,
    MaintenanceSystem,
}

impl DataSource {
    pub fn label(&self) -> &'static str {
        match self {
            DataSource::Acars => "ACARS",
            DataSource::CentralMaintenanceComputer => "Central maintenance computer",
            DataSource::TechLog => "Technical log",
            DataSource::Qar => "QAR / FDM",
            DataSource::PilotReport => "Pilot report",
            DataSource::MaintenanceSystem => "Maintenance information system",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Aircraft {
    pub tail: String,
    pub type_code: String,
    pub msn: u32,
    pub operator: String,
    pub lessor: Option<String>,
    pub engines: u8,
    pub delivered: NaiveDate,
    pub base: String,
    /// Time of the most recent data received from this aircraft's feeds (DAT-01, PRV-06).
    pub data_feed_last: Ts,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Component {
    pub id: String,
    pub tail: String,
    pub part_number: String,
    pub serial: String,
    pub ata: u8,
    pub position: String,
    pub description: String,
    pub installed_at: Ts,
    pub cycles_since_sv: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Flight {
    pub id: String,
    pub tail: String,
    pub number: String,
    pub origin: String,
    pub dest: String,
    pub dep: Ts,
    pub arr: Ts,
    /// Cumulative airframe cycle count after this flight.
    pub cycle: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaultEvent {
    pub id: String,
    pub tail: String,
    pub flight_id: Option<String>,
    pub ts: Ts,
    pub ata: u8,
    pub system: System,
    pub code: String,
    pub component_id: Option<String>,
    pub severity: Severity,
    pub message: String,
    pub source: DataSource,
    pub ingested_at: Ts,
}

impl FaultEvent {
    /// Canonical form used for both storage and analysis: the system follows the ATA chapter and
    /// ingestion is never before the event. Applied before persisting, so a restart re-analyses
    /// exactly what was analysed live.
    pub fn normalise(&mut self) {
        self.system = System::from_ata(self.ata);
        if self.ingested_at < self.ts {
            self.ingested_at = self.ts;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaintAction {
    Replaced,
    Adjusted,
    Tested,
    Cleaned,
    Reset,
    Deferred,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkOrder {
    pub id: String,
    pub tail: String,
    pub event_ids: Vec<String>,
    pub component_id: Option<String>,
    pub ata: u8,
    pub action: MaintAction,
    pub opened: Ts,
    pub closed: Option<Ts>,
    /// Certifying staff sign-off recorded in the maintenance system (WRK-02 documentation check).
    pub signed_off: bool,
    pub team: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MelItem {
    pub id: String,
    pub tail: String,
    pub reference: String,
    pub title: String,
    pub category: char,
    pub ata: u8,
    pub opened: Ts,
    pub due: Ts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Param {
    EgtMarginEng1,
    EgtMarginEng2,
    OilPressEng1,
    OilPressEng2,
    HydPressGreen,
    HydPressGreenAlt,
    HydPressBlue,
    BusVoltageAc1,
    FuelImbalance,
    CabinDiffPress,
}

pub struct ParamMeta {
    pub label: &'static str,
    pub unit: &'static str,
    pub lo: f64,
    pub hi: f64,
    pub ata: u8,
}

impl Param {
    pub const ALL: [Param; 10] = [
        Param::EgtMarginEng1,
        Param::EgtMarginEng2,
        Param::OilPressEng1,
        Param::OilPressEng2,
        Param::HydPressGreen,
        Param::HydPressGreenAlt,
        Param::HydPressBlue,
        Param::BusVoltageAc1,
        Param::FuelImbalance,
        Param::CabinDiffPress,
    ];
    pub fn meta(&self) -> ParamMeta {
        let m = |label, unit, lo, hi, ata| ParamMeta {
            label,
            unit,
            lo,
            hi,
            ata,
        };
        match self {
            Param::EgtMarginEng1 => m("EGT margin ENG 1", "°C", 3.0, 90.0, 72),
            Param::EgtMarginEng2 => m("EGT margin ENG 2", "°C", 3.0, 90.0, 72),
            Param::OilPressEng1 => m("Oil pressure ENG 1", "psi", 25.0, 95.0, 79),
            Param::OilPressEng2 => m("Oil pressure ENG 2", "psi", 25.0, 95.0, 79),
            Param::HydPressGreen => m(
                "Hydraulic pressure GREEN (sensor A)",
                "psi",
                2700.0,
                3300.0,
                29,
            ),
            Param::HydPressGreenAlt => m(
                "Hydraulic pressure GREEN (sensor B)",
                "psi",
                2700.0,
                3300.0,
                29,
            ),
            Param::HydPressBlue => m("Hydraulic pressure BLUE", "psi", 2700.0, 3300.0, 29),
            Param::BusVoltageAc1 => m("AC bus 1 voltage", "V", 108.0, 122.0, 24),
            Param::FuelImbalance => m("Fuel imbalance L/R", "kg", 0.0, 1500.0, 28),
            Param::CabinDiffPress => m("Cabin differential pressure", "psi", 0.0, 9.1, 21),
        }
    }
    pub fn system(&self) -> System {
        System::from_ata(self.meta().ata)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamSample {
    pub tail: String,
    pub flight_id: String,
    pub ts: Ts,
    pub param: Param,
    pub value: f64,
}

/// Next scheduled departure per aircraft, used by AIPS Prevent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledDeparture {
    pub flight_id: String,
    pub tail: String,
    pub number: String,
    pub origin: String,
    pub dest: String,
    pub std: Ts,
}

/// Complete operational dataset the intelligence core reasons over.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Dataset {
    pub generated_at: Option<Ts>,
    pub seed: u64,
    pub aircraft: Vec<Aircraft>,
    pub components: Vec<Component>,
    pub flights: Vec<Flight>,
    pub events: Vec<FaultEvent>,
    pub work_orders: Vec<WorkOrder>,
    pub mel: Vec<MelItem>,
    pub samples: Vec<ParamSample>,
    pub departures: Vec<ScheduledDeparture>,
    /// Per-flight trajectory summaries (AIPS Flight).
    #[serde(default)]
    pub flight_summaries: Vec<crate::flight::FlightSummary>,
}
