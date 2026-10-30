//! AIPS Predict: remaining useful life with uncertainty (PRD-01, PRD-02), a versioned model
//! registry with rollback, and input-drift monitoring (NFR-10). Inference runs in C++ via ONNX Runtime.
use crate::integrity::{config_for, SeriesIndex};
use aips_domain::intel::RulEstimate;
use aips_domain::model::{Dataset, Param};
use aips_signal_sys::{self as signal, RulModel};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Trend window used by parameter analytics for the EGT slope rule (cycles).
pub const WINDOW: usize = 120;

pub const FEATURES: [&str; 4] = [
    "margin_c",
    "slope_per_100cyc",
    "cycles_since_sv",
    "resid_sd",
];

pub struct ActiveModel {
    pub version: String,
    pub window: usize,
    pub model: RulModel,
    pub card: Value,
    pub path: PathBuf,
}

impl ActiveModel {
    pub fn label(&self) -> String {
        format!(
            "rul-qr-{} (ONNX Runtime 1.20, window {} cycles)",
            self.version, self.window
        )
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RegistryEntry {
    pub version: String,
    pub path: String,
    pub card: Value,
}

/// Models live in `<dir>/rul-qr-<version>.onnx` with a `.card.json` beside each.
pub fn registry(dir: &Path) -> Vec<RegistryEntry> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            let name = p
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();
            if let Some(v) = name
                .strip_prefix("rul-qr-")
                .and_then(|n| n.strip_suffix(".onnx"))
            {
                let card = std::fs::read_to_string(p.with_extension("card.json"))
                    .ok()
                    .and_then(|t| serde_json::from_str(&t).ok())
                    .unwrap_or(Value::Null);
                out.push(RegistryEntry {
                    version: v.to_string(),
                    path: p.display().to_string(),
                    card,
                });
            }
        }
    }
    // Numeric version order ("1.10.0" after "1.9.0"), so "latest" is really the newest.
    let key = |v: &str| {
        v.split('.')
            .map(|p| p.parse::<u64>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    out.sort_by_key(|e| key(&e.version));
    out
}

pub fn load(entry: &RegistryEntry) -> Result<ActiveModel, String> {
    // Integrity: the artefact must match the hash recorded in its model card.
    let bytes = std::fs::read(&entry.path).map_err(|e| e.to_string())?;
    // Fail closed: a model without a recorded hash is not loaded (bug B19: it used to be accepted).
    let expect = entry.card["onnx_sha256"].as_str().ok_or_else(|| {
        format!(
            "model card for {} has no onnx_sha256; refusing to load",
            entry.version
        )
    })?;
    let digest = sha256_hex(&bytes);
    if digest != expect {
        return Err(format!(
            "model hash mismatch for {} (card {expect}, file {digest})",
            entry.version
        ));
    }
    let model = RulModel::load(&entry.path)?;
    Ok(ActiveModel {
        version: entry.version.clone(),
        // The card is not covered by the artefact hash; the EGT rule reads the last 10 cycles.
        window: (entry.card["window_cycles"].as_u64().unwrap_or(120) as usize).max(10),
        model,
        card: entry.card.clone(),
        path: PathBuf::from(&entry.path),
    })
}

fn sha256_hex(b: &[u8]) -> String {
    aips_domain::audit::sha256_hex(b)
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct FeatureDrift {
    pub feature: String,
    pub psi: f64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Drift {
    pub model_version: String,
    pub rows: usize,
    pub features: Vec<FeatureDrift>,
    pub method: String,
}

/// Population stability index of live inputs against the training deciles in the model card.
pub fn drift(rows: &[[f32; 4]], card: &Value, version: &str) -> Drift {
    let mut features = Vec::new();
    for (i, name) in FEATURES.iter().enumerate() {
        let Some(edges) = card["feature_deciles"][name].as_array() else {
            continue;
        };
        let edges: Vec<f64> = edges.iter().filter_map(|v| v.as_f64()).collect();
        if rows.is_empty() || edges.len() != 9 {
            continue;
        }
        let mut counts = [0usize; 10];
        for r in rows {
            let v = r[i] as f64;
            let bin = edges.iter().position(|e| v <= *e).unwrap_or(9);
            counts[bin] += 1;
        }
        let n = rows.len() as f64;
        let psi: f64 = counts
            .iter()
            .map(|c| {
                let a = (*c as f64 / n).max(1e-4);
                let e = 0.1;
                (a - e) * (a / e).ln()
            })
            .sum();
        let status = if psi < 0.1 {
            "stable"
        } else if psi < 0.25 {
            "moderate shift"
        } else {
            "significant shift: review model applicability"
        };
        features.push(FeatureDrift {
            feature: name.to_string(),
            psi,
            status: status.into(),
        });
    }
    Drift {
        model_version: version.into(),
        rows: rows.len(),
        features,
        method: "PSI vs training deciles (10 bins)".into(),
    }
}

fn residual_sd(v: &[f64], slope: f64) -> f64 {
    let n = v.len() as f64;
    let mean_x = (n - 1.0) / 2.0;
    let mean_y = v.iter().sum::<f64>() / n;
    let intercept = mean_y - slope * mean_x;
    let ss: f64 = v
        .iter()
        .enumerate()
        .map(|(i, y)| (y - (intercept + slope * i as f64)).powi(2))
        .sum();
    (ss / (n - 2.0).max(1.0)).sqrt()
}

pub fn estimate(
    ds: &Dataset,
    idx: &SeriesIndex,
    model: Option<&Arc<ActiveModel>>,
) -> (Vec<RulEstimate>, Drift) {
    let Some(m) = model else {
        return (vec![], Drift::default());
    };
    let window = m.window;
    let mut flights_per_tail: HashMap<&str, u32> = HashMap::new();
    for f in &ds.flights {
        *flights_per_tail.entry(f.tail.as_str()).or_default() += 1;
    }
    let mut rows = Vec::new();
    let mut meta = Vec::new();
    for a in &ds.aircraft {
        for (pos, param) in [
            ("ENG1", Param::EgtMarginEng1),
            ("ENG2", Param::EgtMarginEng2),
        ] {
            let v = idx.values(&a.tail, param);
            if v.len() < window {
                continue;
            }
            let w = &v[v.len() - window..];
            let Ok(f) = signal::extract(w, &config_for(param)) else {
                continue;
            };
            let margin = w[w.len() - 10..].iter().sum::<f64>() / 10.0;
            let comp_id = format!("{}-{pos}", a.tail);
            let csv = ds
                .components
                .iter()
                .find(|c| c.id == comp_id)
                .map(|c| c.cycles_since_sv)
                .unwrap_or(0)
                + flights_per_tail.get(a.tail.as_str()).copied().unwrap_or(0);
            let slope100 = f.slope * 100.0;
            rows.push([
                margin as f32,
                slope100 as f32,
                csv as f32,
                residual_sd(w, f.slope) as f32,
            ]);
            meta.push((
                a.tail.clone(),
                comp_id,
                pos.to_string(),
                margin,
                slope100,
                csv,
            ));
        }
    }
    let d = drift(&rows, &m.card, &m.version);
    let label = m.label();
    let est = match m.model.predict(&rows) {
        Ok(preds) => preds
            .into_iter()
            .zip(meta)
            .map(
                |(q, (tail, component_id, position, margin_c, slope, csv))| RulEstimate {
                    tail,
                    component_id,
                    position,
                    p10: q[0],
                    p50: q[1],
                    p90: q[2],
                    margin_c,
                    slope_per_100cyc: slope,
                    cycles_since_sv: csv,
                    model_version: label.clone(),
                },
            )
            .collect(),
        Err(e) => {
            tracing::error!(?e, "RUL inference failed");
            vec![]
        }
    };
    (est, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn reg() -> Vec<RegistryEntry> {
        registry(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ml/models/registry"))
    }

    #[test]
    fn drift_is_stable_on_training_like_data_and_flags_a_shift() {
        let card = &reg()
            .into_iter()
            .find(|e| e.version == "1.1.0")
            .expect("registry 1.1.0")
            .card;
        // Sample uniformly between the training deciles: PSI must be near zero.
        let mut rows = Vec::new();
        for k in 0..1000 {
            let mut r = [0f32; 4];
            for (i, name) in FEATURES.iter().enumerate() {
                let e: Vec<f64> = card["feature_deciles"][name]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|v| v.as_f64())
                    .collect();
                let bin = k % 10;
                let lo = if bin == 0 {
                    e[0] - (e[1] - e[0]).abs()
                } else {
                    e[bin - 1]
                };
                let hi = if bin == 9 {
                    e[8] + (e[8] - e[7]).abs()
                } else {
                    e[bin]
                };
                r[i] = ((lo + hi) / 2.0) as f32;
            }
            rows.push(r);
        }
        let d = drift(&rows, card, "1.1.0");
        assert_eq!(d.features.len(), 4);
        assert!(d.features.iter().all(|f| f.psi < 0.1), "{:?}", d.features);
        // Shift every input far above the training range: PSI must report a significant shift.
        let shifted: Vec<[f32; 4]> = rows
            .iter()
            .map(|r| [r[0] + 1e4, r[1] + 1e4, r[2] + 1e7, r[3] + 1e4])
            .collect();
        let d = drift(&shifted, card, "1.1.0");
        assert!(
            d.features
                .iter()
                .all(|f| f.psi > 0.25 && f.status.starts_with("significant")),
            "{:?}",
            d.features
        );
    }

    #[test]
    fn registry_loads_hash_verified_versions_and_rejects_tampering() {
        let entries = reg();
        assert!(entries.len() >= 2, "two versions for rollback");
        for e in &entries {
            let m = load(e).expect("hash-verified load");
            assert_eq!(m.version, e.version);
        }
        // Tampered artefact: same card, different bytes -> refused.
        let dir = std::env::temp_dir().join(format!("aips-reg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = &entries[0];
        let mut bytes = std::fs::read(&src.path).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        let p = dir.join(format!("rul-qr-{}.onnx", src.version));
        std::fs::write(&p, bytes).unwrap();
        std::fs::write(
            p.with_extension("card.json"),
            serde_json::to_vec(&src.card).unwrap(),
        )
        .unwrap();
        let bad = registry(&dir).pop().unwrap();
        assert!(load(&bad).is_err(), "tampered model must not load");
        // A card without a hash is refused too (fail closed).
        let mut card = src.card.clone();
        card.as_object_mut().unwrap().remove("onnx_sha256");
        std::fs::write(
            p.with_extension("card.json"),
            serde_json::to_vec(&card).unwrap(),
        )
        .unwrap();
        std::fs::write(&p, std::fs::read(&src.path).unwrap()).unwrap();
        let nohash = registry(&dir).pop().unwrap();
        assert!(load(&nohash)
            .err()
            .is_some_and(|e| e.contains("no onnx_sha256")));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Inference latency benchmark (NFR-01 budget for the RUL step). Run with
    /// `cargo test --release -p aips-core predict::tests::inference_latency -- --nocapture`.
    #[test]
    fn inference_latency() {
        let e = reg().into_iter().find(|e| e.version == "1.1.0").unwrap();
        let m = load(&e).unwrap();
        let mut report = Vec::new();
        for batch in [1usize, 100, 1000] {
            let rows: Vec<[f32; 4]> = (0..batch)
                .map(|i| [20.0 - (i % 15) as f32, 0.3, 1500.0 + i as f32, 1.2])
                .collect();
            let mut t = Vec::new();
            for _ in 0..200 {
                let s = std::time::Instant::now();
                let out = m.model.predict(&rows).unwrap();
                t.push(s.elapsed().as_secs_f64() * 1000.0);
                assert_eq!(out.len(), batch);
                assert!(
                    out.iter().all(|q| q[0] <= q[1] && q[1] <= q[2]),
                    "quantiles ordered"
                );
            }
            t.sort_by(f64::total_cmp);
            let (p50, p99) = (t[100], t[197]);
            report.push(format!(
                "batch {batch:>4}: p50 {p50:.3} ms, p99 {p99:.3} ms"
            ));
            assert!(p99 < 50.0, "batch {batch} p99 {p99} ms over 50 ms budget");
        }
        println!(
            "ONNX Runtime RUL inference latency\n  {}",
            report.join("\n  ")
        );
    }
}
