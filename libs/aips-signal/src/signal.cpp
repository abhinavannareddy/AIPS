// AIPS Signal: per-series feature extraction.
// Detectors: EWMA deviation, two-sided CUSUM drift, frozen-sensor, physical range, LSQ slope.
#include "aips_signal.h"
#include "internal.hpp"

#include <algorithm>
#include <cmath>
#include <limits>

namespace aips::detail {

namespace {
struct Moments {
    double mean = 0.0;
    double stddev = 0.0;
};

// Welford's algorithm: numerically stable single pass. Non-finite samples are skipped (they are
// counted as out of range by the caller); one NaN would otherwise poison mean and sigma and
// silently disable CUSUM drift detection.
Moments moments(const double* v, size_t n) {
    Moments m;
    double mean = 0.0, m2 = 0.0;
    size_t k = 0;
    for (size_t i = 0; i < n; ++i) {
        if (!std::isfinite(v[i])) continue;
        ++k;
        const double delta = v[i] - mean;
        mean += delta / static_cast<double>(k);
        m2 += delta * (v[i] - mean);
    }
    if (k == 0) return m;
    m.mean = mean;
    m.stddev = k > 1 ? std::sqrt(m2 / static_cast<double>(k - 1)) : 0.0;
    return m;
}

double lsq_slope(const double* v, size_t n) {
    if (n < 2) return 0.0;
    // x = 0..n-1; closed-form sums avoid a second pass over x.
    const double nn = static_cast<double>(n);
    const double sx = nn * (nn - 1.0) / 2.0;
    const double sxx = (nn - 1.0) * nn * (2.0 * nn - 1.0) / 6.0;
    double sy = 0.0, sxy = 0.0;
    for (size_t i = 0; i < n; ++i) {
        sy += v[i];
        sxy += static_cast<double>(i) * v[i];
    }
    const double denom = nn * sxx - sx * sx;
    return denom == 0.0 ? 0.0 : (nn * sxy - sx * sy) / denom;
}
}  // namespace

int extract(const double* values, size_t n, const aips_detector_config& cfg,
            aips_series_features& out) {
    out = aips_series_features{};
    out.cusum_alarm_index = -1;
    if (values == nullptr) return AIPS_ERR_INVALID_ARGUMENT;
    // Written as a positive range test so NaN is rejected too.
    if (!(cfg.ewma_alpha > 0.0 && cfg.ewma_alpha <= 1.0)) return AIPS_ERR_INVALID_ARGUMENT;
    const size_t min_len = std::max<size_t>(4, cfg.baseline_samples / 2);
    if (n < min_len) return AIPS_ERR_TOO_SHORT;

    out.n = static_cast<int32_t>(n);
    const Moments all = moments(values, n);
    out.mean = all.mean;
    out.stddev = all.stddev;
    out.slope = lsq_slope(values, n);
    out.last = values[n - 1];
    out.min = std::numeric_limits<double>::infinity();
    out.max = -std::numeric_limits<double>::infinity();

    const size_t base_n = std::min<size_t>(std::max<uint32_t>(cfg.baseline_samples, 4), n);
    const Moments base = moments(values, base_n);
    out.baseline_mean = base.mean;
    // Floor the baseline sigma so a perfectly quiet baseline does not divide by zero.
    const double sigma = std::max(base.stddev, 1e-9 + std::fabs(base.mean) * 1e-6);
    out.baseline_stddev = base.stddev;

    double ewma = base.mean;
    double cpos = 0.0, cneg = 0.0;
    for (size_t i = 0; i < n; ++i) {
        const double x = values[i];
        out.min = std::min(out.min, x);
        out.max = std::max(out.max, x);
        if (x < cfg.range_lo || x > cfg.range_hi || !std::isfinite(x)) out.out_of_range++;

        ewma = cfg.ewma_alpha * x + (1.0 - cfg.ewma_alpha) * ewma;
        if (i >= base_n) {
            const double z = (x - base.mean) / sigma;
            cpos = std::max(0.0, cpos + z - cfg.cusum_k);
            cneg = std::max(0.0, cneg - z - cfg.cusum_k);
            out.cusum_pos = std::max(out.cusum_pos, cpos);
            out.cusum_neg = std::max(out.cusum_neg, cneg);
            if (out.cusum_alarm == 0) {
                if (cpos > cfg.cusum_h) {
                    out.cusum_alarm = 1;
                    out.cusum_alarm_index = static_cast<int32_t>(i);
                } else if (cneg > cfg.cusum_h) {
                    out.cusum_alarm = -1;
                    out.cusum_alarm_index = static_cast<int32_t>(i);
                }
            }
        }
    }
    out.ewma = ewma;
    const double ewma_sigma = sigma * std::sqrt(cfg.ewma_alpha / (2.0 - cfg.ewma_alpha));
    out.ewma_z = (ewma - base.mean) / ewma_sigma;

    if (cfg.frozen_window >= 3 && n >= cfg.frozen_window) {
        const Moments tail = moments(values + (n - cfg.frozen_window), cfg.frozen_window);
        out.frozen = tail.stddev <= cfg.frozen_epsilon ? 1 : 0;
    }
    return AIPS_OK;
}

}  // namespace aips::detail

extern "C" {

aips_detector_config aips_default_config(double range_lo, double range_hi) {
    aips_detector_config c{};
    c.range_lo = range_lo;
    c.range_hi = range_hi;
    c.ewma_alpha = 0.2;
    c.cusum_k = 0.5;
    c.cusum_h = 8.0;
    c.frozen_epsilon = 1e-6;
    c.frozen_window = 12;
    c.baseline_samples = 40;
    return c;
}

int aips_extract_features(const double* values, size_t n, const aips_detector_config* cfg,
                          aips_series_features* out) {
    if (cfg == nullptr || out == nullptr) return AIPS_ERR_INVALID_ARGUMENT;
    return aips::detail::extract(values, n, *cfg, *out);
}

double aips_cross_sensor_disagreement(const double* a, const double* b, size_t n) {
    if (a == nullptr || b == nullptr || n == 0) return 0.0;
    double diff = 0.0, scale = 0.0;
    for (size_t i = 0; i < n; ++i) {
        diff += std::fabs(a[i] - b[i]);
        scale += (std::fabs(a[i]) + std::fabs(b[i])) * 0.5;
    }
    return scale == 0.0 ? 0.0 : diff / scale;
}

const char* aips_signal_version(void) { return "aips-signal 1.0.0"; }

}  // extern "C"
