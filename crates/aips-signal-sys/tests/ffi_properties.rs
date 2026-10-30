//! Property tests across the Rust/C++ FFI boundary: arbitrary inputs never crash, never yield
//! non-finite statistics for finite data, batch equals single extraction, and known closed forms hold.
use aips_signal_sys::{cross_sensor_disagreement, extract, extract_batch, DetectorConfig};
use proptest::prelude::*;

fn finite_series() -> impl Strategy<Value = Vec<f64>> {
    prop::collection::vec(-1.0e6f64..1.0e6, 0..600)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn extract_is_total_and_finite(v in finite_series(), lo in -1.0e3f64..0.0, span in 1.0f64..1.0e4) {
        let cfg = DetectorConfig::default_for(lo, lo + span);
        match extract(&v, &cfg) {
            Ok(f) => {
                for x in [f.mean, f.stddev, f.ewma, f.slope, f.min, f.max, f.last, f.cusum_pos, f.cusum_neg] {
                    prop_assert!(x.is_finite(), "non-finite output {x}");
                }
                prop_assert!(f.min <= f.mean + 1e-6 && f.mean <= f.max + 1e-6);
                prop_assert!(f.stddev >= 0.0);
                prop_assert_eq!(f.n as usize, v.len());
            }
            // Contract (aips_signal.h): series shorter than max(4, baseline_samples / 2) are rejected.
            Err(_) => prop_assert!(v.len() < (cfg.baseline_samples as usize / 2).max(4), "only too-short series may be rejected"),
        }
    }

    #[test]
    fn slope_of_a_line_is_recovered(a in -500.0f64..500.0, b in -10.0f64..10.0, n in 30usize..400) {
        let v: Vec<f64> = (0..n).map(|i| a + b * i as f64).collect();
        let f = extract(&v, &DetectorConfig::default_for(-1.0e5, 1.0e5)).unwrap();
        prop_assert!((f.slope - b).abs() < 1e-6 * (1.0 + b.abs()), "slope {} vs {}", f.slope, b);
    }

    #[test]
    fn batch_equals_single(series in prop::collection::vec(prop::collection::vec(-100.0f64..100.0, 2..120), 1..12)) {
        let cfg = DetectorConfig::default_for(-200.0, 200.0);
        let refs: Vec<&[f64]> = series.iter().map(|s| s.as_slice()).collect();
        let cfgs = vec![cfg; refs.len()];
        let batch = extract_batch(&refs, &cfgs).unwrap();
        for (s, b) in series.iter().zip(&batch) {
            match extract(s, &cfg) {
                Ok(one) => {
                    prop_assert_eq!(one.n, b.n);
                    prop_assert!((one.mean - b.mean).abs() < 1e-9 && (one.slope - b.slope).abs() < 1e-9);
                }
                Err(_) => prop_assert_eq!(b.n, 0, "short series in a batch yield empty features"),
            }
        }
    }

    #[test]
    fn disagreement_is_symmetric_and_zero_for_identical(a in prop::collection::vec(-1.0e3f64..1.0e3, 0..200), b in prop::collection::vec(-1.0e3f64..1.0e3, 0..200)) {
        let d1 = cross_sensor_disagreement(&a, &b);
        let d2 = cross_sensor_disagreement(&b, &a);
        prop_assert!(d1.is_finite() && (d1 - d2).abs() < 1e-9);
        prop_assert!(cross_sensor_disagreement(&a, &a).abs() < 1e-12);
    }
}
