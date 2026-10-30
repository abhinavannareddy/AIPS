"""AIPS Predict: remaining-useful-life quantile model (PRD-01, NFR-10).

Trains three linear quantile heads (p10, p50, p90) on synthetic engine EGT-margin
run-to-removal data, exports a single ONNX graph with a physics-informed feature
computed *inside* the graph, and writes a model card with lineage and evaluation.

Inputs  (float32[N,4]): margin_c, slope_per_100cyc, cycles_since_sv, resid_sd
Outputs (float32[N,3]): rul_p10, rul_p50, rul_p90 (cycles, clamped >= 0)

Synthetic data only (Phase 0). Python is used for training and evaluation only;
production inference runs in C++ via ONNX Runtime.
"""
from __future__ import annotations

import hashlib
import json
import pathlib
import argparse
import sys

import numpy as np
import onnx
from onnx import TensorProto, helper, numpy_helper

SEED = 20261030
REMOVAL_MARGIN_C = 5.0
QUANTILES = (0.10, 0.50, 0.90)
WINDOW = 120  # default trend window (cycles); recorded in the model card and read by aips-core
MIN_WEAR = 0.25  # degC per 100 cycles; floor for the wear-rate estimate
ROWS_PER_ENGINE = 12  # training windows sampled per synthetic engine
OUT = pathlib.Path(__file__).parent / "models"


def synthesize(rng: np.random.Generator, engines: int, window: int = WINDOW) -> tuple[np.ndarray, np.ndarray]:
    rows, targets = [], []
    for _ in range(engines):
        margin0 = rng.uniform(35, 60)
        # Mostly healthy wear, plus ~20% fast-deteriorating engines (abnormal wear modes).
        rate = rng.lognormal(mean=np.log(0.9), sigma=0.6) if rng.random() > 0.2 else rng.uniform(3.0, 14.0)
        noise = rng.uniform(0.6, 2.5)
        life = int((margin0 - REMOVAL_MARGIN_C) / rate * 100)
        cycles = np.arange(life)
        accel = 1 + 0.35 * (cycles / max(life, 1)) ** 2  # mild end-of-life acceleration
        margin = margin0 - rate * cycles / 100 * accel + rng.normal(0, noise, life)
        if life <= window + 2:
            continue
        for t in rng.integers(window, life - 1, size=ROWS_PER_ENGINE):
            w = margin[t - window : t]
            x = np.arange(window)
            slope, intercept = np.polyfit(x, w, 1)
            resid = w - (slope * x + intercept)
            rows.append([w[-10:].mean(), slope * 100, t, resid.std()])
            targets.append(life - t)
    return np.asarray(rows, np.float64), np.asarray(targets, np.float64)


def projection(x: np.ndarray) -> np.ndarray:
    """Physics prior: cycles until the margin trend reaches the removal margin."""
    return np.maximum(x[:, 0] - REMOVAL_MARGIN_C, 0.5) / np.maximum(-x[:, 1], MIN_WEAR) * 100


def design(x: np.ndarray) -> np.ndarray:
    """Mirror of the in-graph features: [1, log(projection), resid_sd, margin/50]."""
    return np.column_stack([np.ones(len(x)), np.log(projection(x)), x[:, 3], x[:, 0] / 50])


def fit_quantile(z: np.ndarray, y: np.ndarray, q: float, iters: int = 4000) -> np.ndarray:
    """Pinball-loss minimisation with Adam; column 0 of z is the intercept."""
    mu, sd = z[:, 1:].mean(0), z[:, 1:].std(0) + 1e-9
    zs = np.column_stack([z[:, 0], (z[:, 1:] - mu) / sd])
    w = np.zeros(z.shape[1])
    m = v = np.zeros_like(w)
    lr, b1, b2 = 0.05, 0.9, 0.999
    for i in range(1, iters + 1):
        r = y - zs @ w
        g = -zs.T @ np.where(r > 0, q, q - 1) / len(y)
        m, v = b1 * m + (1 - b1) * g, b2 * v + (1 - b2) * g * g
        w -= lr * (m / (1 - b1**i)) / (np.sqrt(v / (1 - b2**i)) + 1e-8)
    # Fold standardisation back into raw-feature weights.
    raw = np.empty_like(w)
    raw[1:] = w[1:] / sd
    raw[0] = w[0] - np.sum(w[1:] * mu / sd)
    return raw


