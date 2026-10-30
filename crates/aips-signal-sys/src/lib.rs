//! Safe Rust bindings to the AIPS C++ signal library (aips_signal.h).
use parking_lot::Mutex;
use std::ffi::{c_char, c_int, CStr, CString};

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DetectorConfig {
    pub range_lo: f64,
    pub range_hi: f64,
    pub ewma_alpha: f64,
    pub cusum_k: f64,
    pub cusum_h: f64,
    pub frozen_epsilon: f64,
    pub frozen_window: u32,
    pub baseline_samples: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Features {
    pub mean: f64,
    pub stddev: f64,
    pub baseline_mean: f64,
    pub baseline_stddev: f64,
    pub ewma: f64,
    pub ewma_z: f64,
    pub cusum_pos: f64,
    pub cusum_neg: f64,
    pub slope: f64,
    pub min: f64,
    pub max: f64,
    pub last: f64,
    pub frozen: i32,
    pub out_of_range: i32,
    pub cusum_alarm: i32,
    pub cusum_alarm_index: i32,
    pub n: i32,
}

#[repr(C)]
struct RawModel {
    _private: [u8; 0],
}

extern "C" {
    fn aips_default_config(lo: f64, hi: f64) -> DetectorConfig;
    fn aips_extract_features(
        v: *const f64,
        n: usize,
        cfg: *const DetectorConfig,
        out: *mut Features,
    ) -> c_int;
    fn aips_batch_extract(
        v: *const f64,
        offsets: *const usize,
        n_series: usize,
        cfgs: *const DetectorConfig,
        out: *mut Features,
    ) -> c_int;
    fn aips_cross_sensor_disagreement(a: *const f64, b: *const f64, n: usize) -> f64;
    fn aips_rul_load(path: *const c_char, err: *mut c_char, err_len: usize) -> *mut RawModel;
    fn aips_rul_predict(
        m: *mut RawModel,
        f: *const f32,
        rows: usize,
        cols: usize,
        out: *mut f32,
    ) -> c_int;
    fn aips_rul_free(m: *mut RawModel);
    fn aips_signal_version() -> *const c_char;
    fn aips_has_onnxruntime() -> c_int;
    fn aips_has_cuda() -> c_int;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignalError {
    InvalidArgument,
    TooShort,
    Model,
    Unavailable,
    Other(i32),
}

fn rc(code: c_int) -> Result<(), SignalError> {
    match code {
        0 => Ok(()),
        1 => Err(SignalError::InvalidArgument),
        2 => Err(SignalError::TooShort),
        3 => Err(SignalError::Model),
        4 => Err(SignalError::Unavailable),
        x => Err(SignalError::Other(x)),
    }
}

impl DetectorConfig {
    pub fn default_for(lo: f64, hi: f64) -> Self {
        // SAFETY: pure function returning a POD struct.
        unsafe { aips_default_config(lo, hi) }
    }
}

pub fn extract(values: &[f64], cfg: &DetectorConfig) -> Result<Features, SignalError> {
    let mut out = Features::default();
    // SAFETY: pointers are valid for the given lengths for the duration of the call.
    rc(unsafe { aips_extract_features(values.as_ptr(), values.len(), cfg, &mut out) })?;
    Ok(out)
}

/// Batch extraction; series shorter than the minimum length yield default features with n = 0.
pub fn extract_batch(
    series: &[&[f64]],
    cfgs: &[DetectorConfig],
) -> Result<Vec<Features>, SignalError> {
    if series.len() != cfgs.len() {
        return Err(SignalError::InvalidArgument);
    }
    let mut flat = Vec::with_capacity(series.iter().map(|s| s.len()).sum());
    let mut offsets = Vec::with_capacity(series.len() + 1);
    offsets.push(0usize);
    for s in series {
        flat.extend_from_slice(s);
        offsets.push(flat.len());
    }
    let mut out = vec![Features::default(); series.len()];
    if series.is_empty() {
        return Ok(out);
    }
    // SAFETY: offsets has n+1 entries indexing into flat; out has n entries.
    rc(unsafe {
        aips_batch_extract(
            flat.as_ptr(),
            offsets.as_ptr(),
            series.len(),
            cfgs.as_ptr(),
            out.as_mut_ptr(),
        )
    })?;
    Ok(out)
}

pub fn cross_sensor_disagreement(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len().min(b.len());
    // SAFETY: both slices hold at least n elements.
    unsafe { aips_cross_sensor_disagreement(a.as_ptr(), b.as_ptr(), n) }
}

pub fn version() -> String {
    // SAFETY: returns a static NUL-terminated string.
    unsafe { CStr::from_ptr(aips_signal_version()) }
        .to_string_lossy()
        .into_owned()
}

pub fn has_onnxruntime() -> bool {
    unsafe { aips_has_onnxruntime() == 1 }
}

pub fn has_cuda() -> bool {
    unsafe { aips_has_cuda() == 1 }
}

/// ONNX Runtime RUL model. ORT sessions are thread-safe, but calls are serialised to bound memory.
pub struct RulModel {
    raw: Mutex<*mut RawModel>,
}

// SAFETY: the underlying ORT session is thread-safe and access is serialised by the mutex.
unsafe impl Send for RulModel {}
unsafe impl Sync for RulModel {}

impl RulModel {
    pub fn load(path: &str) -> Result<Self, String> {
        let c = CString::new(path).map_err(|e| e.to_string())?;
        let mut err = vec![0 as c_char; 512];
        // SAFETY: valid C string and error buffer.
        let raw = unsafe { aips_rul_load(c.as_ptr(), err.as_mut_ptr(), err.len()) };
        if raw.is_null() {
            let msg = unsafe { CStr::from_ptr(err.as_ptr()) }
                .to_string_lossy()
                .into_owned();
            return Err(msg);
        }
        Ok(Self {
            raw: Mutex::new(raw),
        })
    }

    /// rows of [margin_c, slope_per_100cyc, cycles_since_sv, resid_sd] -> [p10, p50, p90]
    pub fn predict(&self, rows: &[[f32; 4]]) -> Result<Vec<[f32; 3]>, SignalError> {
        if rows.is_empty() {
            return Ok(vec![]);
        }
        let flat: Vec<f32> = rows.iter().flatten().copied().collect();
        let mut out = vec![0f32; rows.len() * 3];
        let guard = self.raw.lock();
        // SAFETY: model pointer valid while self lives; buffers sized rows*4 and rows*3.
        rc(unsafe { aips_rul_predict(*guard, flat.as_ptr(), rows.len(), 4, out.as_mut_ptr()) })?;
        Ok(out.chunks(3).map(|c| [c[0], c[1], c[2]]).collect())
    }
}

impl Drop for RulModel {
    fn drop(&mut self) {
        // SAFETY: pointer came from aips_rul_load and is freed once.
        unsafe { aips_rul_free(*self.raw.lock()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ffi_roundtrip() {
        let v: Vec<f64> = (0..100)
            .map(|i| 50.0 + ((i * 37) % 7) as f64 * 0.1)
            .collect();
        let cfg = DetectorConfig::default_for(0.0, 100.0);
        let f = extract(&v, &cfg).unwrap();
        assert_eq!(f.n, 100);
        assert_eq!(f.out_of_range, 0);
        assert!(version().starts_with("aips-signal"));
        let b = extract_batch(&[&v, &v[..50]], &[cfg, cfg]).unwrap();
        assert_eq!(b.len(), 2);
    }
}
