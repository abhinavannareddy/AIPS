//! AIPS Flight: flight safety intelligence (FLT-01..FLT-04).
//!
//! Works on per-flight trajectory summaries (ADS-B / QAR derived): speed, altitude and vertical
//! speed checks, data-gap and delayed-data handling, weather and operational context, and
//! correlation with aircraft-system events. It reports observable facts only; it never infers
//! intent (FLT-04).
use crate::intel::{EvidenceGrade, Module};
use crate::model::{FaultEvent, Severity, System, Ts};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

pub const MODEL_VERSION: &str = "flight-rules-1.0.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackSource {
    AdsB,
    Qar,
}

/// Per-flight trajectory summary. Detailed tracks are reconstructed on demand (see `track`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightSummary {
    pub flight_id: String,
    pub tail: String,
    pub source: TrackSource,
    pub cruise_alt_ft: f64,
    pub max_alt_ft: f64,
    pub cruise_speed_kt: f64,
    pub max_speed_below_10k_kt: f64,
    pub max_climb_fpm: f64,
    pub max_descent_fpm: f64,
    /// Highest rate of descent below 1,000 ft above the runway (approach stability).
    pub max_descent_below_1000_fpm: f64,
    /// Largest unexplained altitude change while in cruise.
    pub cruise_alt_deviation_ft: f64,
    pub samples: u32,
    pub max_gap_s: u32,
    /// When the summary was received by AIPS (delayed-data handling).
    pub received_at: Ts,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Weather {
    pub airport: String,
    pub observed: Ts,
    pub wind_dir: u16,
    pub wind_kt: u16,
    pub gust_kt: Option<u16>,
    pub visibility_m: u32,
    pub ceiling_ft: Option<u32>,
    pub convective: bool,
    pub metar: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notam {
    pub id: String,
    pub airport: String,
    pub text: String,
    pub from: Ts,
    pub to: Ts,
}

fn h(parts: &[&str]) -> u64 {
    let mut s = Sha256::new();
    for p in parts {
        s.update(p.as_bytes());
        s.update([0u8]);
    }
    let d = s.finalize();
    u64::from_be_bytes(d[..8].try_into().unwrap())
}

/// Deterministic synthetic METAR for an airport and hour (Phase 0 operational context).
pub fn weather(airport: &str, at: Ts) -> Weather {
    let hour = at.format("%Y%m%d%H").to_string();
    let r = h(&[airport, &hour]);
    let wind_dir = ((r % 36) * 10) as u16;
    let wind_kt = (3 + (r >> 8) % 22) as u16;
    let gust_kt = (r >> 16)
        .is_multiple_of(5)
        .then(|| wind_kt + 8 + ((r >> 20) % 12) as u16);
    let visibility_m = if (r >> 24).is_multiple_of(9) {
        1200 + ((r >> 28) % 30) as u32 * 100
    } else {
        9999
    };
    let ceiling_ft = (r >> 32)
        .is_multiple_of(3)
        .then(|| 300 + ((r >> 36) % 40) as u32 * 100);
    let convective = (r >> 44).is_multiple_of(23);
    let obs = at.format("%d%H00Z").to_string();
    let wind = match gust_kt {
        Some(g) => format!("{wind_dir:03}{wind_kt:02}G{g:02}KT"),
        None => format!("{wind_dir:03}{wind_kt:02}KT"),
    };
    let vis = if visibility_m >= 9999 {
        "9999".to_string()
    } else {
        format!("{visibility_m:04}")
    };
    let cloud = match ceiling_ft {
        Some(c) if convective => format!("BKN{:03}CB", c / 100),
        Some(c) => format!("BKN{:03}", c / 100),
        None => "FEW040".to_string(),
    };
    let wx = if convective { " TSRA" } else { "" };
    Weather {
        airport: airport.into(),
        observed: at,
        wind_dir,
        wind_kt,
        gust_kt,
        visibility_m,
        ceiling_ft,
        convective,
        metar: format!("{airport} {obs} {wind} {vis}{wx} {cloud}"),
    }
}

/// Deterministic synthetic NOTAMs active at an airport around a time (Phase 0 context).
pub fn notams(airport: &str, at: Ts) -> Vec<Notam> {
    let day = at.format("%Y%m%d").to_string();
    let r = h(&[airport, &day, "notam"]);
    let mut out = Vec::new();
    let start = at.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc();
    let end = start + chrono::Duration::days(1);
    let texts = [
        "ILS RWY 26 GP U/S",
        "TWY B CLSD BTN B3 AND B5",
        "RWY 08/26 CLSD 0100-0400 FOR MAINT",
        "CRANE 320 FT AMSL 1.2 NM FINAL RWY 19",
        "VOR U/S",
    ];
    if r.is_multiple_of(3) {
        out.push(Notam {
            id: format!("A{:04}/26", r % 9999),
            airport: airport.into(),
            text: texts[(r >> 8) as usize % texts.len()].into(),
            from: start,
            to: end,
        });
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlightEventKind {
    UnstableApproach,
    RapidDescent,
    SpeedExceedanceBelow10k,
    CruiseAltitudeDeviation,
    WeakClimbPerformance,
    TrackDataGap,
    DelayedData,
}

impl FlightEventKind {
    pub fn label(&self) -> &'static str {
        match self {
            FlightEventKind::UnstableApproach => "High descent rate below 1,000 ft",
            FlightEventKind::RapidDescent => "Rapid descent",
            FlightEventKind::SpeedExceedanceBelow10k => "Speed above 250 kt below 10,000 ft",
            FlightEventKind::CruiseAltitudeDeviation => "Altitude deviation in cruise",
            FlightEventKind::WeakClimbPerformance => "Weak climb performance",
            FlightEventKind::TrackDataGap => "Incomplete trajectory data",
            FlightEventKind::DelayedData => "Delayed flight data",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightEvent {
    pub id: String,
    pub flight_id: String,
    pub tail: String,
    pub number: String,
    pub origin: String,
    pub dest: String,
    pub kind: FlightEventKind,
    pub severity: Severity,
    pub at: Ts,
    pub observed: String,
    pub threshold: String,
    pub source: TrackSource,
    pub data_age_seconds: i64,
    pub weather: Option<Weather>,
    pub weather_note: Option<String>,
    pub correlated_events: Vec<String>,
    pub correlation_note: Option<String>,
    pub evidence_grade: EvidenceGrade,
    pub module: Module,
    pub model_version: String,
    /// FLT-04: what this event does not mean.
    pub limitation: String,
}

pub const LIMITATION: &str = "Observable flight-data fact only. It does not establish intent or cause; authorised personnel assess it with crew reports and flight data monitoring procedures.";

pub struct FlightInput<'a> {
    pub flight_id: &'a str,
    pub tail: &'a str,
    pub number: &'a str,
    pub origin: &'a str,
    pub dest: &'a str,
    pub dep: Ts,
    pub arr: Ts,
    pub wide: bool,
}

/// Apply the flight rules to one flight summary (FLT-01, FLT-02, FLT-03).
pub fn analyse(
    f: &FlightInput,
    s: &FlightSummary,
    faults: &[&FaultEvent],
    now: Ts,
) -> Vec<FlightEvent> {
    let mut out = Vec::new();
    let mut push = |kind: FlightEventKind,
                    sev: Severity,
                    at: Ts,
                    observed: String,
                    threshold: String,
                    airport: Option<&str>| {
        let wx = airport.map(|a| weather(a, at));
        let weather_note = wx.as_ref().and_then(|w| {
            let mut notes = Vec::new();
            if let Some(g) = w.gust_kt {
                notes.push(format!("gusts {g} kt"));
            }
            if w.convective {
                notes.push("convective weather".into());
            }
            if w.visibility_m < 5000 {
                notes.push(format!("visibility {} m", w.visibility_m));
            }
            (!notes.is_empty()).then(|| format!("Context at {}: {}.", w.airport, notes.join(", ")))
        });
        // System-event correlation: aircraft faults on the same flight within 15 minutes.
        let related: Vec<&&FaultEvent> = faults
            .iter()
            .filter(|e| {
                e.flight_id.as_deref() == Some(f.flight_id) && (e.ts - at).num_minutes().abs() <= 15
            })
            .collect();
        let correlation_note = (!related.is_empty()).then(|| {
            let systems: Vec<&str> = related.iter().map(|e| e.system.label()).collect();
            format!(
                "Aircraft-system messages near this event: {}. Suspected relationship only.",
                systems.join(", ")
            )
        });
        let grade = if related.is_empty() {
            EvidenceGrade::Suspected
        } else {
            EvidenceGrade::Corroborated
        };
        out.push(FlightEvent {
            id: format!("FE-{}-{:?}", f.flight_id, kind).to_lowercase(),
            flight_id: f.flight_id.into(),
            tail: f.tail.into(),
            number: f.number.into(),
            origin: f.origin.into(),
            dest: f.dest.into(),
            kind,
            severity: sev,
            at,
            observed,
            threshold,
            source: s.source,
            data_age_seconds: (now - s.received_at).num_seconds().max(0),
            weather: wx,
            weather_note,
            correlated_events: related.iter().map(|e| e.id.clone()).collect(),
            correlation_note,
            evidence_grade: grade,
            module: Module::Flight,
            model_version: MODEL_VERSION.into(),
            limitation: LIMITATION.into(),
        });
    };
    let approach = f.arr - chrono::Duration::minutes(3);
    if s.max_descent_below_1000_fpm > 1000.0 {
        push(
            FlightEventKind::UnstableApproach,
            Severity::Caution,
            approach,
            format!("{:.0} fpm", s.max_descent_below_1000_fpm),
            "≤ 1,000 fpm below 1,000 ft".into(),
            Some(f.dest),
        );
    }
    if s.max_descent_fpm > 6000.0 {
        let at = f.dep + (f.arr - f.dep) / 2;
        push(
            FlightEventKind::RapidDescent,
            Severity::Warning,
            at,
            format!("{:.0} fpm", s.max_descent_fpm),
            "≤ 6,000 fpm outside an emergency".into(),
            None,
        );
    }
    if s.max_speed_below_10k_kt > 250.0 {
        push(
            FlightEventKind::SpeedExceedanceBelow10k,
            Severity::Advisory,
            f.dep + chrono::Duration::minutes(6),
            format!("{:.0} kt", s.max_speed_below_10k_kt),
            "≤ 250 kt below 10,000 ft".into(),
            Some(f.origin),
        );
    }
    if s.cruise_alt_deviation_ft > 300.0 {
        let at = f.dep + (f.arr - f.dep) / 3;
        push(
            FlightEventKind::CruiseAltitudeDeviation,
            Severity::Advisory,
            at,
            format!("{:.0} ft", s.cruise_alt_deviation_ft),
            "≤ 300 ft from cleared level".into(),
            None,
        );
    }
    let climb_norm = if f.wide { 1800.0 } else { 2200.0 };
    if s.max_climb_fpm < climb_norm * 0.55 {
        push(
            FlightEventKind::WeakClimbPerformance,
            Severity::Advisory,
            f.dep + chrono::Duration::minutes(4),
            format!("{:.0} fpm max climb", s.max_climb_fpm),
            format!("≥ {:.0} fpm typical for type", climb_norm * 0.55),
            Some(f.origin),
        );
    }
    if s.max_gap_s > 300 {
        push(
            FlightEventKind::TrackDataGap,
            Severity::Info,
            f.dep,
            format!("{} s gap, {} samples", s.max_gap_s, s.samples),
            "≤ 300 s between samples".into(),
            None,
        );
    }
    let delay = (s.received_at - f.arr).num_minutes();
    if delay > 60 {
        push(
            FlightEventKind::DelayedData,
            Severity::Info,
            f.arr,
            format!("received {delay} min after arrival"),
            "≤ 60 min".into(),
            None,
        );
    }
    out
}

/// Reconstruct a plausible minute-by-minute profile consistent with the summary, for display.
#[derive(Debug, Clone, Serialize)]
pub struct TrackPoint {
    pub t_min: u32,
    pub alt_ft: f64,
    pub speed_kt: f64,
    pub vs_fpm: f64,
}

pub fn track(dep: Ts, arr: Ts, s: &FlightSummary) -> Vec<TrackPoint> {
    let total = (arr - dep).num_minutes().max(20) as u32;
    let climb_min =
        ((s.cruise_alt_ft / s.max_climb_fpm.max(800.0)) * 1.4).min(total as f64 / 3.0) as u32;
    let descent_min = ((s.cruise_alt_ft / 2000.0) * 1.3).min(total as f64 / 3.0) as u32;
    let mut pts = Vec::with_capacity(total as usize + 1);
    let mut alt = 0.0;
    for t in 0..=total {
        let target = if t < climb_min {
            s.cruise_alt_ft * (t as f64 / climb_min.max(1) as f64)
        } else if t + descent_min > total {
            s.cruise_alt_ft * ((total - t) as f64 / descent_min.max(1) as f64)
        } else if t == total / 3 {
            s.cruise_alt_ft - s.cruise_alt_deviation_ft
        } else {
            s.cruise_alt_ft
        };
        let vs = target - alt;
        alt = target;
        let speed = if alt < 10000.0 {
            (150.0 + alt / 100.0).min(s.max_speed_below_10k_kt)
        } else {
            s.cruise_speed_kt
        };
        pts.push(TrackPoint {
            t_min: t,
            alt_ft: alt.max(0.0),
            speed_kt: speed,
            vs_fpm: vs,
        });
    }
    pts
}

/// Group helper used by services.
pub fn index_faults(events: &[FaultEvent]) -> HashMap<&str, Vec<&FaultEvent>> {
    let mut m: HashMap<&str, Vec<&FaultEvent>> = HashMap::new();
    for e in events {
        if let Some(f) = &e.flight_id {
            m.entry(f.as_str()).or_default().push(e);
        }
    }
    m
}

pub fn system_of(_k: FlightEventKind) -> System {
    System::Other
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn summary() -> FlightSummary {
        FlightSummary {
            flight_id: "F1".into(),
            tail: "SE-T".into(),
            source: TrackSource::AdsB,
            cruise_alt_ft: 37000.0,
            max_alt_ft: 37000.0,
            cruise_speed_kt: 460.0,
            max_speed_below_10k_kt: 248.0,
            max_climb_fpm: 2600.0,
            max_descent_fpm: 2800.0,
            max_descent_below_1000_fpm: 760.0,
            cruise_alt_deviation_ft: 60.0,
            samples: 400,
            max_gap_s: 30,
            received_at: Utc.with_ymd_and_hms(2026, 1, 1, 10, 10, 0).unwrap(),
        }
    }
    fn input<'a>() -> FlightInput<'a> {
        FlightInput {
            flight_id: "F1",
            tail: "SE-T",
            number: "NA1",
            origin: "ARN",
            dest: "CPH",
            dep: Utc.with_ymd_and_hms(2026, 1, 1, 8, 0, 0).unwrap(),
            arr: Utc.with_ymd_and_hms(2026, 1, 1, 10, 0, 0).unwrap(),
            wide: false,
        }
    }

    #[test]
    fn normal_flight_has_no_events() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        assert!(analyse(&input(), &summary(), &[], now).is_empty());
    }

    #[test]
    fn each_rule_fires_with_evidence_and_limitation() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        let mut s = summary();
        s.max_descent_below_1000_fpm = 1450.0;
        s.max_descent_fpm = 7200.0;
        s.max_speed_below_10k_kt = 282.0;
        s.max_gap_s = 900;
        s.received_at = Utc.with_ymd_and_hms(2026, 1, 1, 11, 30, 0).unwrap();
        let ev = analyse(&input(), &s, &[], now);
        let kinds: Vec<_> = ev.iter().map(|e| e.kind).collect();
        for k in [
            FlightEventKind::UnstableApproach,
            FlightEventKind::RapidDescent,
            FlightEventKind::SpeedExceedanceBelow10k,
            FlightEventKind::TrackDataGap,
            FlightEventKind::DelayedData,
        ] {
            assert!(kinds.contains(&k), "{k:?} missing");
        }
        assert!(ev
            .iter()
            .all(|e| e.limitation.contains("does not establish intent")));
        assert!(ev
            .iter()
            .all(|e| e.evidence_grade != EvidenceGrade::EngineeringConfirmed));
    }

    #[test]
    fn weather_is_deterministic_and_well_formed() {
        let t = Utc.with_ymd_and_hms(2026, 3, 4, 7, 0, 0).unwrap();
        let a = weather("ARN", t);
        assert_eq!(a.metar, weather("ARN", t).metar);
        assert!(a.metar.starts_with("ARN 040700Z"));
        assert!(a.wind_dir < 360);
    }

    #[test]
    fn track_reconstruction_respects_summary() {
        let s = summary();
        let i = input();
        let t = track(i.dep, i.arr, &s);
        assert_eq!(t.first().unwrap().alt_ft, 0.0);
        assert!(t.iter().all(|p| p.alt_ft <= s.max_alt_ft + 1.0));
    }
}
