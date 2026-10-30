//! Property-based tests (proptest) for safety-relevant invariants across random inputs.
use aips_domain::audit::AuditChain;
use aips_domain::intel::EvidenceGrade;
use aips_domain::memory::{analyse, MemoryConfig};
use aips_domain::prevent::{self, Inputs, OverallState};
use chrono::{Duration, TimeZone, Utc};
use proptest::prelude::*;
use std::collections::HashSet;

fn arb_entry() -> impl Strategy<Value = (String, String, String, i64)> {
    (
        "[a-z.]{1,12}",
        "[a-z.]{1,16}",
        "[A-Za-z0-9-]{1,20}",
        any::<i64>(),
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Any sequence of appends verifies; changing any field of any entry is detected at or before it.
    #[test]
    fn audit_chain_detects_any_single_mutation(entries in prop::collection::vec(arb_entry(), 1..40), pick in any::<prop::sample::Index>(), field in 0u8..5) {
        let mut c = AuditChain::default();
        for (actor, action, subject, n) in &entries {
            c.append(actor, action, subject, serde_json::json!({ "n": n }));
        }
        prop_assert_eq!(c.verify(), None);
        let i = pick.index(c.entries.len());
        let e = &mut c.entries[i];
        match field {
            0 => e.actor.push('x'),
            1 => e.action.push('x'),
            2 => e.subject.push('x'),
            3 => e.detail = serde_json::json!({ "n": "tampered" }),
            _ => e.at += Duration::seconds(1),
        }
        let broken = c.verify();
        prop_assert!(broken.is_some());
        prop_assert!(broken.unwrap() <= c.entries[i].seq);
    }

    /// A chain survives a save/load round trip through JSON (the emergency journal on disk), even
    /// with arbitrary floating-point detail such as fused confidence values.
    #[test]
    fn audit_chain_verifies_after_json_round_trip(xs in prop::collection::vec(any::<f64>().prop_filter("finite", |x| x.is_finite()), 1..30)) {
        let mut c = AuditChain::default();
        for x in &xs {
            c.append("crew", "ack", "INC-1", serde_json::json!({ "confidence": x }));
        }
        let back: AuditChain = serde_json::from_str(&serde_json::to_string_pretty(&c).unwrap()).unwrap();
        prop_assert_eq!(back.verify(), None);
    }

    /// Deleting or reordering entries is detected.
    #[test]
    fn audit_chain_detects_deletion(entries in prop::collection::vec(arb_entry(), 2..30), pick in any::<prop::sample::Index>()) {
        let mut c = AuditChain::default();
        for (actor, action, subject, n) in &entries {
            c.append(actor, action, subject, serde_json::json!({ "n": n }));
        }
        let i = pick.index(c.entries.len() - 1);
        c.entries.remove(i);
        prop_assert!(c.verify().is_some());
    }
}

proptest! {
    // Dataset generation is ~0.4 s per case, so fewer cases with random seeds and clocks.
    #![proptest_config(ProptestConfig::with_cases(10))]

    /// Cross-Flight Memory invariants for any fleet history.
    #[test]
    fn memory_invariants(seed in any::<u64>(), day in 0i64..365) {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap() + Duration::days(day);
        let g = aips_replay::generate(seed, now);
        let cfg = MemoryConfig::default();
        let h = analyse(&g.dataset, &cfg);
        let ids: HashSet<_> = g.dataset.events.iter().map(|e| (e.id.as_str(), e.tail.as_str())).collect();
        let mut seen = HashSet::new();
        for x in &h {
            // Machines never confirm: only engineers can (CFM-03).
            prop_assert!(x.machine_grade != EvidenceGrade::EngineeringConfirmed);
            prop_assert!(x.grade != EvidenceGrade::EngineeringConfirmed);
            prop_assert!(seen.insert(x.id.clone()), "hypothesis ids are unique");
            // Every linked event exists and belongs to the same aircraft (no cross-tenant leakage).
            for e in &x.event_ids {
                prop_assert!(ids.contains(&(e.as_str(), x.tail.as_str())));
            }
            prop_assert!(!x.explanation.narrative.is_empty(), "every hypothesis is explained");
        }
        // Deterministic: same input, same hypotheses (stable ids, reproducible evidence).
        let again = analyse(&g.dataset, &cfg);
        prop_assert_eq!(h.iter().map(|x| &x.id).collect::<Vec<_>>(), again.iter().map(|x| &x.id).collect::<Vec<_>>());
    }

    /// Prevent never reports an aircraft as "clear"; stale data always yields Insufficient data.
    #[test]
    fn prevent_never_clears_and_flags_stale_data(seed in any::<u64>(), extra_hours in 0i64..200) {
        let gen_now = Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap();
        let g = aips_replay::generate(seed, gen_now);
        let now = gen_now + Duration::hours(extra_hours);
        let inp = Inputs { ds: &g.dataset, hypotheses: &[], alerts: &[], cases: &[], rul: &[], rul_available: true, now };
        for dep in g.dataset.departures.iter().take(15) {
            let r = prevent::build(&inp, dep);
            prop_assert!(!r.disclaimer.is_empty());
            let feed = g.dataset.aircraft.iter().find(|a| a.tail == dep.tail).unwrap().data_feed_last;
            if now - feed > Duration::hours(prevent::STALE_AFTER_HOURS) {
                prop_assert_eq!(r.overall, OverallState::InsufficientData);
                prop_assert!(prevent::needs_escalation(&r));
            }
            if r.overall == OverallState::NoFindingsInAvailableData {
                prop_assert!(r.ranked.is_empty() || r.ranked.iter().all(|f| f.severity < aips_domain::model::Severity::Caution));
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(6))]

    /// The indexed fleet path gives exactly the per-departure result (regression for the
    /// NFR-04 performance rewrite, bug B11).
    #[test]
    fn build_all_matches_build(seed in any::<u64>(), extra_hours in 0i64..48) {
        let gen_now = Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap();
        let g = aips_replay::generate(seed, gen_now);
        let now = gen_now + Duration::hours(extra_hours);
        let hyps = analyse(&g.dataset, &MemoryConfig::default());
        let inp = Inputs { ds: &g.dataset, hypotheses: &hyps, alerts: &[], cases: &[], rul: &[], rul_available: true, now };
        let all = prevent::build_all(&inp, &g.dataset.departures);
        for (d, a) in g.dataset.departures.iter().zip(&all) {
            let one = prevent::build(&inp, d);
            prop_assert_eq!(one.overall, a.overall);
            prop_assert_eq!(serde_json::to_value(&one.ranked).unwrap(), serde_json::to_value(&a.ranked).unwrap());
            prop_assert_eq!(serde_json::to_value(&one.categories).unwrap(), serde_json::to_value(&a.categories).unwrap());
        }
    }

    /// PRV-03: concerns are ranked by status, then severity, then evidence grade (most
    /// serious first), and every report shows freshness for each data source.
    #[test]
    fn prevent_ranks_concerns_and_shows_freshness(seed in any::<u64>(), extra_hours in 0i64..48) {
        use aips_domain::prevent::ItemStatus;
        let status_rank = |s: ItemStatus| match s {
            ItemStatus::Complete => 0,
            ItemStatus::RequiresApproval => 1,
            ItemStatus::Missing => 2,
            ItemStatus::Inconsistent => 3,
            ItemStatus::Outstanding => 4,
            ItemStatus::InsufficientData => 5,
        };
        let gen_now = Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap();
        let g = aips_replay::generate(seed, gen_now);
        let now = gen_now + Duration::hours(extra_hours);
        let hyps = analyse(&g.dataset, &MemoryConfig::default());
        let inp = Inputs { ds: &g.dataset, hypotheses: &hyps, alerts: &[], cases: &[], rul: &[], rul_available: true, now };
        for r in prevent::build_all(&inp, &g.dataset.departures) {
            let key = |f: &prevent::Finding| (status_rank(f.status), f.severity, f.evidence_grade);
            for w in r.ranked.windows(2) {
                prop_assert!(key(&w[0]) >= key(&w[1]), "ranked out of order in {}", r.id);
            }
            prop_assert_eq!(r.ranked.len(), r.categories.iter().map(|c| c.findings.len()).sum::<usize>());
            prop_assert!(r.freshness.len() >= 2, "health feed and maintenance system both shown");
            for f in &r.freshness {
                prop_assert!(!f.source.is_empty() && f.age_seconds >= 0);
            }
            prop_assert_eq!(r.freshness[0].stale, r.overall == OverallState::InsufficientData);
        }
    }
}

/// Regression (P4): faults on systems outside the named categories (ATA 26 fire protection maps
/// to `System::Other`) were never reported, so an aircraft could show "no findings".
#[test]
fn prevent_reports_faults_on_other_systems() {
    use aips_domain::model::{DataSource, FaultEvent, Severity, System};
    let now = Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap();
    let mut g = aips_replay::generate(7, now);
    let dep = g
        .dataset
        .departures
        .iter()
        .find(|d| {
            let feed = g
                .dataset
                .aircraft
                .iter()
                .find(|a| a.tail == d.tail)
                .unwrap()
                .data_feed_last;
            now - feed <= Duration::hours(prevent::STALE_AFTER_HOURS)
        })
        .expect("a departure with fresh data")
        .clone();
    g.dataset.events.push(FaultEvent {
        id: "T-ATA26-FIRE-DET".into(),
        tail: dep.tail.clone(),
        flight_id: None,
        ts: now - Duration::hours(1),
        ata: 26,
        system: System::from_ata(26),
        code: "FIRE-DET-LOOP-A".into(),
        component_id: None,
        severity: Severity::Warning,
        message: "Engine fire detection loop A fault".into(),
        source: DataSource::CentralMaintenanceComputer,
        ingested_at: now - Duration::hours(1),
    });
    let inp = Inputs {
        ds: &g.dataset,
        hypotheses: &[],
        alerts: &[],
        cases: &[],
        rul: &[],
        rul_available: true,
        now,
    };
    let r = prevent::build(&inp, &dep);
    assert!(
        r.ranked
            .iter()
            .any(|f| f.record_ids.iter().any(|id| id == "T-ATA26-FIRE-DET")),
        "fire-protection fault missing from the pre-departure report"
    );
    assert_ne!(r.overall, OverallState::NoFindingsInAvailableData);
}

/// Regression (P4): with no remaining-life model loaded, reports said nothing about component
/// life; now the Engines category carries an explicit Insufficient-data finding.
#[test]
fn prevent_flags_missing_rul_model() {
    use aips_domain::prevent::{Category, ItemStatus};
    let now = Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap();
    let g = aips_replay::generate(7, now);
    let inp = Inputs {
        ds: &g.dataset,
        hypotheses: &[],
        alerts: &[],
        cases: &[],
        rul: &[],
        rul_available: false,
        now,
    };
    for r in prevent::build_all(&inp, &g.dataset.departures) {
        assert_ne!(
            r.overall,
            OverallState::NoFindingsInAvailableData,
            "{}",
            r.id
        );
        assert!(r.ranked.iter().any(|f| f.category == Category::Engines
            && f.status == ItemStatus::InsufficientData
            && f.title == "Remaining-life model unavailable"));
    }
}
