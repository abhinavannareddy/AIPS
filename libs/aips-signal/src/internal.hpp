#pragma once
#include "aips_signal.h"

namespace aips::detail {
int extract(const double* values, size_t n, const aips_detector_config& cfg,
            aips_series_features& out);

#ifdef AIPS_ENABLE_CUDA
// Returns true if the GPU computed moments for all series (means/stddevs/slopes filled).
bool cuda_batch_moments(const double* values, const size_t* offsets, size_t n_series,
                        double* means, double* stddevs, double* slopes);
bool cuda_device_available();
#endif
}  // namespace aips::detail
