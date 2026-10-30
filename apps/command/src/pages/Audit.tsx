import { useState } from 'react';
import { api } from '../api';
import { Empty, ErrorBox, Loading, Panel } from '../components/ui';
import { utc } from '../format';
import { can, useAction, useApi, usePrincipal } from '../hooks';
import type { AuditEntry } from '../types';

export function AuditPage() {
  const a = useApi<{ total: number; entries: AuditEntry[] }>('/api/audit?limit=300');
  const act = useAction();
  const p = usePrincipal();
  const [v, setV] = useState<{ intact: boolean; entries: number; first_broken_seq: number | null } | null>(null);
  if (a.error) return <ErrorBox message={a.error} onRetry={a.reload} />;
  if (!a.data) return <Loading label="Loading audit trail" />;
  return (
    <div className="page">
      <header className="page-head">
        <h1>Audit trail</h1>
        <p>Every human decision and automated escalation, hash-chained so any later change to history is detectable.</p>
      </header>
      <Panel title={`${a.data.total} entries`} aside={can(p, 'engineer', 'safety_manager') ? <button className="btn btn-small" onClick={() => act.run(async () => setV(await api('/api/audit/verify')))} disabled={act.pending}>Verify chain</button> : undefined}>
        {v && <p className={v.intact ? 'ok' : 'err'}>{v.intact ? `Chain intact: all ${v.entries} entries verified.` : `Chain broken at entry ${v.first_broken_seq}.`}</p>}
        {act.error && <p className="err">{act.error}</p>}
        {a.data.entries.length ? (
          <table className="table compact">
            <thead><tr><th>#</th><th>Time</th><th>Actor</th><th>Action</th><th>Subject</th><th>Detail</th><th>Hash</th></tr></thead>
            <tbody>{a.data.entries.map((e) => (
              <tr key={e.seq}><td className="tabular">{e.seq}</td><td className="tabular">{utc(e.at)}</td><td>{e.actor}</td><td><strong>{e.action}</strong></td>
                <td>{e.subject}</td><td><small>{JSON.stringify(e.detail)}</small></td><td><code title={e.hash}>{e.hash.slice(0, 10)}</code></td></tr>
            ))}</tbody>
          </table>
        ) : <Empty>No decisions recorded yet. Grade a hypothesis or assign a case to start the trail.</Empty>}
      </Panel>
    </div>
  );
}
