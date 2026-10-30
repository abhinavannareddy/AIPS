import { num, utc } from '../format';
import type { RulEstimate } from '../types';

interface SeriesResp {
  label: string; unit: string; lo: number; hi: number;
  points: { ts: string; value: number; flight_id: string }[];
  features: { baseline_mean: number; baseline_stddev: number; cusum_alarm: number; cusum_alarm_index: number; frozen: boolean; ewma_z: number } | null;
  model_version: string;
}

/** Parameter history with the healthy baseline band (±3σ) and where a drift alarm fired. */
export function SeriesChart({ s }: { s: SeriesResp }) {
  const W = 760, H = 220, L = 54, R = 12, T = 12, B = 28;
  const v = s.points.map((p) => p.value);
  if (v.length < 2) return <p className="footnote">Not enough data yet.</p>;
  const f = s.features;
  const band = f ? [f.baseline_mean - 3 * f.baseline_stddev, f.baseline_mean + 3 * f.baseline_stddev] : null;
  let min = Math.min(...v, band ? band[0] : Infinity);
  let max = Math.max(...v, band ? band[1] : -Infinity);
  const pad = (max - min) * 0.08 || 1;
  min -= pad; max += pad;
  const x = (i: number) => L + (i / (v.length - 1)) * (W - L - R);
  const y = (val: number) => T + (1 - (val - min) / (max - min)) * (H - T - B);
  const path = v.map((val, i) => `${i ? 'L' : 'M'}${x(i).toFixed(1)},${y(val).toFixed(1)}`).join('');
  const ticks = [min + pad, (min + max) / 2, max - pad];
  const alarmIdx = f && f.cusum_alarm !== 0 ? f.cusum_alarm_index : -1;
  const baselineEnd = Math.min(120, v.length - 1);
  return (
    <figure className="chart">
      <svg viewBox={`0 0 ${W} ${H}`} role="img" aria-label={`${s.label} over the last ${v.length} flights`}>
        {band && <rect x={L} width={W - L - R} y={y(band[1])} height={Math.max(0, y(band[0]) - y(band[1]))} className="chart-band" />}
        <line x1={x(baselineEnd)} x2={x(baselineEnd)} y1={T} y2={H - B} className="chart-baseline-end" />
        {ticks.map((t) => (
          <g key={t}><line x1={L} x2={W - R} y1={y(t)} y2={y(t)} className="chart-grid" /><text x={L - 6} y={y(t) + 4} textAnchor="end">{num(t, Math.abs(t) < 20 ? 1 : 0)}</text></g>
        ))}
        <path d={path} className="chart-line" />
        {alarmIdx >= 0 && (
          <g className="chart-alarm"><line x1={x(alarmIdx)} x2={x(alarmIdx)} y1={T} y2={H - B} /><text x={x(alarmIdx) + 4} y={T + 12}>Drift alarm</text></g>
        )}
        <text x={L} y={H - 8}>{utc(s.points[0].ts)}</text>
        <text x={W - R} y={H - 8} textAnchor="end">{utc(s.points[s.points.length - 1].ts)}</text>
      </svg>
      <figcaption>
        Shaded band: healthy baseline ±3σ from the first {baselineEnd + 1} flights. {f?.frozen ? 'Trailing readings are frozen. ' : ''}
        Unit: {s.unit}. Model: {s.model_version}.
      </figcaption>
    </figure>
  );
}
export type { SeriesResp };

/** Remaining useful life as a range: the p10–p90 bar with the p50 tick. Never a single number. */
export function RulBar({ r, scaleMax }: { r: RulEstimate; scaleMax: number }) {
  const pct = (n: number) => `${Math.min(100, (n / scaleMax) * 100)}%`;
  const low = r.p10 < 800;
  return (
    <div className={`rul ${low ? 'rul-low' : ''}`}>
      <div className="rul-head">
        <strong>Engine {r.position.replace('ENG', '')}</strong>
        <span className="tabular">p10 {num(r.p10)} · p50 {num(r.p50)} · p90 {num(r.p90)} cycles</span>
      </div>
      <div className="rul-track" aria-hidden>
        <span className="rul-review" style={{ left: pct(800) }} title="800-cycle review threshold" />
        <span className="rul-range" style={{ left: pct(r.p10), width: `calc(${pct(r.p90)} - ${pct(r.p10)})` }} />
        <span className="rul-mid" style={{ left: pct(r.p50) }} />
      </div>
      <p className="footnote">EGT margin {num(r.margin_c, 1)} °C, trend {num(r.slope_per_100cyc, 2)} °C per 100 cycles. Estimate for planning only.</p>
    </div>
  );
}
