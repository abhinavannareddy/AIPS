/*
 * AIPS Signal: high-performance aircraft parameter analytics (C ABI).
 *
 * Requirement trace: INT-02 (abnormal behaviour), INT-03 (sensor reliability),
 * PRD-01/PRD-02 (degradation trend, RUL inference via ONNX Runtime).
 *
 * The library is advisory-only. It computes features and model outputs; it never
 * produces commands for any aircraft system (PRD section 5, "What AIPS must never do").
 */
#ifndef AIPS_SIGNAL_H
#define AIPS_SIGNAL_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define AIPS_OK 0
#define AIPS_ERR_INVALID_ARGUMENT 1
#define AIPS_ERR_TOO_SHORT 2
#define AIPS_ERR_MODEL 3
#define AIPS_ERR_UNAVAILABLE 4

/* Detector configuration. Units follow the input series. */
typedef struct aips_detector_config {
    double range_lo;          /* physical lower limit; values below are out of range */
    double range_hi;          /* physical upper limit */
    double ewma_alpha;        /* EWMA smoothing factor, 0 < alpha <= 1 */
    double cusum_k;           /* CUSUM slack, in baseline standard deviations */
    double cusum_h;           /* CUSUM decision interval, in baseline standard deviations */
    double frozen_epsilon;    /* standard deviation below which a window counts as frozen */
    uint32_t frozen_window;   /* number of trailing samples checked for a frozen sensor */
    uint32_t baseline_samples;/* leading samples used as the healthy baseline */
} aips_detector_config;

/* Features for one parameter series. */
typedef struct aips_series_features {
    double mean;
    double stddev;
    double baseline_mean;
    double baseline_stddev;
    double ewma;
    double ewma_z;            /* standardised EWMA deviation from baseline */
    double cusum_pos;         /* peak upper CUSUM statistic (in sigma) */
    double cusum_neg;         /* peak lower CUSUM statistic (in sigma) */
    double slope;             /* least-squares slope per sample */
    double min;
    double max;
    double last;
    int32_t frozen;           /* 1 if the trailing window is frozen */
    int32_t out_of_range;     /* count of samples outside [range_lo, range_hi] */
    int32_t cusum_alarm;      /* +1 upward drift, -1 downward drift, 0 none */
    int32_t cusum_alarm_index;/* first sample index where the alarm fired, -1 if none */
    int32_t n;
} aips_series_features;

aips_detector_config aips_default_config(double range_lo, double range_hi);

int aips_extract_features(const double* values, size_t n,
                          const aips_detector_config* cfg,
                          aips_series_features* out);

/* Batch extraction over many concatenated series. offsets has n_series + 1 entries.
 * Uses CUDA for the reduction stage when built with AIPS_ENABLE_CUDA and a device is present,
 * otherwise a CPU thread pool. */
int aips_batch_extract(const double* values, const size_t* offsets, size_t n_series,
                       const aips_detector_config* cfgs, aips_series_features* out);

/* Normalised disagreement between two redundant sensors: mean |a-b| / (mean(|a|+|b|)/2). */
double aips_cross_sensor_disagreement(const double* a, const double* b, size_t n);

/* ---- Remaining useful life inference (ONNX Runtime) ---- */
typedef struct aips_rul_model aips_rul_model;

aips_rul_model* aips_rul_load(const char* model_path, char* err, size_t err_len);
/* features: rows x cols row-major float32. out: rows x 3 (p10, p50, p90), cycles. */
int aips_rul_predict(aips_rul_model* model, const float* features, size_t rows, size_t cols,
                     float* out);
void aips_rul_free(aips_rul_model* model);

const char* aips_signal_version(void);
int aips_has_onnxruntime(void);
int aips_has_cuda(void);

#ifdef __cplusplus
}
#endif

#endif /* AIPS_SIGNAL_H */
