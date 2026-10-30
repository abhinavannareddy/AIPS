// Unit tests for aips-signal (no external framework; CTest runs the binary).
#include "aips_signal.h"

#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <random>
#include <vector>

static int failures = 0;
#define CHECK(cond)                                                         \
    do {                                                                    \
        if (!(cond)) {                                                      \
            std::fprintf(stderr, "FAIL %s:%d  %s\n", __FILE__, __LINE__, #cond); \
            ++failures;                                                     \
        }                                                                   \
    } while (0)

static std::vector<double> noise(size_t n, double mean, double sd, unsigned seed) {
    std::mt19937 rng(seed);
    std::normal_distribution<double> d(mean, sd);
    std::vector<double> v(n);
    for (auto& x : v) x = d(rng);
    return v;
}

static void test_healthy_series_no_alarm() {
    auto v = noise(200, 3000.0, 15.0, 1);
    auto cfg = aips_default_config(2500.0, 3300.0);
    aips_series_features f{};
    CHECK(aips_extract_features(v.data(), v.size(), &cfg, &f) == AIPS_OK);
    CHECK(f.cusum_alarm == 0);
    CHECK(f.frozen == 0);
    CHECK(f.out_of_range == 0);
    CHECK(std::fabs(f.mean - 3000.0) < 5.0);
}

static void test_drift_detected() {
    auto v = noise(200, 50.0, 1.0, 2);
    for (size_t i = 100; i < v.size(); ++i) v[i] -= 0.08 * static_cast<double>(i - 100);
    auto cfg = aips_default_config(0.0, 100.0);
    aips_series_features f{};
    CHECK(aips_extract_features(v.data(), v.size(), &cfg, &f) == AIPS_OK);
    CHECK(f.cusum_alarm == -1);
    CHECK(f.cusum_alarm_index > 100);
    CHECK(f.slope < 0.0);
}

static void test_frozen_sensor() {
    auto v = noise(100, 3000.0, 20.0, 3);
    for (size_t i = 80; i < v.size(); ++i) v[i] = 2987.5;
    auto cfg = aips_default_config(2000.0, 3500.0);
    aips_series_features f{};
    CHECK(aips_extract_features(v.data(), v.size(), &cfg, &f) == AIPS_OK);
    CHECK(f.frozen == 1);
}

static void test_out_of_range_and_short() {
    std::vector<double> v{1, 2, 3, 400, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20};
    auto cfg = aips_default_config(0.0, 100.0);
    cfg.baseline_samples = 8;
    aips_series_features f{};
    CHECK(aips_extract_features(v.data(), v.size(), &cfg, &f) == AIPS_OK);
    CHECK(f.out_of_range == 1);
    CHECK(aips_extract_features(v.data(), 2, &cfg, &f) == AIPS_ERR_TOO_SHORT);
}

static void test_batch_matches_single() {
    auto a = noise(120, 10.0, 1.0, 4);
    auto b = noise(80, 20.0, 2.0, 5);
    std::vector<double> all(a);
    all.insert(all.end(), b.begin(), b.end());
    std::vector<size_t> offsets{0, a.size(), a.size() + b.size()};
    std::vector<aips_detector_config> cfgs{aips_default_config(0, 100), aips_default_config(0, 100)};
    std::vector<aips_series_features> out(2);
    CHECK(aips_batch_extract(all.data(), offsets.data(), 2, cfgs.data(), out.data()) == AIPS_OK);
    aips_series_features single{};
    aips_extract_features(b.data(), b.size(), &cfgs[1], &single);
    CHECK(std::fabs(single.mean - out[1].mean) < 1e-9);
}

static void test_cross_sensor() {
    std::vector<double> a{100, 101, 99, 100}, b{100, 101, 99, 100}, c{80, 81, 79, 80};
    CHECK(aips_cross_sensor_disagreement(a.data(), b.data(), 4) < 1e-12);
    CHECK(aips_cross_sensor_disagreement(a.data(), c.data(), 4) > 0.2);
}

static void test_rul_model(const char* path) {
    if (!aips_has_onnxruntime()) {
        std::printf("skip RUL test: built without ONNX Runtime\n");
        return;
    }
    char err[256] = {0};
    aips_rul_model* m = aips_rul_load(path, err, sizeof err);
    if (m == nullptr) {
        std::printf("skip RUL test: %s\n", err);
        return;
    }
    // features: margin_c, slope_per_100cyc, cycles_since_sv, resid_sd
    const float healthy[4] = {45.0f, -0.3f, 1500.0f, 1.0f};
    const float worn[4] = {12.0f, -2.5f, 9000.0f, 2.5f};
    float out[3];
    CHECK(aips_rul_predict(m, healthy, 1, 4, out) == AIPS_OK);
    const float healthy_p50 = out[1];
    CHECK(out[0] <= out[1] && out[1] <= out[2]);
    CHECK(aips_rul_predict(m, worn, 1, 4, out) == AIPS_OK);
    CHECK(out[1] < healthy_p50);
    CHECK(aips_rul_predict(m, worn, 1, 3, out) == AIPS_ERR_INVALID_ARGUMENT);
    aips_rul_free(m);
}

// Fuzz-style robustness: random lengths, NaN/Inf, extreme values and odd configurations must
// never crash or read out of bounds (run under ASan/UBSan in CI).
static void test_fuzz_robustness() {
    std::mt19937 rng(42);
    std::uniform_int_distribution<int> len(0, 600);
    std::uniform_real_distribution<double> val(-1e12, 1e12);
    const double specials[] = {std::nan(""), INFINITY, -INFINITY, 0.0, -0.0, 1e308, -1e308};
    for (int it = 0; it < 4000; ++it) {
        std::vector<double> v(static_cast<size_t>(len(rng)));
        for (auto& x : v) x = (rng() % 17 == 0) ? specials[rng() % 7] : val(rng);
        aips_detector_config cfg = aips_default_config(val(rng), val(rng));
        cfg.ewma_alpha = (rng() % 5 == 0) ? -1.0 : std::uniform_real_distribution<double>(0.01, 1.0)(rng);
        cfg.frozen_window = static_cast<uint32_t>(rng() % 700);
        cfg.baseline_samples = static_cast<uint32_t>(rng() % 700);
        aips_series_features f{};
        const int rc = aips_extract_features(v.empty() ? nullptr : v.data(), v.size(), &cfg, &f);
        CHECK(rc == AIPS_OK || rc == AIPS_ERR_TOO_SHORT || rc == AIPS_ERR_INVALID_ARGUMENT);
        if (!v.empty()) (void)aips_cross_sensor_disagreement(v.data(), v.data(), v.size());
    }
    CHECK(aips_extract_features(nullptr, 10, nullptr, nullptr) == AIPS_ERR_INVALID_ARGUMENT);
    CHECK(aips_batch_extract(nullptr, nullptr, 3, nullptr, nullptr) == AIPS_ERR_INVALID_ARGUMENT);
    char err[8];
    CHECK(aips_rul_load(nullptr, err, sizeof err) == nullptr);
    CHECK(aips_rul_load("/nonexistent/model.onnx", err, sizeof err) == nullptr);
}

// Regression: one NaN in the baseline made sigma NaN, so CUSUM silently never alarmed.
static void test_nan_in_baseline_keeps_drift_detection() {
    auto v = noise(200, 50.0, 1.0, 2);
    for (size_t i = 100; i < v.size(); ++i) v[i] -= 0.08 * static_cast<double>(i - 100);
    v[5] = std::nan("");
    auto cfg = aips_default_config(0.0, 100.0);
    aips_series_features f{};
    CHECK(aips_extract_features(v.data(), v.size(), &cfg, &f) == AIPS_OK);
    CHECK(f.cusum_alarm == -1);
    CHECK(f.out_of_range >= 1);
}

// Regression: NaN alpha passed the range check; non-monotonic batch offsets read out of bounds.
static void test_invalid_inputs_rejected() {
    auto v = noise(200, 50.0, 1.0, 9);
    aips_series_features f{};
    auto cfg = aips_default_config(0.0, 100.0);
    cfg.ewma_alpha = std::nan("");
    CHECK(aips_extract_features(v.data(), v.size(), &cfg, &f) == AIPS_ERR_INVALID_ARGUMENT);
    std::vector<size_t> offsets{0, 150, 100};
    std::vector<aips_detector_config> cfgs{aips_default_config(0, 100), aips_default_config(0, 100)};
    std::vector<aips_series_features> out(2);
    CHECK(aips_batch_extract(v.data(), offsets.data(), 2, cfgs.data(), out.data()) == AIPS_ERR_INVALID_ARGUMENT);
}

int main(int argc, char** argv) {
    test_fuzz_robustness();
    test_healthy_series_no_alarm();
    test_drift_detected();
    test_frozen_sensor();
    test_out_of_range_and_short();
    test_batch_matches_single();
    test_cross_sensor();
    test_invalid_inputs_rejected();
    test_nan_in_baseline_keeps_drift_detection();
    test_rul_model(argc > 1 ? argv[1] : "ml/models/rul_qr.onnx");
    std::printf("%s (%d failures), %s, onnxruntime=%d cuda=%d\n", failures ? "FAILED" : "PASSED",
                failures, aips_signal_version(), aips_has_onnxruntime(), aips_has_cuda());
    return failures ? EXIT_FAILURE : EXIT_SUCCESS;
}
