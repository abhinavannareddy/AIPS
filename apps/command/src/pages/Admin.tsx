import { useState } from 'react';
import { api } from '../api';
import { Empty, ErrorBox, Loading, Notice, Panel } from '../components/ui';
import { utc } from '../format';
import { useAction, useApi, usePrincipal } from '../hooks';
import type { ErasureRequest } from '../types';

/** DAT-03: aircraft data erasure under a two-person rule. Request and approval must come from different administrators. */
export function AdminPage() {
  const p = usePrincipal();
  const list = useApi<ErasureRequest[]>('/api/admin/erasure-requests');
  const act = useAction();
  const [tail, setTail] = useState('');
  const [reason, setReason] = useState('');
  const submit = () => act.run(async () => {
    await api('/api/admin/erasure-requests', { method: 'POST', body: { tail: tail.trim().toUpperCase(), reason } });
    setTail(''); setReason(''); list.reload();
  });
  const approve = (id: string) => act.run(async () => { await api(`/api/admin/erasure-requests/${encodeURIComponent(id)}/approve`, { method: 'POST' }); list.reload(); });
  return (
    <div className="page">
      <header className="page-head">
        <h1>Data administration</h1>
        <p>Erase every operational record of one aircraft, for example when it leaves the operator. The audit trail itself is retained and records that erasure happened.</p>
      </header>
      <Notice tone="caution">Erasure is irreversible. A second administrator must approve each request; the requester cannot approve their own.</Notice>
      <Panel title="New erasure request">
        <div className="form-row">
          <label className="form-col">Registration<input value={tail} onChange={(e) => setTail(e.target.value)} placeholder="SE-RAA" /></label>
          <label className="form-col grow">Reason (at least 10 characters)<input value={reason} onChange={(e) => setReason(e.target.value)} /></label>
          <button className="btn btn-danger" disabled={act.pending || !tail || reason.trim().length < 10} onClick={submit}>Request erasure</button>
        </div>
        {act.error && <p className="err">{act.error}</p>}
      </Panel>
      <Panel title="Requests">
        {list.error ? <ErrorBox message={list.error} onRetry={list.reload} /> : !list.data ? <Loading /> : list.data.length ? (
          <table className="table">
            <thead><tr><th>Aircraft</th><th>Reason</th><th>Requested</th><th>Status</th><th /></tr></thead>
            <tbody>{list.data.map((r) => (
              <tr key={r.id}>
                <td><strong>{r.tail}</strong></td><td>{r.reason}</td><td>{r.requested_by}<br /><small>{utc(r.requested_at)}</small></td>
                <td>{r.executed_at ? <small>Erased {utc(r.executed_at)} by approval of {r.approved_by}; {r.rows_erased} rows</small> : <small className="muted">Awaiting second administrator</small>}</td>
                <td>{!r.executed_at && (r.requested_by === p?.subject
                  ? <small className="muted">You requested this</small>
                  : <button className="btn btn-small btn-danger" disabled={act.pending} onClick={() => approve(r.id)}>Approve and erase</button>)}</td>
              </tr>
            ))}</tbody>
          </table>
        ) : <Empty>No erasure requests.</Empty>}
      </Panel>
    </div>
  );
}
