//! AIPS Guardian emergency workflow engine (GRD-01..GRD-09, EMG-01..EMG-07).
//!
//! Phase 0 runs TRAINING DRILLS ONLY. Notification channels are simulated sinks that never
//! transmit to real crew, ATC or emergency services. Live mode is refused (EMG-07, PRD 10).
use aips_domain::audit::AuditChain;
use chrono::{DateTime, Duration, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};

pub type Ts = DateTime<Utc>;

pub const QUALIFY_CONFIDENCE: f64 = 0.90;
pub const MIN_INDEPENDENT_CLASSES: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scenario {
    DualIncapacitation,
    SingleIncapacitation,
    CockpitSecurity,
    CabinFire,
    MedicalEmergency,
    FalseIndicator,
    /// EMG-04: indicators that disagree (some support, some contradict incapacitation).
    ConflictingIndicators,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndicatorClass {
    ControlInputs,
    CrewResponse,
    Physiological,
    CockpitDoor,
    CrewReport,
    AircraftSystem,
    CabinSensor,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Indicator {
    pub class: IndicatorClass,
    pub name: String,
    pub source: String,
    pub value: String,
    pub weight: f64,
    pub at: Ts,
    /// EMG-04: evidence against the hypothesis (e.g. the flight deck answered a call).
    #[serde(default)]
    pub contradicts: bool,
}

/// GRD-08: a procedure card shown to acknowledging crew. Loaded from configuration; the
/// demonstrator ships training examples only, clearly labelled as not operator procedures.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcedureCard {
    pub scenario: Scenario,
    pub title: String,
    pub steps: Vec<String>,
    pub source: String,
    pub training_example: bool,
}

/// AUT-03: message content for ATC and operations, composed from flight and weather context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtcMessage {
    pub text: String,
    pub destination: String,
    pub destination_metar: Option<String>,
    pub alternates: Vec<Alternate>,
    pub status: String,
    pub composed_at: Ts,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alternate {
    pub icao: String,
    pub name: String,
    pub metar: String,
}

/// Static alternate table for the demonstrator network (IATA codes as used in the replay data).
/// In service this comes from the operator's flight-planning system.
pub fn alternates_for(dest: &str) -> Vec<(&'static str, &'static str)> {
    match dest {
        "ARN" => vec![("GOT", "Göteborg Landvetter"), ("OSL", "Oslo Gardermoen")],
        "OSL" => vec![("BGO", "Bergen Flesland"), ("ARN", "Stockholm Arlanda")],
        "CPH" => vec![("BLL", "Billund"), ("GOT", "Göteborg Landvetter")],
        "HEL" => vec![("TMP", "Tampere-Pirkkala"), ("TLL", "Tallinn")],
        "GOT" => vec![("CPH", "Copenhagen Kastrup"), ("ARN", "Stockholm Arlanda")],
        "ZRH" => vec![("BSL", "EuroAirport Basel"), ("MUC", "Munich")],
        "LHR" => vec![("LGW", "London Gatwick"), ("STN", "London Stansted")],
        "AMS" => vec![("BRU", "Brussels"), ("DUS", "Düsseldorf")],
        "FRA" => vec![("CGN", "Cologne Bonn"), ("MUC", "Munich")],
        "CDG" => vec![("ORY", "Paris Orly"), ("BRU", "Brussels")],
        _ => vec![],
    }
}

/// Built-in training-example procedure cards, used when no configuration file is supplied.
pub fn default_procedures() -> Vec<ProcedureCard> {
    let card = |scenario, title: &str, steps: &[&str]| {
        ProcedureCard {
        scenario,
        title: title.into(),
        steps: steps.iter().map(|s| s.to_string()).collect(),
        source: "TRAINING EXAMPLE. Not an operator procedure. Replace with content from the approved operations manual.".into(),
        training_example: true,
    }
    };
    vec![
        card(
            Scenario::DualIncapacitation,
            "Flight crew incapacitation (example)",
            &[
                "Use the emergency access procedure in your operations manual.",
                "Confirm the condition of both pilots.",
                "Request any qualified pilot on board via the approved announcement.",
                "Contact the operations centre through the approved channel.",
            ],
        ),
        card(
            Scenario::SingleIncapacitation,
            "Single pilot incapacitation (example)",
            &[
                "Assist the remaining pilot as directed.",
                "Provide first aid to the incapacitated pilot.",
                "Secure the pilot away from the controls if instructed.",
            ],
        ),
        card(
            Scenario::CabinFire,
            "Cabin fire (example)",
            &[
                "Locate the source and inform the flight deck.",
                "Fight the fire using the nearest extinguisher.",
                "Move passengers away and prepare protective equipment.",
            ],
        ),
        card(
            Scenario::MedicalEmergency,
            "Medical emergency (example)",
            &[
                "Assess the passenger and start first aid.",
                "Bring the medical kit and defibrillator.",
                "Ask for a medical professional on board and inform the flight deck.",
            ],
        ),
        card(
            Scenario::CockpitSecurity,
            "Flight deck security concern (example)",
            &[
                "Follow the security procedure in your operations manual.",
                "Do not attempt to open the flight deck door outside approved procedures.",
            ],
        ),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Monitoring,
    Notifying,
    Acknowledged,
    Escalated,
    Cancelled,
    Resolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    CrewDevicePush,
    CabinInterphone,
    AirlineOpsMessaging,
    AtcDatalink,
}

impl Channel {
    pub fn label(&self) -> &'static str {
        match self {
            Channel::CrewDevicePush => "Crew device push",
            Channel::CabinInterphone => "Cabin interphone chime",
            Channel::AirlineOpsMessaging => "Airline operations messaging",
            Channel::AtcDatalink => "ATC via approved aircraft communication system",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatus {
    Pending,
    Delivered,
    Failed,
    /// Training mode: recorded, intentionally not transmitted.
    NotTransmitted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delivery {
    pub channel: Channel,
    pub recipient: String,
    pub sent_at: Ts,
    pub due_at: Ts,
    pub status: DeliveryStatus,
    pub delivered_at: Option<Ts>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrewMember {
    pub id: String,
    pub name: String,
    pub position: String,
    pub designated: bool,
    pub notified: bool,
    pub acknowledged_at: Option<Ts>,
    pub available: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Done,
    Skipped,
    NotAvailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    pub stage: u8,
    pub name: String,
    pub status: StepStatus,
    pub at: Option<Ts>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Incident {
    pub id: String,
    pub training: bool,
    pub tail: String,
    pub type_code: String,
    pub flight: String,
    pub scenario: Scenario,
    pub created_by: String,
    pub created_at: Ts,
    pub state: State,
    pub confidence: f64,
    pub independent_classes: usize,
    pub qualified: bool,
    pub indicators: Vec<Indicator>,
    pub crew: Vec<CrewMember>,
    pub deliveries: Vec<Delivery>,
    pub steps: Vec<Step>,
    pub escalation_level: u8,
    pub ack_deadline: Option<Ts>,
    pub feed_lost: bool,
    pub last_data_at: Ts,
    pub failed_channel: Option<Channel>,
    pub closed_by: Option<String>,
    pub closed_reason: Option<String>,
    pub journal: AuditChain,
    #[serde(default)]
    pub destination: String,
    #[serde(default)]
    pub disagreement: Option<String>,
    #[serde(default)]
    pub procedure: Option<ProcedureCard>,
    #[serde(default)]
    pub atc_message: Option<AtcMessage>,
}

/// Aircraft-type registry for certified emergency landing capability (AUT-01).
/// No type in the demonstrator fleet has a certified emergency autoland; AIPS never invents one.
pub fn autoland_capability(type_code: &str) -> Option<&'static str> {
    match type_code {
        // Example of a supported entry once an OEM integration exists (none in Phase 0).
        "DEMO-CERTIFIED-TYPE" => {
            Some("Certified emergency autoland (aircraft-specific, OEM-approved interface)")
        }
        _ => None,
    }
}

pub struct Timers {
    pub ack_timeout: Duration,
    pub second_escalation: Duration,
}

impl Default for Timers {
    fn default() -> Self {
        Self {
            ack_timeout: Duration::seconds(20),
            second_escalation: Duration::seconds(25),
        }
    }
}

fn indicators_for(s: Scenario, now: Ts) -> Vec<Indicator> {
    let ind = |class, name: &str, source: &str, value: &str, weight| Indicator {
        class,
        name: name.into(),
        source: source.into(),
        value: value.into(),
        weight,
        at: now,
        contradicts: false,
    };
    use IndicatorClass::*;
    match s {
        Scenario::DualIncapacitation => vec![
            ind(
                ControlInputs,
                "No flight-deck control or switch activity",
                "Aircraft data bus (read-only)",
                "0 inputs in 240 s",
                0.55,
            ),
            ind(
                CrewResponse,
                "No reply to cabin interphone call",
                "Cabin interphone",
                "2 calls unanswered",
                0.6,
            ),
            ind(
                CockpitDoor,
                "Cockpit door alert",
                "Door surveillance system (read-only)",
                "Emergency access request denied",
                0.5,
            ),
            ind(
                CrewReport,
                "Cabin crew report: noise from flight deck",
                "Crew device",
                "Report from L1",
                0.45,
            ),
        ],
        Scenario::SingleIncapacitation => vec![
            ind(
                Physiological,
                "Approved pilot alertness monitor",
                "Physiological monitor (operator-approved)",
                "No response to prompt",
                0.5,
            ),
            ind(
                CrewReport,
                "Pilot monitoring reports incapacitation",
                "Flight deck interphone",
                "PM call to purser",
                0.85,
            ),
        ],
        Scenario::CockpitSecurity => vec![
            ind(
                CockpitDoor,
                "Repeated unauthorised access attempts",
                "Door surveillance system (read-only)",
                "4 attempts in 60 s",
                0.7,
            ),
            ind(
                CrewReport,
                "Cabin crew report: disruptive passenger at forward galley",
                "Crew device",
                "Report from L2",
                0.75,
            ),
        ],
        Scenario::CabinFire => vec![
            ind(
                CabinSensor,
                "Lavatory smoke detector",
                "Smoke detection system",
                "AFT LAV SMOKE",
                0.7,
            ),
            ind(
                CrewReport,
                "Cabin crew report: visible smoke aft",
                "Crew device",
                "Report from R2",
                0.75,
            ),
        ],
        Scenario::MedicalEmergency => vec![
            ind(
                CrewReport,
                "Passenger unresponsive, row 23",
                "Crew device",
                "Report from R1",
                0.85,
            ),
            ind(
                CrewResponse,
                "Medical kit requested",
                "Crew device",
                "AED requested",
                0.4,
            ),
        ],
        Scenario::ConflictingIndicators => vec![
            ind(
                ControlInputs,
                "No flight-deck control or switch activity",
                "Aircraft data bus (read-only)",
                "0 inputs in 240 s",
                0.55,
            ),
            ind(
                Physiological,
                "Seat sensor: no movement (captain)",
                "Seat sensor (demo)",
                "No movement for 200 s",
                0.6,
            ),
            Indicator {
                contradicts: true,
                ..ind(
                    CrewResponse,
                    "Flight deck answered cabin interphone call",
                    "Cabin interphone",
                    "Answered after 4 s",
                    0.8,
                )
            },
        ],
        Scenario::FalseIndicator => vec![ind(
            ControlInputs,
            "Low flight-deck activity (cruise, autopilot engaged)",
            "Aircraft data bus (read-only)",
            "Below baseline for 180 s",
            0.55,
        )],
    }
}

fn roster(now: Ts) -> Vec<CrewMember> {
    let _ = now;
    [
        ("CC1", "M. Andersson", "Purser (L1)", true),
        ("CC2", "S. Nyberg", "L2", true),
        ("CC3", "J. Haugen", "R1", false),
        ("CC4", "E. Virtanen", "R2", false),
        ("CC5", "K. Holm", "Aft galley", false),
    ]
    .into_iter()
    .map(|(id, name, pos, designated)| CrewMember {
        id: id.into(),
        name: name.into(),
        position: pos.into(),
        designated,
        notified: false,
        acknowledged_at: None,
        available: None,
    })
    .collect()
}

fn steps(type_code: &str) -> Vec<Step> {
    let names = [
        "Detection",
        "Activation",
        "Cabin notification",
        "Crew availability",
        "ATC notification",
        "Ground coordination",
        "Emergency landing",
        "Landing coordination",
        "Post-landing",
    ];
    names
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let (status, detail) = if i == 6 {
                match autoland_capability(type_code) {
                    Some(cap) => (StepStatus::Pending, format!("{cap}: forwarding only; the certified system decides activation.")),
                    None => (
                        StepStatus::NotAvailable,
                        format!("No certified emergency landing capability installed on {type_code}. AIPS does not control the aircraft; standard emergency procedures apply."),
                    ),
                }
            } else {
                (StepStatus::Pending, String::new())
            };
            Step { stage: (i + 1) as u8, name: n.to_string(), status, at: None, detail }
        })
        .collect()
}

/// Noisy-OR fusion of independent indicators (GRD-01, GRD-02).
/// Contradicting indicators scale the fused confidence down by (1 - weight) and do not count
/// as independent supporting classes (EMG-04).
pub fn fuse(ind: &[Indicator]) -> (f64, usize) {
    let mut classes: Vec<IndicatorClass> = ind
        .iter()
        .filter(|i| !i.contradicts)
        .map(|i| i.class)
        .collect();
    classes.sort_by_key(|c| *c as u8);
    classes.dedup();
    let support = 1.0
        - ind
            .iter()
            .filter(|i| !i.contradicts)
            .fold(1.0, |acc, i| acc * (1.0 - i.weight.clamp(0.0, 0.99)));
    let against = ind
        .iter()
        .filter(|i| i.contradicts)
        .fold(1.0, |acc, i| acc * (1.0 - i.weight.clamp(0.0, 0.99)));
    (support * against, classes.len())
}

/// EMG-04: an explicit statement when supporting and contradicting indicators coexist.
pub fn disagreement(ind: &[Indicator]) -> Option<String> {
    let against: Vec<&str> = ind
        .iter()
        .filter(|i| i.contradicts)
        .map(|i| i.name.as_str())
        .collect();
    let support = ind.iter().filter(|i| !i.contradicts).count();
    if against.is_empty() || support == 0 {
        return None;
    }
    Some(format!(
        "Indicators disagree: {support} support the hypothesis, but {} contradict it ({}). Human verification is required; AIPS does not resolve the conflict automatically.",
        against.len(),
        against.join("; ")
    ))
}

impl Incident {
    pub fn new(
        id: String,
        tail: String,
        type_code: String,
        flight: String,
        scenario: Scenario,
        by: &str,
    ) -> Self {
        let now = Utc::now();
        let indicators = indicators_for(scenario, now);
        let (confidence, independent_classes) = fuse(&indicators);
        let disagreement = disagreement(&indicators);
        let qualified = confidence >= QUALIFY_CONFIDENCE
            && independent_classes >= MIN_INDEPENDENT_CLASSES
            && disagreement.is_none();
        let mut inc = Incident {
            id,
            training: true,
            tail,
            steps: steps(&type_code),
            type_code,
            flight,
            scenario,
            created_by: by.into(),
            created_at: now,
            state: State::Monitoring,
            confidence,
            independent_classes,
            qualified,
            indicators,
            crew: roster(now),
            deliveries: vec![],
            escalation_level: 0,
            ack_deadline: None,
            feed_lost: false,
            last_data_at: now,
            failed_channel: None,
            closed_by: None,
            closed_reason: None,
            journal: AuditChain::default(),
            destination: String::new(),
            disagreement: disagreement.clone(),
            procedure: None,
            atc_message: None,
        };
        inc.log(
            by,
            "drill.created",
            serde_json::json!({ "scenario": scenario, "training": true }),
        );
        inc.log("guardian", "detection.evaluated", serde_json::json!({
            "confidence": confidence, "independent_classes": independent_classes, "qualified": qualified,
            "rule": format!("confidence >= {QUALIFY_CONFIDENCE} and >= {MIN_INDEPENDENT_CLASSES} independent indicator classes"),
        }));
        if let Some(d) = &disagreement {
            inc.log(
                "guardian",
                "disagreement.flagged",
                serde_json::json!({ "detail": d }),
            );
        }
        inc.step_done(
            1,
            &format!(
                "{} indicator(s), {} independent class(es), confidence {:.0}%",
                inc.indicators.len(),
                independent_classes,
                confidence * 100.0
            ),
        );
        inc
    }

    /// AUT-03: compose the ATC/operations message. In training it is logged and never transmitted.
    pub fn compose_atc_message(&mut self) {
        let now = Utc::now();
        let dest = self.destination.clone();
        let metar = (!dest.is_empty()).then(|| aips_domain::flight::weather(&dest, now).metar);
        let alternates: Vec<Alternate> = alternates_for(&dest)
            .into_iter()
            .map(|(icao, name)| Alternate {
                icao: icao.into(),
                name: name.into(),
                metar: aips_domain::flight::weather(icao, now).metar,
            })
            .collect();
        let what = match self.scenario {
            Scenario::DualIncapacitation => "suspected flight crew incapacitation",
            Scenario::SingleIncapacitation => "suspected single pilot incapacitation",
            Scenario::CockpitSecurity => "flight deck security concern",
            Scenario::CabinFire => "cabin fire reported",
            Scenario::MedicalEmergency => "medical emergency on board",
            _ => "unverified emergency indication",
        };
        let text = format!(
            "{}{} {} ({}): {}. Destination {}{}. Alternates: {}. Crew response: {}.",
            if self.training {
                "TRAINING DRILL - NOT A REAL EMERGENCY. "
            } else {
                ""
            },
            self.flight,
            self.tail,
            self.type_code,
            what,
            if dest.is_empty() { "unknown" } else { &dest },
            metar
                .as_ref()
                .map(|m| format!(" ({m})"))
                .unwrap_or_default(),
            if alternates.is_empty() {
                "none on file".to_string()
            } else {
                alternates
                    .iter()
                    .map(|a| a.icao.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            },
            if self.crew.iter().any(|c| c.available == Some(true)) {
                "crew member available"
            } else {
                "no available crew acknowledgement"
            },
        );
        let msg = AtcMessage {
            text,
            destination: dest,
            destination_metar: metar,
            alternates,
            status: if self.training {
                "logged; NOT transmitted (training)".into()
            } else {
                "handed to approved communication system".into()
            },
            composed_at: now,
        };
        self.log("guardian", "atc.message_composed", serde_json::json!(msg));
        self.atc_message = Some(msg);
    }

    pub fn log(&mut self, actor: &str, action: &str, detail: serde_json::Value) {
        let subject = self.id.clone();
        self.journal.append(actor, action, &subject, detail);
    }

    fn step_done(&mut self, stage: u8, detail: &str) {
        if let Some(s) = self.steps.iter_mut().find(|s| s.stage == stage) {
            if s.status == StepStatus::Pending {
                s.status = StepStatus::Done;
                s.at = Some(Utc::now());
                s.detail = detail.into();
            }
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(
            self.state,
            State::Monitoring | State::Notifying | State::Acknowledged | State::Escalated
        )
    }

    fn send(&mut self, channel: Channel, recipient: &str, rng: &mut impl Rng) {
        let now = Utc::now();
        let status = if channel == Channel::AtcDatalink && self.training {
            DeliveryStatus::NotTransmitted
        } else {
            DeliveryStatus::Pending
        };
        let latency = Duration::milliseconds(rng.gen_range(300..1800));
        self.deliveries.push(Delivery {
            channel,
            recipient: recipient.into(),
            sent_at: now,
            due_at: now + latency,
            status,
            delivered_at: None,
        });
    }

    /// Start the notification workflow for a qualified detection (GRD-06).
    pub fn activate(&mut self, timers: &Timers, rng: &mut impl Rng) {
        if !self.qualified {
            let reason = if self.disagreement.is_some() {
                "indicators disagree; human verification required before any notification"
            } else {
                "detection did not qualify; monitoring continues, no crew notification"
            };
            self.log(
                "guardian",
                "activation.withheld",
                serde_json::json!({ "reason": reason }),
            );
            return;
        }
        self.state = State::Notifying;
        self.step_done(2, "Approved emergency workflow started (training drill)");
        let designated: Vec<String> = self
            .crew
            .iter()
            .filter(|c| c.designated)
            .map(|c| c.id.clone())
            .collect();
        for id in &designated {
            self.send(Channel::CrewDevicePush, id, rng);
            self.send(Channel::CabinInterphone, id, rng);
            if let Some(c) = self.crew.iter_mut().find(|c| &c.id == id) {
                c.notified = true;
            }
        }
        self.ack_deadline = Some(Utc::now() + timers.ack_timeout);
        self.step_done(
            3,
            &format!(
                "Designated crew notified on two independent channels: {}",
                designated.join(", ")
            ),
        );
        self.log("guardian", "crew.notified", serde_json::json!({ "recipients": designated, "channels": [Channel::CrewDevicePush.label(), Channel::CabinInterphone.label()] }));
    }

    /// `proxy` is true when a ground role relays the response: it is journalled, but it does not
    /// count as the crew member confirming availability and does not stop escalation (GRD-07).
    pub fn acknowledge(
        &mut self,
        crew_id: &str,
        available: bool,
        by: &str,
        proxy: bool,
    ) -> Result<(), String> {
        if !self.is_active() {
            return Err("incident is closed".into());
        }
        let c = self
            .crew
            .iter_mut()
            .find(|c| c.id == crew_id)
            .ok_or("unknown crew member")?;
        if !c.notified {
            return Err("crew member has not been notified".into());
        }
        if proxy {
            self.log(
                by,
                "crew.acknowledged_by_proxy",
                serde_json::json!({ "crew": crew_id, "available": available }),
            );
            return Ok(());
        }
        c.acknowledged_at = Some(Utc::now());
        c.available = Some(available);
        let name = c.name.clone();
        self.log(
            by,
            "crew.acknowledged",
            serde_json::json!({ "crew": crew_id, "available": available }),
        );
        // GRD-07: only an acknowledgement that confirms availability counts as a response.
        if available && matches!(self.state, State::Notifying | State::Escalated) {
            self.state = State::Acknowledged;
            self.step_done(
                4,
                &format!("{name} ({crew_id}) acknowledged and confirmed available"),
            );
        }
        Ok(())
    }

    fn escalate(&mut self, level: u8, rng: &mut impl Rng) {
        self.escalation_level = level;
        if level == 1 {
            let rest: Vec<String> = self
                .crew
                .iter()
                .filter(|c| !c.notified)
                .map(|c| c.id.clone())
                .collect();
            for id in &rest {
                self.send(Channel::CrewDevicePush, id, rng);
                self.send(Channel::CabinInterphone, id, rng);
                if let Some(c) = self.crew.iter_mut().find(|c| &c.id == id) {
                    c.notified = true;
                }
            }
            self.send(
                Channel::AirlineOpsMessaging,
                "Operations control centre",
                rng,
            );
            if self.state != State::Acknowledged {
                self.state = State::Escalated;
            }
            self.step_done(6, "Airline operations control notified");
            self.log("guardian", "escalated", serde_json::json!({ "level": 1, "reason": "no available crew acknowledgement before deadline", "added_recipients": rest }));
        } else {
            self.compose_atc_message();
            self.send(Channel::AirlineOpsMessaging, "Duty operations manager", rng);
            self.send(
                Channel::AtcDatalink,
                "ATC (approved aircraft communication system)",
                rng,
            );
            self.step_done(
                5,
                if self.training {
                    "ATC message prepared and logged; NOT transmitted (training)"
                } else {
                    "ATC notified"
                },
            );
            self.log("guardian", "escalated", serde_json::json!({ "level": 2, "atc": if self.training { "not transmitted (training)" } else { "sent" } }));
        }
    }

    /// Advance timers and simulated channel deliveries. Returns true when state changed.
    pub fn tick(&mut self, timers: &Timers, rng: &mut impl Rng) -> bool {
        if !self.is_active() {
            return false;
        }
        let now = Utc::now();
        let mut changed = false;
        let failed = self.failed_channel;
        let mut log_items = Vec::new();
        for d in self
            .deliveries
            .iter_mut()
            .filter(|d| d.status == DeliveryStatus::Pending && d.due_at <= now)
        {
            if Some(d.channel) == failed {
                d.status = DeliveryStatus::Failed;
                log_items.push((
                    "channel.failed",
                    serde_json::json!({ "channel": d.channel, "recipient": d.recipient }),
                ));
            } else {
                d.status = DeliveryStatus::Delivered;
                d.delivered_at = Some(now);
            }
            changed = true;
        }
        for (a, v) in log_items {
            self.log("guardian", a, v);
        }
        if let Some(deadline) = self.ack_deadline {
            let responded = self.crew.iter().any(|c| c.available == Some(true));
            if !responded && self.escalation_level == 0 && now >= deadline {
                self.escalate(1, rng);
                changed = true;
            } else if !responded
                && self.escalation_level == 1
                && now >= deadline + timers.second_escalation
            {
                self.escalate(2, rng);
                changed = true;
            }
        }
        changed
    }

    pub fn set_feed(&mut self, lost: bool, by: &str) {
        if lost && !self.feed_lost {
            self.last_data_at = Utc::now();
        }
        self.feed_lost = lost;
        // EMG-03: losing data never clears an active emergency.
        self.log(
            by,
            if lost { "data.lost" } else { "data.restored" },
            serde_json::json!({ "state_preserved": format!("{:?}", self.state) }),
        );
    }

    pub fn close(&mut self, by: &str, reason: &str, cancelled: bool) -> Result<(), String> {
        if !self.is_active() {
            return Err("incident already closed".into());
        }
        if reason.trim().len() < 5 {
            return Err("a reason is required".into());
        }
        self.state = if cancelled {
            State::Cancelled
        } else {
            State::Resolved
        };
        self.closed_by = Some(by.into());
        self.closed_reason = Some(reason.into());
        if !cancelled {
            self.step_done(8, "Coordination complete");
            self.step_done(
                9,
                "Incident record preserved; emergency services notification logged (training)",
            );
        }
        self.log(
            by,
            if cancelled {
                "incident.cancelled"
            } else {
                "incident.resolved"
            },
            serde_json::json!({ "reason": reason }),
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    fn rng() -> rand::rngs::StdRng {
        rand::rngs::StdRng::seed_from_u64(1)
    }

    #[test]
    fn single_indicator_never_notifies() {
        let mut i = Incident::new(
            "X".into(),
            "SE-T".into(),
            "A320neo".into(),
            "NA1".into(),
            Scenario::FalseIndicator,
            "trainer",
        );
        assert!(!i.qualified);
        i.activate(&Timers::default(), &mut rng());
        assert_eq!(i.state, State::Monitoring);
        assert!(i.deliveries.is_empty());
    }

    #[test]
    fn dual_incapacitation_notifies_two_channels_and_escalates() {
        let mut r = rng();
        let t = Timers {
            ack_timeout: Duration::milliseconds(0),
            second_escalation: Duration::milliseconds(0),
        };
        let mut i = Incident::new(
            "X".into(),
            "SE-T".into(),
            "A320neo".into(),
            "NA1".into(),
            Scenario::DualIncapacitation,
            "trainer",
        );
        assert!(i.qualified && i.independent_classes >= 2);
        i.activate(&t, &mut r);
        let chans: std::collections::HashSet<_> = i.deliveries.iter().map(|d| d.channel).collect();
        assert!(
            chans.contains(&Channel::CrewDevicePush) && chans.contains(&Channel::CabinInterphone)
        );
        assert!(i.tick(&t, &mut r));
        assert_eq!(i.escalation_level, 1);
        i.tick(&t, &mut r);
        assert_eq!(i.escalation_level, 2);
        let atc = i
            .deliveries
            .iter()
            .find(|d| d.channel == Channel::AtcDatalink)
            .unwrap();
        assert_eq!(atc.status, DeliveryStatus::NotTransmitted);
        assert_eq!(i.steps[6].status, StepStatus::NotAvailable);
        assert!(i.journal.verify().is_none());
    }

    #[test]
    fn unavailable_ack_does_not_count_and_feed_loss_keeps_state() {
        let mut r = rng();
        let mut i = Incident::new(
            "X".into(),
            "SE-T".into(),
            "A320neo".into(),
            "NA1".into(),
            Scenario::CabinFire,
            "trainer",
        );
        i.activate(&Timers::default(), &mut r);
        i.acknowledge("CC1", false, "cc1", false).unwrap();
        assert_eq!(i.state, State::Notifying);
        i.set_feed(true, "trainer");
        assert_eq!(i.state, State::Notifying);
        i.acknowledge("CC2", true, "cc2", false).unwrap();
        assert_eq!(i.state, State::Acknowledged);
        assert!(i.close("pilot", "x", true).is_err());
        i.close("pilot", "False alarm confirmed by flight deck", true)
            .unwrap();
        assert_eq!(i.state, State::Cancelled);
        assert!(i.acknowledge("CC3", true, "cc3", false).is_err());
    }

    /// Regression (GRD-07): a ground role answering "available" for a crew member is journalled
    /// but does not confirm availability or stop escalation.
    #[test]
    fn proxy_ack_is_journalled_but_does_not_confirm_availability() {
        let mut r = rng();
        let mut i = Incident::new(
            "P".into(),
            "SE-T".into(),
            "A320neo".into(),
            "NA1".into(),
            Scenario::CabinFire,
            "trainer",
        );
        i.activate(&Timers::default(), &mut r);
        i.acknowledge("CC1", true, "ops.user", true).unwrap();
        assert_eq!(i.state, State::Notifying);
        assert!(i.crew.iter().all(|c| c.acknowledged_at.is_none()));
        assert!(i
            .journal
            .entries
            .iter()
            .any(|e| e.action == "crew.acknowledged_by_proxy"));
        i.acknowledge("CC1", true, "cc1", false).unwrap();
        assert_eq!(i.state, State::Acknowledged);
    }
}

#[cfg(test)]
mod phase2_tests {
    use super::*;

    #[test]
    fn conflicting_indicators_are_flagged_and_never_notify() {
        let mut i = Incident::new(
            "T1".into(),
            "SE-RAA".into(),
            "A320neo".into(),
            "NA1".into(),
            Scenario::ConflictingIndicators,
            "t",
        );
        assert!(i.disagreement.as_deref().unwrap().contains("disagree"));
        assert!(!i.qualified, "a conflict must never qualify on its own");
        i.activate(&Timers::default(), &mut rand::thread_rng());
        assert!(
            i.deliveries.is_empty(),
            "no crew notified while indicators disagree"
        );
        assert!(i
            .journal
            .entries
            .iter()
            .any(|e| e.action == "disagreement.flagged"));
        // Without the contradicting indicator the same evidence would be a weaker, unconflicted reading.
        let (with, _) = fuse(&i.indicators);
        let supporting: Vec<Indicator> = i
            .indicators
            .iter()
            .filter(|x| !x.contradicts)
            .cloned()
            .collect();
        let (without, _) = fuse(&supporting);
        assert!(with < without, "contradiction lowers confidence");
    }

    #[test]
    fn atc_message_has_destination_weather_and_alternates_and_is_not_transmitted() {
        let mut i = Incident::new(
            "T2".into(),
            "SE-RAA".into(),
            "A320neo".into(),
            "NA1028".into(),
            Scenario::DualIncapacitation,
            "t",
        );
        i.destination = "ZRH".into();
        i.compose_atc_message();
        let m = i.atc_message.unwrap();
        assert!(m.text.starts_with("TRAINING DRILL"));
        assert!(m.destination_metar.unwrap().starts_with("ZRH"));
        assert_eq!(m.alternates.len(), 2);
        assert!(m.status.contains("NOT transmitted"));
    }

    #[test]
    fn default_procedure_cards_are_labelled_training_examples() {
        let cards = default_procedures();
        assert!(cards.len() >= 4);
        assert!(cards
            .iter()
            .all(|c| c.training_example && c.source.starts_with("TRAINING EXAMPLE")));
    }
}
