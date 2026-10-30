import { useState } from 'react';
import { api } from '../api';
import { Empty, ErrorBox, Loading, Notice, Panel } from '../components/ui';
import { num } from '../format';
import { can, useAction, useApi, usePrincipal } from '../hooks';
import type { MetricsResponse, ModelRegistry } from '../types';

/** NFR-10: drift against training distribution, version registry and administrator rollback. */
function ModelPanel() {
  const p = usePrincipal();
  const reg = useApi<ModelRegistry>('/api/models/rul');
  const act = useAction();
  const [reason, setReason] = useState('');
  const activate = (version: string) => act.run(async () => {
    await api('/api/models/rul/activate', { method: 'POST', body: { version, reason } });
    setReason(''); reg.reload();
  });
  if (reg.error) return <ErrorBox message={reg.error} onRetry={reg.reload} />;
  if (!reg.data) return <Loading label="Reading model registry" />;
  const d = reg.data;
  const admin = can(p, 'admin') && p?.roles.includes('admin');
  return (
    <Panel title="Remaining-life model: drift and versions" aside={d.active ? `Active ${d.active}` : 'No model active'}>
      <div className="grid-2">
        <div>
          <h3 className="sub">Input drift ({d.drift.method || 'PSI'}, {d.drift.rows} live rows)</h3>
          {d.drift.features.length ? (
            <table className="table compact">
              <thead><tr><th>Feature</th><th className="num">PSI</th><th>Status</th></tr></thead>
              <tbody>{d.drift.features.map((f) => (
                <tr key={f.feature}><td>{f.feature}</td><td className="num tabular">{f.psi.toFixed(3)}</td>
                  <td className={f.status === 'stable' ? 'ok' : f.status.startsWith('moderate') ? '' : 'err'}>{f.status}</td></tr>
              ))}</tbody>
            </table>
          ) : <Empty>No live inputs to compare yet.</Empty>}
          <p className="footnote">PSI below 0.10 is stable, 0.10–0.25 moderate, above 0.25 significant. Significant drift means estimates should be treated with extra caution and the model reviewed.</p>
        </div>
        <div>
          <h3 className="sub">Registry</h3>
          <table className="table compact">
            <thead><tr><th>Version</th><th>Window</th><th>Evaluation</th><th /></tr></thead>
            <tbody>{d.registry.map((m) => (
              <tr key={m.version}>
                <td><strong>{m.version}</strong>{m.active && <small className="ok"> active</small>}</td>
                <td className="tabular">{m.window_cycles ?? '—'} cycles</td>
                <td><small>{m.evaluation ? Object.entries(m.evaluation).filter(([k, v]) => typeof v === 'number' && k !== 'holdout_rows' && k !== 'target_coverage').slice(0, 3).map(([k, v]) => `${k} ${(v as number).toFixed(2)}`).join(', ') : '—'}</small></td>
                <td>{admin && !m.active && <button className="btn btn-small btn-quiet" disabled={act.pending || reason.trim().length < 5} onClick={() => activate(m.version)}>Activate</button>}</td>
              </tr>
            ))}</tbody>
          </table>
          {admin && <label className="form-col">Reason for change (audited)<input value={reason} onChange={(e) => setReason(e.target.value)} placeholder="e.g. rollback after drift review" /></label>}
          {act.error && <p className="err">{act.error}</p>}
          <p className="footnote">Each artefact is verified against the SHA-256 in its model card before it is loaded. Activation is audited and survives restarts.</p>
        </div>
      </div>
    </Panel>
  );
}

export function MetricsPage() {
  const m = useApi<MetricsResponse>('/api/metrics');
  const p = usePrincipal();
  if (m.error) return <ErrorBox message={m.error} onRetry={m.reload} />;
  if (!m.data) return <Loading label="Measuring" />;
  const { metrics, dataset, models } = m.data;
  return (
    <div className="page">
      <header className="page-head">
        <h1>Detection quality</h1>
        <p>Measured against the PRD targets on the Replay Lab dataset (seed {dataset.seed}): {num(dataset.aircraft)} aircraft, {num(dataset.flights)} flights, {num(dataset.events)} fault events, {num(dataset.samples)} parameter samples.</p>
      </header>
      <Notice tone="caution">These numbers come from synthetic data with known, injected scenarios. They show that the pipeline works; they are not evidence of performance on operational data.</Notice>
      <Panel title="Targets">
        <table className="table">
          <thead><tr><th>Measure</th><th className="num">Result</th><th>Target</th><th>Status</th><th>Method</th></tr></thead>
          <tbody>
            {metrics.kpis.map((k) => (
              <tr key={k.key}>
                <td><strong>{k.label}</strong></td>
                <td className="num tabular">{k.value === null ? 'Not yet measured' : `${num(k.value, k.unit === 'ms' || k.unit === 'min' ? 2 : k.unit === '' ? 2 : 1)} ${k.unit}`}</td>
                <td>{k.target}</td>
                <td>{k.met === null ? <span className="muted">Pending</span> : k.met ? <span className="ok">Met</span> : <span className="err">Not met</span>}</td>
                <td><small>{k.note}</small></td>
              </tr>
            ))}
          </tbody>
        </table>
        <p className="footnote">Engineer feedback so far: {metrics.feedback_count} alerts rated{metrics.feedback_precision !== null ? `, ${num(metrics.feedback_precision * 100)}% useful` : ''}. On operational data this replaces the synthetic precision figure.</p>
      </Panel>
      {can(p, 'engineer', 'safety_manager') && <ModelPanel />}
      <div className="grid-2">
        <Panel title={`Hidden scenarios (${metrics.scenarios.filter((s) => s.detected).length} of ${metrics.scenarios.length} detected)`}>
          {metrics.scenarios.length ? (
            <table className="table compact">
              <thead><tr><th>Scenario</th><th>Aircraft</th><th>Detected by</th></tr></thead>
              <tbody>{metrics.scenarios.map((s) => (
                <tr key={s.id}><td><strong>{s.description}</strong><br /><small>{s.id}, {s.kind.replace(/_/g, ' ')}</small></td><td><a href={`#/aircraft/${s.tail}`}>{s.tail}</a></td>
                  <td>{s.detected ? <small>{s.detected_by.join('; ')}</small> : <span className="err">Missed</span>}</td></tr>
              ))}</tbody>
            </table>
          ) : <Empty>No ground truth loaded.</Empty>}
        </Panel>
        <Panel title="Models in use">
          {models.map((md) => (
            <div key={md.name} className="model">
              <strong>{md.name}</strong><p>{md.version ?? 'not loaded'}, {md.kind}</p>
              {md.card && <pre className="card-json">{JSON.stringify((md.card as { evaluation?: unknown }).evaluation ?? md.card, null, 2)}</pre>}
            </div>
          ))}
          <p className="footnote">Hypotheses {metrics.hypotheses_matched} of {metrics.hypotheses_total} and parameter alerts {metrics.alerts_matched} of {metrics.alerts_total} match a labelled scenario. Analysis pass {num(m.data.recompute_ms, 0)} ms.</p>
        </Panel>
      </div>
    </div>
  );
}
