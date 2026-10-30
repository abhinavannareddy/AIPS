//! AIPS Replay Lab: deterministic synthetic fleet generator with hidden, labelled scenarios.
//!
//! Phase 0 uses synthetic data only (PRD section 11, "Demonstrator data"). Every operator,
//! lessor, registration, part number and serial produced here is fictional.
use aips_domain::model::*;
use aips_domain::truth::{ScenarioTruth, TruthKind};
use chrono::{Duration, NaiveDate, Timelike};
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

pub const HISTORY_DAYS: i64 = 90;
pub const FLEET_SIZE: usize = 50;

struct TypeSpec {
    code: &'static str,
    count: usize,
    wide: bool,
    engine: &'static str,
}

const TYPES: [TypeSpec; 5] = [
    TypeSpec {
        code: "A320neo",
        count: 18,
        wide: false,
        engine: "LEAP-1A",
    },
    TypeSpec {
        code: "B737-8",
        count: 12,
        wide: false,
        engine: "LEAP-1B",
    },
    TypeSpec {
        code: "A321neo",
        count: 8,
        wide: false,
        engine: "LEAP-1A",
    },
    TypeSpec {
        code: "B787-9",
        count: 6,
        wide: true,
        engine: "GEnx-1B",
    },
    TypeSpec {
        code: "A350-900",
        count: 6,
        wide: true,
        engine: "Trent XWB",
    },
];
const SHORT_HAUL: [&str; 16] = [
    "ARN", "CPH", "OSL", "HEL", "GOT", "BGO", "AMS", "LHR", "CDG", "FRA", "MUC", "BCN", "FCO",
    "VIE", "ZRH", "DUB",
];
const LONG_HAUL: [&str; 8] = ["JFK", "ORD", "DXB", "SIN", "NRT", "BKK", "LAX", "YYZ"];
const BASES: [&str; 4] = ["ARN", "CPH", "OSL", "HEL"];

pub struct Generated {
    pub dataset: Dataset,
    pub truth: Vec<ScenarioTruth>,
}

struct Gen {
    rng: ChaCha8Rng,
    ds: Dataset,
    truth: Vec<ScenarioTruth>,
    seq: u64,
}

impl Gen {
    fn id(&mut self, prefix: &str) -> String {
        self.seq += 1;
        format!("{prefix}-{:06}", self.seq)
    }
}

/// Default demonstrator fleet size (sum of `TYPES` counts).
pub const DEFAULT_FLEET: usize = 50;

/// Generate the default 50-aircraft fleet whose history ends at `now`.
pub fn generate(seed: u64, now: Ts) -> Generated {
    generate_fleet(seed, now, DEFAULT_FLEET)
}