def export(weights: np.ndarray, path: pathlib.Path) -> None:
    c = lambda name, val: numpy_helper.from_array(np.asarray(val, np.float32), name)
    nodes = [
        helper.make_node("Slice", ["x", "s0", "s1", "ax"], ["margin"]),
        helper.make_node("Slice", ["x", "s1", "s2", "ax"], ["slope"]),
        helper.make_node("Slice", ["x", "s2", "s3", "ax"], ["cycles"]),
        helper.make_node("Slice", ["x", "s3", "s4", "ax"], ["resid"]),
        helper.make_node("Sub", ["margin", "removal"], ["headroom_raw"]),
        helper.make_node("Max", ["headroom_raw", "min_headroom"], ["headroom"]),
        helper.make_node("Neg", ["slope"], ["wear"]),
        helper.make_node("Max", ["wear", "min_wear"], ["wear_c"]),
        helper.make_node("Div", ["headroom", "wear_c"], ["ratio"]),
        helper.make_node("Mul", ["ratio", "hundred"], ["proj"]),
        helper.make_node("Div", ["cycles", "tenthousand"], ["cyc_k"]),
        helper.make_node("Div", ["margin", "fifty"], ["margin_s"]),
        helper.make_node("ConstantOfShape", ["batch_shape"], ["ones"], value=c("one", [1.0])),
        helper.make_node("Shape", ["margin"], ["batch_shape"]),
        helper.make_node("Log", ["proj"], ["log_proj"]),
        helper.make_node("Concat", ["ones", "log_proj", "resid", "margin_s"], ["z"], axis=1),
        helper.make_node("MatMul", ["z", "W"], ["log_rul"]),
        helper.make_node("Exp", ["log_rul"], ["rul"]),
    ]
    i64 = lambda name, val: numpy_helper.from_array(np.asarray(val, np.int64), name)
    inits = [
        i64("s0", [0]), i64("s1", [1]), i64("s2", [2]), i64("s3", [3]), i64("s4", [4]), i64("ax", [1]),
        c("removal", [REMOVAL_MARGIN_C]), c("min_wear", [MIN_WEAR]), c("hundred", [100.0]),
        c("tenthousand", [10000.0]), c("fifty", [50.0]), c("min_headroom", [0.5]), c("W", weights.T),
    ]
    # Topological order: Shape must precede ConstantOfShape.
    nodes.insert(12, nodes.pop(13))
    graph = helper.make_graph(
        nodes, "aips_rul_quantile",
        [helper.make_tensor_value_info("x", TensorProto.FLOAT, ["N", 4])],
        [helper.make_tensor_value_info("rul", TensorProto.FLOAT, ["N", 3])],
        inits,
    )
    model = helper.make_model(graph, opset_imports=[helper.make_opsetid("", 17)],
                              producer_name="aips-ml", producer_version="1.0.0")
    model.ir_version = 9
    onnx.checker.check_model(model)
    onnx.save(model, path)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--version", default="1.1.0")
    ap.add_argument("--window", type=int, default=WINDOW)
    ap.add_argument("--registry-only", action="store_true", help="write only to models/registry")
    args = ap.parse_args()
    rng = np.random.default_rng(SEED)
    x, y = synthesize(rng, engines=900, window=args.window)
    # Split by engine, not by row: each engine contributes ROWS_PER_ENGINE overlapping windows,
    # so a row split puts the same trajectory in train and test and flatters the evaluation.
    engine = np.arange(len(x)) // ROWS_PER_ENGINE
    held_out = rng.permutation(engine.max() + 1)[int(0.8 * (engine.max() + 1)):]
    te_mask = np.isin(engine, held_out)
    tr, te = np.flatnonzero(~te_mask), np.flatnonzero(te_mask)
    split = len(tr)
    z = design(x)
    logy = np.log(np.maximum(y, 1.0))
    weights = np.stack([fit_quantile(z[tr], logy[tr], q) for q in QUANTILES])
    pred = np.exp(design(x[te]) @ weights.T)
    pred.sort(axis=1)
    mae = float(np.mean(np.abs(pred[:, 1] - y[te])))
    coverage = float(np.mean((y[te] >= pred[:, 0]) & (y[te] <= pred[:, 2])))
    # Gate before writing anything: a failing model must never replace the shipped artefacts.
    if not 0.65 <= coverage <= 0.95:
        print(json.dumps({"gate": "failed", "p10_p90_coverage": round(coverage, 3), "p50_mae_cycles": round(mae, 1)}))
        return 1
    reg = OUT / "registry"
    reg.mkdir(parents=True, exist_ok=True)
    model_path = reg / f"rul-qr-{args.version}.onnx"
    export(weights, model_path)
    data_hash = hashlib.sha256(x.tobytes() + y.tobytes()).hexdigest()
    card = {
        "model": "rul-qr", "version": args.version, "window_cycles": args.window, "requirement": ["PRD-01", "NFR-10", "NFR-11"],
        "inputs": ["margin_c", "slope_per_100cyc", "cycles_since_sv", "resid_sd"],
        "outputs": ["rul_p10", "rul_p50", "rul_p90"], "units": "cycles",
        "training_data": {"kind": "synthetic", "seed": SEED, "rows": int(split), "sha256": data_hash},
        "method": "log-space linear quantile regression on a physics projection (margin headroom / wear rate)",
        "evaluation": {"holdout_rows": int(len(te)), "p50_mae_cycles": round(mae, 1),
                       "p50_mape_fast_wear": round(float(np.median(np.abs(pred[:, 1] - y[te])[x[te][:, 1] < -3] / np.maximum(y[te][x[te][:, 1] < -3], 1))), 3),
                       "p10_p90_coverage": round(coverage, 3), "target_coverage": 0.80},
        "limitations": "Synthetic EGT-margin degradation only; not validated on operational data.",
        "approval": {"status": "pending", "required_role": "safety_lead"},
        "onnx_sha256": hashlib.sha256(model_path.read_bytes()).hexdigest(),
        # Training-distribution deciles per input feature: the reference for drift (PSI) checks.
        "feature_deciles": {name: np.quantile(x[tr][:, i], np.linspace(0.1, 0.9, 9)).round(4).tolist()
                            for i, name in enumerate(["margin_c", "slope_per_100cyc", "cycles_since_sv", "resid_sd"])},
    }
    (reg / f"rul-qr-{args.version}.card.json").write_text(json.dumps(card, indent=2))
    if not args.registry_only:
        (OUT / "rul_qr.onnx").write_bytes(model_path.read_bytes())
        (OUT / "rul_qr.card.json").write_text(json.dumps(card, indent=2))
    print(json.dumps(card["evaluation"]))
    return 0


if __name__ == "__main__":
    sys.exit(main())
