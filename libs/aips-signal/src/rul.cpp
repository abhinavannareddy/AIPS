// Remaining-useful-life inference through ONNX Runtime (PRD-01: RUL with an uncertainty range).
// The model emits three quantiles (p10, p50, p90); callers must never show p50 without the range.
#include "aips_signal.h"

#include <cstring>
#include <memory>
#include <string>
#include <vector>

#ifdef AIPS_WITH_ONNXRUNTIME
#include <onnxruntime_cxx_api.h>
#endif

namespace {
void write_err(char* err, size_t len, const std::string& msg) {
    if (err == nullptr || len == 0) return;
    std::strncpy(err, msg.c_str(), len - 1);
    err[len - 1] = '\0';
}
}  // namespace

struct aips_rul_model {
#ifdef AIPS_WITH_ONNXRUNTIME
    Ort::Env env{ORT_LOGGING_LEVEL_WARNING, "aips-rul"};
    std::unique_ptr<Ort::Session> session;
    std::string input_name;
    std::string output_name;
    size_t expected_cols = 0;
#endif
};

extern "C" {

int aips_has_onnxruntime(void) {
#ifdef AIPS_WITH_ONNXRUNTIME
    return 1;
#else
    return 0;
#endif
}

aips_rul_model* aips_rul_load(const char* model_path, char* err, size_t err_len) {
#ifdef AIPS_WITH_ONNXRUNTIME
    if (model_path == nullptr) {
        write_err(err, err_len, "model path is null");
        return nullptr;
    }
    try {
        auto model = std::make_unique<aips_rul_model>();
        Ort::SessionOptions opts;
        opts.SetIntraOpNumThreads(1);
        opts.SetGraphOptimizationLevel(GraphOptimizationLevel::ORT_ENABLE_ALL);
        model->session = std::make_unique<Ort::Session>(model->env, model_path, opts);
        Ort::AllocatorWithDefaultOptions alloc;
        model->input_name = model->session->GetInputNameAllocated(0, alloc).get();
        model->output_name = model->session->GetOutputNameAllocated(0, alloc).get();
        auto shape = model->session->GetInputTypeInfo(0).GetTensorTypeAndShapeInfo().GetShape();
        if (shape.size() != 2 || shape[1] <= 0) {
            write_err(err, err_len, "model input must be [batch, features]");
            return nullptr;
        }
        model->expected_cols = static_cast<size_t>(shape[1]);
        return model.release();
    } catch (const std::exception& e) {
        write_err(err, err_len, e.what());
        return nullptr;
    }
#else
    (void)model_path;
    write_err(err, err_len, "aips-signal was built without ONNX Runtime");
    return nullptr;
#endif
}

int aips_rul_predict(aips_rul_model* model, const float* features, size_t rows, size_t cols,
                     float* out) {
#ifdef AIPS_WITH_ONNXRUNTIME
    if (model == nullptr || features == nullptr || out == nullptr || rows == 0)
        return AIPS_ERR_INVALID_ARGUMENT;
    if (cols != model->expected_cols) return AIPS_ERR_INVALID_ARGUMENT;
    try {
        auto mem = Ort::MemoryInfo::CreateCpu(OrtArenaAllocator, OrtMemTypeDefault);
        const std::vector<int64_t> shape{static_cast<int64_t>(rows), static_cast<int64_t>(cols)};
        auto input = Ort::Value::CreateTensor<float>(mem, const_cast<float*>(features),
                                                     rows * cols, shape.data(), shape.size());
        const char* in_names[] = {model->input_name.c_str()};
        const char* out_names[] = {model->output_name.c_str()};
        auto result = model->session->Run(Ort::RunOptions{nullptr}, in_names, &input, 1,
                                          out_names, 1);
        const auto info = result[0].GetTensorTypeAndShapeInfo();
        if (info.GetElementCount() != rows * 3) return AIPS_ERR_MODEL;
        std::memcpy(out, result[0].GetTensorData<float>(), rows * 3 * sizeof(float));
        // Enforce monotone quantiles so the uncertainty band is always well-formed.
        for (size_t r = 0; r < rows; ++r) {
            float* q = out + r * 3;
            if (q[0] > q[1]) std::swap(q[0], q[1]);
            if (q[1] > q[2]) std::swap(q[1], q[2]);
            if (q[0] > q[1]) std::swap(q[0], q[1]);
        }
        return AIPS_OK;
    } catch (...) {
        return AIPS_ERR_MODEL;
    }
#else
    (void)model; (void)features; (void)rows; (void)cols; (void)out;
    return AIPS_ERR_UNAVAILABLE;
#endif
}

void aips_rul_free(aips_rul_model* model) { delete model; }

}  // extern "C"