/// Generate a fleet of `fleet` aircraft (NFR-04 scale testing). The type mix keeps the default
/// proportions. For `fleet == 50` the output is identical to `generate` (same seed, same data).
pub fn generate_fleet(seed: u64, now: Ts, fleet: usize) -> Generated {
    // Minimum is the default fleet: the labelled scenarios are pinned to tail indexes up to 48.
    let fleet = fleet.clamp(DEFAULT_FLEET, 2000);
    let now = now.with_nanosecond(0).unwrap();
    let mut g = Gen {
        rng: ChaCha8Rng::seed_from_u64(seed),
        ds: Dataset::default(),
        truth: vec![],
        seq: 0,
    };
    g.ds.seed = seed;
    g.ds.generated_at = Some(now);
    let start = now - Duration::days(HISTORY_DAYS);

    // ---- Fleet and components ----
    let mut letters = Vec::new();
    if fleet <= 130 {
        for a in b'A'..=b'Z' {
            for b in *b"AEKOU" {
                letters.push(format!("SE-R{}{}", a as char, b as char));
            }
        }
    } else {
        // Larger synthetic fleets: SE-Rxx plus SE-Sxx/SE-Txx/... (all fictional registrations).
        for p in b'R'..=b'Z' {
            for a in b'A'..=b'Z' {
                for b in b'A'..=b'Z' {
                    letters.push(format!("SE-{}{}{}", p as char, a as char, b as char));
                }
            }
        }
    }
    letters.shuffle(&mut g.rng);
    let mut specs = Vec::new();
    for t in &TYPES {
        let n = if fleet == DEFAULT_FLEET {
            t.count
        } else {
            (t.count * fleet).div_ceil(DEFAULT_FLEET)
        };
        for _ in 0..n {
            specs.push(t);
        }
    }
    specs.truncate(fleet);
    for (i, spec) in specs.iter().enumerate() {
        let tail = letters[i].clone();
        let lessor = match i % 3 {
            0 => Some("Aurora Aviation Capital".to_string()),
            1 => Some("Polar Lease Partners".to_string()),
            _ => None,
        };
        g.ds.aircraft.push(Aircraft {
            tail: tail.clone(),
            type_code: spec.code.into(),
            msn: 6000 + (i as u32) * 37,
            operator: "Nordic Air (demo)".into(),
            lessor,
            engines: 2,
            delivered: NaiveDate::from_ymd_opt(2016 + (i % 9) as i32, 1 + (i % 12) as u32, 10)
                .unwrap(),
            base: BASES[i % BASES.len()].into(),
            data_feed_last: now,
        });
        let comps: [(&str, u8, &str, String); 8] = [
            ("ENG1", 72, "Engine 1", spec.engine.to_string()),
            ("ENG2", 72, "Engine 2", spec.engine.to_string()),
            ("EDP-G", 29, "Engine-driven pump (green)", "EDP".into()),
            ("IDG1", 24, "Integrated drive generator 1", "IDG".into()),
            ("FQP-L1", 28, "Fuel boost pump L1", "Boost pump".into()),
            ("BRK-3", 32, "Brake assembly #3", "Carbon brake".into()),
            (
                "PACK1-V",
                21,
                "Pack 1 flow control valve",
                "Pack valve".into(),
            ),
            (
                "PFCU-AIL",
                27,
                "Aileron actuator (L)",
                "Servo actuator".into(),
            ),
        ];
        for (pos, ata, desc, model) in comps {
            let id = format!("{tail}-{pos}");
            let serial = format!("{}{:05}", &pos[..2], 10000 + g.rng.gen_range(0..89999));
            g.ds.components.push(Component {
                id,
                tail: tail.clone(),
                part_number: format!("AIPS-PN-{ata}{:03}", pos.len() * 37 % 997),
                serial,
                ata,
                position: pos.into(),
                description: format!("{desc} ({model})"),
                installed_at: start - Duration::days(g.rng.gen_range(30..900)),
                cycles_since_sv: g.rng.gen_range(800..6000),
            });
        }
    }
    let n_aircraft = g.ds.aircraft.len();
    debug_assert_eq!(n_aircraft, fleet);

    // ---- Flights and per-flight parameter summaries ----
    let tails: Vec<String> = g.ds.aircraft.iter().map(|a| a.tail.clone()).collect();
    let stale_tail = tails[48].clone();
    for (i, tail) in tails.iter().enumerate() {
        let wide = specs[i].wide;
        let base = g.ds.aircraft[i].base.clone();
        let mut t = start + Duration::minutes(g.rng.gen_range(360..600));
        let mut here = base.clone();
        let mut cycle: u32 = g.rng.gen_range(9000..30000);
        let stop_at = if *tail == stale_tail {
            now - Duration::hours(19)
        } else {
            now
        };
        let baseline = Baseline::new(&mut g.rng);
        let mut k = 0usize;
        loop {
            let block = if wide {
                g.rng.gen_range(420..660)
            } else {
                g.rng.gen_range(55..190)
            };
            let dep = t;
            let arr = dep + Duration::minutes(block);
            if arr > stop_at {
                break;
            }
            let dest = if here != base {
                base.clone()
            } else if wide {
                LONG_HAUL.choose(&mut g.rng).unwrap().to_string()
            } else {
                loop {
                    let d = SHORT_HAUL.choose(&mut g.rng).unwrap().to_string();
                    if d != here {
                        break d;
                    }
                }
            };
            cycle += 1;
            // Flight numbers repeat across aircraft, so the id also carries the registration
            // (regression: duplicate primary keys found when persisting to PostgreSQL).
            let fid = format!(
                "NA{}{:03}-{}-{}",
                if wide { 9 } else { 1 },
                (i * 7 + k % 40) % 1000,
                tail,
                dep.format("%Y%m%d%H%M")
            );
            g.ds.flights.push(Flight {
                id: fid.clone(),
                tail: tail.clone(),
                number: format!(
                    "NA{}{:03}",
                    if wide { 9 } else { 1 },
                    (i * 7 + k % 40) % 1000
                ),
                origin: here.clone(),
                dest: dest.clone(),
                dep,
                arr,
                cycle,
            });
            for p in Param::ALL {
                let v = baseline.sample(p, k, &mut g.rng);
                g.ds.samples.push(ParamSample {
                    tail: tail.clone(),
                    flight_id: fid.clone(),
                    ts: arr,
                    param: p,
                    value: v,
                });
            }
            here = dest;
            k += 1;
            let turn = if wide {
                g.rng.gen_range(120..240)
            } else {
                g.rng.gen_range(45..95)
            };
            t = arr + Duration::minutes(turn);
            // Narrow-body night stop between 22:00 and 06:00 UTC.
            if !wide && (t.hour() >= 22 || t.hour() < 5) {
                let next_morning = (t + Duration::hours(if t.hour() >= 22 { 8 } else { 1 }))
                    .with_hour(5)
                    .unwrap()
                    .with_minute(g.rng.gen_range(30..59))
                    .unwrap();
                t = next_morning.max(t);
            }
        }
        // Next scheduled departure (AIPS Prevent input).
        let std = if *tail == stale_tail {
            now + Duration::minutes(95)
        } else {
            t.max(now + Duration::minutes(25))
        };
        let number = format!(
            "NA{}{:03}",
            if wide { 9 } else { 1 },
            (i * 7 + k % 40) % 1000
        );
        g.ds.departures.push(ScheduledDeparture {
            flight_id: format!("{number}-{tail}-{}", std.format("%Y%m%d%H%M")),
            tail: tail.clone(),
            number,
            origin: here.clone(),
            dest: if here == base {
                SHORT_HAUL[(i + 3) % SHORT_HAUL.len()].to_string()
            } else {
                base.clone()
            },
            std,
        });
    }
    for a in g.ds.aircraft.iter_mut() {
        a.data_feed_last =
            g.ds.flights
                .iter()
                .filter(|f| f.tail == a.tail)
                .map(|f| f.arr)
                .max()
                .unwrap_or(start);
    }

    flight_summaries(
        &mut g,
        &specs.iter().map(|t| t.wide).collect::<Vec<_>>(),
        &tails,
    );
    nuisance_events(&mut g);
    inject_scenarios(&mut g, &tails, now);
    inject_flight_scenarios(&mut g, &tails, now);
    routine_mel(&mut g, &tails, now);

    g.ds.events.sort_by_key(|e| e.ts);
    g.ds.flights.sort_by_key(|f| f.dep);
    Generated {
        dataset: g.ds,
        truth: g.truth,
    }
}

