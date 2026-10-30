// CUDA reduction kernel: per-series mean, sample standard deviation and LSQ slope.
// Compiled only with -DAIPS_ENABLE_CUDA=ON. One block per series; shared-memory tree reduction.
#include "internal.hpp"

#include <cuda_runtime.h>
#include <vector>

namespace {
constexpr int kThreads = 256;

__global__ void moments_kernel(const double* values, const size_t* offsets, double* means,
                               double* stddevs, double* slopes) {
    __shared__ double s_sum[kThreads];
    __shared__ double s_sq[kThreads];
    __shared__ double s_xy[kThreads];
    const size_t series = blockIdx.x;
    const size_t begin = offsets[series];
    const size_t n = offsets[series + 1] - begin;

    double sum = 0.0, sq = 0.0, xy = 0.0;
    for (size_t i = threadIdx.x; i < n; i += blockDim.x) {
        const double v = values[begin + i];
        sum += v;
        sq += v * v;
        xy += static_cast<double>(i) * v;
    }
    s_sum[threadIdx.x] = sum;
    s_sq[threadIdx.x] = sq;
    s_xy[threadIdx.x] = xy;
    __syncthreads();
    for (int stride = blockDim.x / 2; stride > 0; stride >>= 1) {
        if (threadIdx.x < stride) {
            s_sum[threadIdx.x] += s_sum[threadIdx.x + stride];
            s_sq[threadIdx.x] += s_sq[threadIdx.x + stride];
            s_xy[threadIdx.x] += s_xy[threadIdx.x + stride];
        }
        __syncthreads();
    }
    if (threadIdx.x == 0) {
        const double nn = static_cast<double>(n);
        const double mean = n ? s_sum[0] / nn : 0.0;
        const double var = n > 1 ? (s_sq[0] - nn * mean * mean) / (nn - 1.0) : 0.0;
        const double sx = nn * (nn - 1.0) / 2.0;
        const double sxx = (nn - 1.0) * nn * (2.0 * nn - 1.0) / 6.0;
        const double denom = nn * sxx - sx * sx;
        means[series] = mean;
        stddevs[series] = var > 0.0 ? sqrt(var) : 0.0;
        slopes[series] = denom == 0.0 ? 0.0 : (nn * s_xy[0] - sx * s_sum[0]) / denom;
    }
}
}  // namespace

namespace aips::detail {

bool cuda_device_available() {
    int count = 0;
    return cudaGetDeviceCount(&count) == cudaSuccess && count > 0;
}

bool cuda_batch_moments(const double* values, const size_t* offsets, size_t n_series,
                        double* means, double* stddevs, double* slopes) {
    const size_t total = offsets[n_series];
    double *d_values = nullptr, *d_means = nullptr, *d_std = nullptr, *d_slopes = nullptr;
    size_t* d_offsets = nullptr;
    bool ok = cudaMalloc(&d_values, total * sizeof(double)) == cudaSuccess &&
              cudaMalloc(&d_offsets, (n_series + 1) * sizeof(size_t)) == cudaSuccess &&
              cudaMalloc(&d_means, n_series * sizeof(double)) == cudaSuccess &&
              cudaMalloc(&d_std, n_series * sizeof(double)) == cudaSuccess &&
              cudaMalloc(&d_slopes, n_series * sizeof(double)) == cudaSuccess;
    if (ok) {
        cudaMemcpy(d_values, values, total * sizeof(double), cudaMemcpyHostToDevice);
        cudaMemcpy(d_offsets, offsets, (n_series + 1) * sizeof(size_t), cudaMemcpyHostToDevice);
        moments_kernel<<<static_cast<unsigned>(n_series), kThreads>>>(d_values, d_offsets, d_means,
                                                                      d_std, d_slopes);
        ok = cudaDeviceSynchronize() == cudaSuccess;
        if (ok) {
            cudaMemcpy(means, d_means, n_series * sizeof(double), cudaMemcpyDeviceToHost);
            cudaMemcpy(stddevs, d_std, n_series * sizeof(double), cudaMemcpyDeviceToHost);
            cudaMemcpy(slopes, d_slopes, n_series * sizeof(double), cudaMemcpyDeviceToHost);
        }
    }
    cudaFree(d_values);
    cudaFree(d_offsets);
    cudaFree(d_means);
    cudaFree(d_std);
    cudaFree(d_slopes);
    return ok;
}

}  // namespace aips::detail
