// Batch feature extraction across many series on a CPU thread pool,
// with an optional CUDA reduction stage (PRD tech stack: "CUDA only where profiling justifies it").
#include "aips_signal.h"
#include "internal.hpp"

#include <algorithm>
#include <atomic>
#include <thread>
#include <vector>

extern "C" int aips_batch_extract(const double* values, const size_t* offsets, size_t n_series,
                                  const aips_detector_config* cfgs, aips_series_features* out) {
    if (values == nullptr || offsets == nullptr || cfgs == nullptr || out == nullptr)
        return AIPS_ERR_INVALID_ARGUMENT;
    if (n_series == 0) return AIPS_OK;
    // Public C ABI: non-monotonic offsets would underflow a length and read out of bounds.
    for (size_t i = 0; i < n_series; ++i)
        if (offsets[i + 1] < offsets[i]) return AIPS_ERR_INVALID_ARGUMENT;

    std::atomic<int> first_error{AIPS_OK};
    std::atomic<size_t> next{0};
    const unsigned hw = std::max(1u, std::thread::hardware_concurrency());
    const unsigned workers = static_cast<unsigned>(std::min<size_t>(hw, n_series));

    auto run = [&]() {
        for (;;) {
            const size_t i = next.fetch_add(1, std::memory_order_relaxed);
            if (i >= n_series) return;
            const size_t begin = offsets[i];
            const size_t len = offsets[i + 1] - begin;
            const int rc = aips::detail::extract(values + begin, len, cfgs[i], out[i]);
            if (rc != AIPS_OK && rc != AIPS_ERR_TOO_SHORT) {
                int expected = AIPS_OK;
                first_error.compare_exchange_strong(expected, rc);
            }
        }
    };

    std::vector<std::thread> pool;
    pool.reserve(workers);
    for (unsigned w = 0; w < workers; ++w) pool.emplace_back(run);
    for (auto& t : pool) t.join();

#ifdef AIPS_ENABLE_CUDA
    // Cross-check: the GPU path recomputes whole-series moments for large batches.
    if (n_series >= 1024 && aips::detail::cuda_device_available()) {
        std::vector<double> m(n_series), s(n_series), sl(n_series);
        if (aips::detail::cuda_batch_moments(values, offsets, n_series, m.data(), s.data(),
                                              sl.data())) {
            for (size_t i = 0; i < n_series; ++i) {
                out[i].mean = m[i];
                out[i].stddev = s[i];
                out[i].slope = sl[i];
            }
        }
    }
#endif
    return first_error.load();
}

extern "C" int aips_has_cuda(void) {
#ifdef AIPS_ENABLE_CUDA
    return aips::detail::cuda_device_available() ? 1 : 0;
#else
    return 0;
#endif
}