struct Baseline {
    egt1: f64,
    egt2: f64,
    egt_rate1: f64,
    egt_rate2: f64,
    oil1: f64,
    oil2: f64,
    hyd_g: f64,
    hyd_b: f64,
    volt: f64,
    cab: f64,
}

impl Baseline {
    fn new(rng: &mut ChaCha8Rng) -> Self {
        Self {
            egt1: rng.gen_range(38.0..62.0),
            egt2: rng.gen_range(38.0..62.0),
            egt_rate1: rng.gen_range(0.15..0.6),
            egt_rate2: rng.gen_range(0.15..0.6),
            oil1: rng.gen_range(52.0..64.0),
            oil2: rng.gen_range(52.0..64.0),
            hyd_g: rng.gen_range(2980.0..3020.0),
            hyd_b: rng.gen_range(2980.0..3020.0),
            volt: rng.gen_range(114.5..115.5),
            cab: rng.gen_range(8.05..8.25),
        }
    }
    fn sample(&self, p: Param, k: usize, rng: &mut ChaCha8Rng) -> f64 {
        let n = |rng: &mut ChaCha8Rng, sd: f64| {
            // Box–Muller normal noise.
            let u1: f64 = rng.gen_range(1e-9..1.0);
            let u2: f64 = rng.gen();
            sd * (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
        };
        let kc = k as f64;
        match p {
            Param::EgtMarginEng1 => self.egt1 - self.egt_rate1 * kc / 100.0 + n(rng, 1.2),
            Param::EgtMarginEng2 => self.egt2 - self.egt_rate2 * kc / 100.0 + n(rng, 1.2),
            Param::OilPressEng1 => self.oil1 + n(rng, 1.4),
            Param::OilPressEng2 => self.oil2 + n(rng, 1.4),
            Param::HydPressGreen => self.hyd_g + n(rng, 14.0),
            Param::HydPressGreenAlt => self.hyd_g + n(rng, 14.0),
            Param::HydPressBlue => self.hyd_b + n(rng, 14.0),
            Param::BusVoltageAc1 => self.volt + n(rng, 0.45),
            Param::FuelImbalance => (180.0 + n(rng, 70.0)).abs(),
            Param::CabinDiffPress => self.cab + n(rng, 0.04),
        }
    }
}

const NUISANCE_ATAS: [u8; 12] = [21, 22, 23, 24, 27, 28, 29, 31, 32, 34, 36, 73];
const NUISANCE_MSG: [&str; 8] = [
    "INTERMITTENT FAULT",
    "BITE MSG",
    "SENSOR DEGRADED",
    "CHANNEL FAULT",
    "COMM LOSS",
    "VALVE POS DISAGREE",
    "LO PR TRANSIENT",
    "OVHT DET LOOP FAULT",
];

fn nuisance_events(g: &mut Gen) {
    let flights = g.ds.flights.clone();
    for f in flights {
        if g.rng.gen::<f64>() > 0.11 {
            continue;
        }
        let ata = *NUISANCE_ATAS.choose(&mut g.rng).unwrap();
        let code = format!(
            "{ata}-{:02}-{:02}",
            g.rng.gen_range(10..60),
            g.rng.gen_range(1..40)
        );
        let ts =
            f.dep + Duration::minutes(g.rng.gen_range(5..((f.arr - f.dep).num_minutes().max(6))));
        let id = g.id("EV");
        let sev = if g.rng.gen::<f64>() < 0.7 {
            Severity::Info
        } else {
            Severity::Advisory
        };
        let msg = format!("{code} {}", NUISANCE_MSG.choose(&mut g.rng).unwrap());
        g.ds.events.push(FaultEvent {
            id: id.clone(),
            tail: f.tail.clone(),
            flight_id: Some(f.id.clone()),
            ts,
            ata,
            system: System::from_ata(ata),
            code,
            component_id: None,
            severity: sev,
            message: msg,
            source: if g.rng.gen::<bool>() {
                DataSource::CentralMaintenanceComputer
            } else {
                DataSource::Acars
            },
            ingested_at: ts + Duration::seconds(g.rng.gen_range(4..90)),
        });
        if g.rng.gen::<f64>() < 0.5 {
            let wid = g.id("WO");
            let opened = f.arr + Duration::minutes(15);
            g.ds.work_orders.push(WorkOrder {
                id: wid,
                tail: f.tail.clone(),
                event_ids: vec![id],
                component_id: None,
                ata,
                action: if g.rng.gen::<bool>() {
                    MaintAction::Reset
                } else {
                    MaintAction::Tested
                },
                opened,
                closed: Some(opened + Duration::minutes(g.rng.gen_range(20..120))),
                signed_off: true,
                team: System::from_ata(ata).team().into(),
                note: "Operational test satisfactory".into(),
            });
        }
    }
}

/// Flights on `tail` ordered by departure, restricted to the last `days` days.
fn recent_flights(g: &Gen, tail: &str, now: Ts, days: i64) -> Vec<Flight> {
    let mut v: Vec<Flight> =
        g.ds.flights
            .iter()
            .filter(|f| f.tail == tail && f.arr <= now && now - f.dep <= Duration::days(days))
            .cloned()
            .collect();
    v.sort_by_key(|f| f.dep);
    v
}

#[allow(clippy::too_many_arguments)]
fn push_event(
    g: &mut Gen,
    f: &Flight,
    offset_min: i64,
    ata: u8,
    code: &str,
    comp: Option<String>,
    sev: Severity,
    msg: &str,
    src: DataSource,
) -> String {
    let id = g.id("EV");
    let ts = f.dep + Duration::minutes(offset_min);
    g.ds.events.push(FaultEvent {
        id: id.clone(),
        tail: f.tail.clone(),
        flight_id: Some(f.id.clone()),
        ts,
        ata,
        system: System::from_ata(ata),
        code: code.into(),
        component_id: comp,
        severity: sev,
        message: msg.into(),
        source: src,
        ingested_at: ts + Duration::seconds(20),
    });
    id
}

fn truth(g: &mut Gen, kind: TruthKind, tail: &str, codes: &[&str], params: &[&str], desc: &str) {
    let id = format!("SCN-{:02}", g.truth.len() + 1);
    g.truth.push(ScenarioTruth {
        id,
        kind,
        tail: tail.into(),
        codes: codes.iter().map(|s| s.to_string()).collect(),
        params: params.iter().map(|s| s.to_string()).collect(),
        description: desc.into(),
    });
}

#[allow(clippy::too_many_arguments)]
fn recurrence(
    g: &mut Gen,
    tail: &str,
    now: Ts,
    ata: u8,
    code: &str,
    comp: &str,
    msg: &str,
    n: usize,
    fixes: &[MaintAction],
) {
    let flights = recent_flights(g, tail, now, 24);
    let step = (flights.len() / (n + 1)).max(1);
    let comp_id = format!("{tail}-{comp}");
    for j in 0..n {
        let f = flights[(j + 1) * step - 1].clone();
        let eid = push_event(
            g,
            &f,
            25 + j as i64 * 7,
            ata,
            code,
            Some(comp_id.clone()),
            Severity::Caution,
            msg,
            DataSource::Acars,
        );
        if let Some(action) = fixes.get(j) {
            let wid = g.id("WO");
            let opened = f.arr + Duration::minutes(30);
            g.ds.work_orders.push(WorkOrder {
                id: wid,
                tail: tail.into(),
                event_ids: vec![eid],
                component_id: Some(comp_id.clone()),
                ata,
                action: *action,
                opened,
                closed: Some(opened + Duration::hours(3)),
                signed_off: true,
                team: System::from_ata(ata).team().into(),
                note: format!("{action:?} per AMM troubleshooting; ops test satisfactory"),
            });
        }
    }
}

fn cross(g: &mut Gen, tail: &str, now: Ts, a: (u8, &str, &str), b: (u8, &str, &str), n: usize) {
    let flights = recent_flights(g, tail, now, 40);
    let step = (flights.len() / (n + 1)).max(1);
    for j in 0..n {
        let f = flights[(j + 1) * step - 1].clone();
        push_event(
            g,
            &f,
            30,
            a.0,
            a.1,
            None,
            Severity::Advisory,
            a.2,
            DataSource::CentralMaintenanceComputer,
        );
        push_event(
            g,
            &f,
            30 + 4 + j as i64 * 3,
            b.0,
            b.1,
            None,
            Severity::Caution,
            b.2,
            DataSource::Acars,
        );
    }
}

fn adjust_samples(
    g: &mut Gen,
    tail: &str,
    param: Param,
    last_n: usize,
    f: impl Fn(usize, f64) -> f64,
) {
    let mut idx: Vec<usize> =
        g.ds.samples
            .iter()
            .enumerate()
            .filter(|(_, s)| s.tail == tail && s.param == param)
            .map(|(i, _)| i)
            .collect();
    idx.sort_by_key(|i| g.ds.samples[*i].ts);
    let total = idx.len();
    for (j, i) in idx.into_iter().enumerate() {
        if j + last_n >= total {
            let pos = j + last_n - total;
            g.ds.samples[i].value = f(pos, g.ds.samples[i].value);
        }
    }
}

fn inject_scenarios(g: &mut Gen, tails: &[String], now: Ts) {
    use MaintAction::*;
    let t = |i: usize| tails[i].clone();

    let tl = t(3);
    recurrence(
        g,
        &tl,
        now,
        29,
        "29-11-07",
        "EDP-G",
        "HYD G ENG 1 PUMP LO PR",
        4,
        &[],
    );
    truth(
        g,
        TruthKind::Recurrence,
        &tl,
        &["29-11-07"],
        &[],
        "Green EDP low-pressure message recurring, no repair yet",
    );

    let tl = t(7);
    recurrence(
        g,
        &tl,
        now,
        29,
        "29-31-04",
        "EDP-G",
        "HYD G RSVR LO LVL",
        4,
        &[Replaced, Adjusted],
    );
    truth(
        g,
        TruthKind::RepeatAfterFix,
        &tl,
        &["29-31-04"],
        &[],
        "Green reservoir level message returns after two repairs",
    );

    let tl = t(15);
    recurrence(
        g,
        &tl,
        now,
        32,
        "32-42-11",
        "BRK-3",
        "BRAKES HOT #3",
        3,
        &[Cleaned],
    );
    truth(
        g,
        TruthKind::RepeatAfterFix,
        &tl,
        &["32-42-11"],
        &[],
        "Brake #3 hot recurs after cleaning",
    );

    let tl = t(22);
    recurrence(
        g,
        &tl,
        now,
        21,
        "21-31-02",
        "PACK1-V",
        "CAB PR SYS 1 FAULT",
        3,
        &[],
    );
    truth(
        g,
        TruthKind::Recurrence,
        &tl,
        &["21-31-02"],
        &[],
        "Cabin pressure controller channel 1 fault recurring",
    );

    let tl = t(41);
    recurrence(
        g,
        &tl,
        now,
        28,
        "28-21-05",
        "FQP-L1",
        "FUEL L TK PUMP 1 LO PR",
        3,
        &[],
    );
    truth(
        g,
        TruthKind::Recurrence,
        &tl,
        &["28-21-05"],
        &[],
        "Left tank pump 1 low pressure recurring",
    );

    let tl = t(11);
    cross(
        g,
        &tl,
        now,
        (24, "24-22-05", "AC BUS 1 VOLTAGE TRANSIENT"),
        (77, "77-11-03", "ENG 1 EGT SIGNAL DISAGREE"),
        5,
    );
    truth(
        g,
        TruthKind::CrossSystem,
        &tl,
        &["24-22-05", "77-11-03"],
        &[],
        "AC bus 1 transients precede engine 1 EGT indication faults",
    );

    let tl = t(33);
    cross(
        g,
        &tl,
        now,
        (24, "24-31-02", "DC ESS BUS FAULT"),
        (27, "27-51-04", "FLAP SYS 1 FAULT"),
        4,
    );
    truth(
        g,
        TruthKind::CrossSystem,
        &tl,
        &["24-31-02", "27-51-04"],
        &[],
        "DC essential bus faults coincide with flap channel faults",
    );

    let tl = t(19);
    adjust_samples(g, &tl, Param::EgtMarginEng2, 260, |pos, v| {
        v - 0.085 * pos as f64
    });
    truth(
        g,
        TruthKind::Degradation,
        &tl,
        &[],
        &["egt_margin_eng2"],
        "Engine 2 EGT margin deteriorating rapidly",
    );

    let tl = t(27);
    adjust_samples(g, &tl, Param::HydPressGreen, 16, |_, _| 3001.5);
    truth(
        g,
        TruthKind::FrozenSensor,
        &tl,
        &[],
        &["hyd_press_green"],
        "Green hydraulic pressure sensor A frozen",
    );

    let tl = t(37);
    adjust_samples(g, &tl, Param::HydPressGreenAlt, 30, |pos, v| {
        v * (1.0 - 0.004 * pos as f64)
    });
    truth(
        g,
        TruthKind::SensorDisagreement,
        &tl,
        &[],
        &["hyd_press_green_alt"],
        "Green hydraulic sensor B drifting low versus sensor A",
    );

    let tl = t(45);
    adjust_samples(g, &tl, Param::OilPressEng1, 22, |_, v| v - 9.0);
    truth(
        g,
        TruthKind::TrendShift,
        &tl,
        &[],
        &["oil_press_eng1"],
        "Engine 1 oil pressure step decrease",
    );

    let tl = t(48);
    truth(
        g,
        TruthKind::StaleFeed,
        &tl,
        &[],
        &[],
        "Aircraft data feed silent for 19 h",
    );

    // Configuration: the same IDG serial recorded on two aircraft.
    let (a, b) = (t(30), t(5));
    let src =
        g.ds.components
            .iter()
            .find(|c| c.tail == b && c.position == "IDG1")
            .unwrap()
            .clone();
    if let Some(c) =
        g.ds.components
            .iter_mut()
            .find(|c| c.tail == a && c.position == "IDG1")
    {
        c.part_number = src.part_number.clone();
        c.serial = src.serial.clone();
    }
    truth(
        g,
        TruthKind::ConfigMismatch,
        &a,
        &[],
        &[],
        "IDG 1 serial also recorded on another aircraft",
    );

    let tl = t(12);
    let mid = g.id("MEL");
    g.ds.mel.push(MelItem {
        id: mid,
        tail: tl.clone(),
        reference: "30-21-01".into(),
        title: "Engine 2 anti-ice valve inoperative".into(),
        category: 'C',
        ata: 30,
        opened: now - Duration::days(11),
        due: now - Duration::hours(6),
    });
    truth(
        g,
        TruthKind::MelOverdue,
        &tl,
        &[],
        &[],
        "MEL item past rectification interval",
    );

    let tl = t(24);
    let f = recent_flights(g, &tl, now, 4)
        .into_iter()
        .rfind(|f| f.arr < now - Duration::hours(8))
        .unwrap();
    let eid = push_event(
        g,
        &f,
        40,
        36,
        "36-11-06",
        None,
        Severity::Advisory,
        "BLEED 2 OVERPRESSURE",
        DataSource::TechLog,
    );
    let wid = g.id("WO");
    g.ds.work_orders.push(WorkOrder {
        id: wid,
        tail: tl.clone(),
        event_ids: vec![eid],
        component_id: None,
        ata: 36,
        action: Replaced,
        opened: f.arr + Duration::minutes(20),
        closed: Some(f.arr + Duration::hours(2)),
        signed_off: false,
        team: "Powerplant Engineering".into(),
        note: "Bleed pressure regulating valve replaced".into(),
    });
    truth(
        g,
        TruthKind::UnsignedWorkOrder,
        &tl,
        &[],
        &[],
        "Corrective action closed without certifying sign-off",
    );
}

fn flight_summaries(g: &mut Gen, wide_by_index: &[bool], tails: &[String]) {
    use aips_domain::flight::{FlightSummary, TrackSource};
    let idx: std::collections::HashMap<&str, usize> = tails
        .iter()
        .enumerate()
        .map(|(i, t)| (t.as_str(), i))
        .collect();
    let flights = g.ds.flights.clone();
    for f in &flights {
        let wide = wide_by_index[idx[f.tail.as_str()]];
        let minutes = (f.arr - f.dep).num_minutes().max(20) as u32;
        let cruise = if wide {
            g.rng.gen_range(35000.0..41000.0)
        } else {
            g.rng.gen_range(31000.0..39000.0)
        };
        g.ds.flight_summaries.push(FlightSummary {
            flight_id: f.id.clone(),
            tail: f.tail.clone(),
            source: if g.rng.gen::<f64>() < 0.7 {
                TrackSource::AdsB
            } else {
                TrackSource::Qar
            },
            cruise_alt_ft: (cruise / 1000.0_f64).round() * 1000.0,
            max_alt_ft: (cruise / 1000.0_f64).round() * 1000.0,
            cruise_speed_kt: if wide {
                g.rng.gen_range(475.0..500.0)
            } else {
                g.rng.gen_range(435.0..470.0)
            },
            max_speed_below_10k_kt: g.rng.gen_range(228.0..249.0),
            max_climb_fpm: if wide {
                g.rng.gen_range(1800.0..2800.0)
            } else {
                g.rng.gen_range(2200.0..3400.0)
            },
            max_descent_fpm: g.rng.gen_range(1800.0..3300.0),
            max_descent_below_1000_fpm: g.rng.gen_range(520.0..940.0),
            cruise_alt_deviation_ft: g.rng.gen_range(0.0..140.0),
            samples: minutes * 6,
            max_gap_s: g.rng.gen_range(8..75),
            received_at: f.arr + Duration::minutes(g.rng.gen_range(2..40)),
        });
    }
}

/// Mutate the summaries of the given flights (scenario injection).
fn adjust_flights(
    g: &mut Gen,
    ids: &[String],
    f: impl Fn(&mut aips_domain::flight::FlightSummary),
) {
    for s in
        g.ds.flight_summaries
            .iter_mut()
            .filter(|s| ids.contains(&s.flight_id))
    {
        f(s);
    }
}

fn inject_flight_scenarios(g: &mut Gen, tails: &[String], now: Ts) {
    let pick = |g: &Gen, tail: &str, days: i64, n: usize| -> Vec<Flight> {
        let fl = recent_flights(g, tail, now, days);
        let step = (fl.len() / (n + 1)).max(1);
        (0..n)
            .filter_map(|j| fl.get((j + 1) * step - 1).cloned())
            .collect()
    };
    let tl = tails[2].clone();
    let ids: Vec<String> = pick(g, &tl, 12, 3).iter().map(|f| f.id.clone()).collect();
    adjust_flights(g, &ids, |s| {
        s.max_descent_below_1000_fpm = 1350.0 + s.max_gap_s as f64
    });
    truth(
        g,
        TruthKind::FlightUnstableApproach,
        &tl,
        &[],
        &[],
        "Repeated high descent rates below 1,000 ft on approach",
    );

    let tl = tails[26].clone();
    let f = pick(g, &tl, 9, 1).remove(0);
    adjust_flights(g, std::slice::from_ref(&f.id), |s| {
        s.max_descent_fpm = 7400.0
    });
    let mid = (f.arr - f.dep).num_minutes() / 2;
    push_event(
        g,
        &f,
        mid - 2,
        21,
        "21-31-09",
        None,
        Severity::Warning,
        "CAB PRESS EXCESS CAB ALT",
        DataSource::Acars,
    );
    truth(
        g,
        TruthKind::FlightRapidDescent,
        &tl,
        &[],
        &[],
        "Rapid descent coinciding with a cabin altitude warning",
    );

    let tl = tails[35].clone();
    let ids: Vec<String> = pick(g, &tl, 15, 2).iter().map(|f| f.id.clone()).collect();
    adjust_flights(g, &ids, |s| s.max_speed_below_10k_kt = 276.0);
    truth(
        g,
        TruthKind::FlightSpeedExceedance,
        &tl,
        &[],
        &[],
        "Speed above 250 kt below 10,000 ft",
    );

    let tl = tails[47].clone();
    let ids: Vec<String> = pick(g, &tl, 20, 4).iter().map(|f| f.id.clone()).collect();
    adjust_flights(g, &ids, |s| {
        s.max_gap_s = 1260;
        s.samples /= 3;
    });
    truth(
        g,
        TruthKind::FlightDataGap,
        &tl,
        &[],
        &[],
        "ADS-B coverage gaps leave incomplete trajectories",
    );
}

fn routine_mel(g: &mut Gen, tails: &[String], now: Ts) {
    for (i, tail) in tails.iter().enumerate().filter(|(i, _)| i % 9 == 2) {
        let id = g.id("MEL");
        g.ds.mel.push(MelItem {
            id,
            tail: tail.clone(),
            reference: format!("25-{:02}-01", 10 + i % 40),
            title: "Cabin reading light inoperative (row 14)".into(),
            category: 'D',
            ata: 25,
            opened: now - Duration::days(2),
            due: now + Duration::days(8),
        });
    }
    // A deferred hydraulic defect needing authorised review.
    let tail = tails[9].clone();
    let wid = g.id("WO");
    g.ds.work_orders.push(WorkOrder {
        id: wid,
        tail,
        event_ids: vec![],
        component_id: None,
        ata: 29,
        action: MaintAction::Deferred,
        opened: now - Duration::hours(20),
        closed: None,
        signed_off: false,
        team: "Hydraulics & Landing Gear".into(),
        note: "Minor seepage at green system filter housing within AMM limits".into(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    #[test]
    fn deterministic_and_complete() {
        let now = Utc.with_ymd_and_hms(2026, 10, 30, 12, 0, 0).unwrap();
        let a = generate(7, now);
        let b = generate(7, now);
        assert_eq!(a.dataset.aircraft.len(), FLEET_SIZE);
        assert_eq!(a.dataset.events.len(), b.dataset.events.len());
        assert_eq!(a.truth.len(), 19);
        assert_eq!(a.dataset.flight_summaries.len(), a.dataset.flights.len());
        assert!(a.dataset.flights.len() > 10_000);
        assert_eq!(a.dataset.departures.len(), FLEET_SIZE);
        assert!(a.dataset.events.iter().all(|e| e.ts <= now));
        // Regression: identifiers must be unique (they are primary keys in PostgreSQL).
        let mut ids: Vec<&str> = a.dataset.flights.iter().map(|f| f.id.as_str()).collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n, "duplicate flight ids");
        let mut ev: Vec<&str> = a.dataset.events.iter().map(|e| e.id.as_str()).collect();
        let m = ev.len();
        ev.sort_unstable();
        ev.dedup();
        assert_eq!(ev.len(), m, "duplicate event ids");
    }
}
