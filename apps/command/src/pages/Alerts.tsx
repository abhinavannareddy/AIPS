import { useMemo, useState } from 'react';
import { api } from '../api';
import { DownloadButton, Empty, ErrorBox, GradeChip, Loading, Sev } from '../components/ui';
import { age, systemLabel, utc } from '../format';
import { can, useAction, useApi, usePrincipal } from '../hooks';
import type { Alert } from '../types';

const MODULE_LABEL: Record<string, string> = { integrity: 'Integrity', predict: 'Predict', prevent: 'Prevent', flight: 'Flight', guardian: 'Guardian', workflow: 'Workflow' };

/** INT-05: reason, sources with data age and model version are always visible. INT-06: feedback. */
export function AlertRow({ a, onChange }: { a: Alert; onChange?: () => void }) {
  const p = usePrincipal();
  const act = useAction();
  const [open, setOpen] = useState(false);
  const canFeedback = can(p, 'engineer', 'mcc_controller', 'safety_manager');
  const canAck = can(p, 'engineer', 'mcc_controller', 'safety_manager');
  const ackIt = () => act.run(async () => {
    await api(`/api/alerts/${encodeURIComponent(a.id)}/acknowledge`, { method: 'POST' });
    onChange?.();
  });
  const send = (useful: boolean) => act.run(async () => {
    await api(`/api/alerts/${encodeURIComponent(a.id)}/feedback`, { method: 'POST', body: { useful, comment: '' } });
    onChange?.();
  });
  return (
    <li className="alert-row">
      <Sev s={a.severity} />
      <div className="alert-body">
        <button className="alert-title linkish" onClick={() => setOpen(!open)} aria-expanded={open}>
          <strong>{a.tail}</strong> {a.title}
        </button>
        <p>{a.reason}</p>
        {open && (
          <dl className="evidence">
            <div><dt>Module</dt><dd>{MODULE_LABEL[a.module] ?? a.module}, {systemLabel[a.system]}</dd></div>
            {a.sources.map((s) => (
              <div key={s.source}><dt>Source</dt><dd>{s.source}: data age {age(s.data_age_seconds)}; records {s.record_ids.slice(0, 4).join(', ')}{s.record_ids.length > 4 ? ` +${s.record_ids.length - 4}` : ''}</dd></div>
            ))}
            <div><dt>Model</dt><dd>{a.model_version}</dd></div>
            <div><dt>Raised</dt><dd>{utc(a.created_at)}</dd></div>
            {a.hypothesis_id && <div><dt>Memory</dt><dd><a href={`#/memory?tail=${a.tail}&h=${a.hypothesis_id}`}>Open linked evidence</a></dd></div>}
          </dl>
        )}
      </div>
      <div className="alert-side">
        <GradeChip g={a.evidence_grade} />
        {a.escalated_at && <small className="err">Escalated {utc(a.escalated_at)}: not acknowledged in time</small>}
        {a.acknowledged ? <small className="fb-done">Acknowledged by {a.acknowledged.by}</small>
          : canAck ? <button className="btn btn-small" disabled={act.pending} onClick={ackIt}>Acknowledge</button> : null}
        {a.feedback ? (
          <small className="fb-done">{a.feedback.useful ? 'Marked useful' : 'Marked not useful'} by {a.feedback.by}</small>
        ) : canFeedback ? (
          <span className="fb">
            <button className="btn btn-small" disabled={act.pending} onClick={() => send(true)}>Useful</button>
            <button className="btn btn-small btn-quiet" disabled={act.pending} onClick={() => send(false)}>Not useful</button>
          </span>
        ) : null}
        {act.error && <small className="err">{act.error}</small>}
      </div>
    </li>
  );
}

export function AlertsPage() {
  const al = useApi<Alert[]>('/api/alerts');
  const [sev, setSev] = useState('');
  const [mod, setMod] = useState('');
  const [q, setQ] = useState('');
  const rows = useMemo(() => (al.data ?? []).filter((a) => (!sev || a.severity === sev) && (!mod || a.module === mod)
    && (!q || `${a.tail} ${a.title}`.toLowerCase().includes(q.toLowerCase()))), [al.data, sev, mod, q]);
  if (al.error) return <ErrorBox message={al.error} onRetry={al.reload} />;
  if (!al.data) return <Loading label="Loading alerts" />;
  return (
    <div className="page">
      <header className="page-head">
        <h1>Alerts</h1>
        <p>Every alert shows why it fired, which data it used, how old that data is, and the model version. Acknowledge Warnings within 30 minutes or they escalate to the duty controller. Your feedback tunes precision.</p>
      </header>
      <div className="toolbar">
        <select aria-label="Severity" value={sev} onChange={(e) => setSev(e.target.value)}>
          <option value="">All severities</option><option value="warning">Warning</option><option value="caution">Caution</option><option value="advisory">Advisory</option>
        </select>
        <select aria-label="Module" value={mod} onChange={(e) => setMod(e.target.value)}>
          <option value="">All modules</option>{Object.entries(MODULE_LABEL).map(([k, v]) => <option key={k} value={k}>{v}</option>)}
        </select>
        <input type="search" placeholder="Registration or text" value={q} onChange={(e) => setQ(e.target.value)} aria-label="Search alerts" />
        <span className="toolbar-count">{rows.length} of {al.data.length}</span>
        <DownloadButton path="/api/reports/alerts.csv" filename="aips-alerts.csv" />
      </div>
      {rows.length ? <ul className="alert-list panel">{rows.map((a) => <AlertRow key={a.id} a={a} onChange={al.reload} />)}</ul>
        : <Empty>No alerts match these filters.</Empty>}
    </div>
  );
}
