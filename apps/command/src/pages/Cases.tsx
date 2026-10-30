import { useMemo, useState } from 'react';
import { api } from '../api';
import { DownloadButton, Empty, ErrorBox, GradeChip, Loading, Panel, Sev } from '../components/ui';
import { caseKindLabel, relative, systemLabel, utc } from '../format';
import { can, go, useAction, useApi, useLive, usePrincipal } from '../hooks';
import type { Case, CaseStatus, FaultEvent, Hypothesis } from '../types';

const STATUS_LABEL: Record<CaseStatus, string> = { open: 'Open', assigned: 'Assigned', in_progress: 'In progress', escalated: 'Escalated', closed: 'Closed' };
const TEAMS = ['Powerplant Engineering', 'Avionics & Electrical', 'Hydraulics & Landing Gear', 'Fuel Systems', 'Flight Controls', 'Structures & Cabin'];

export function CasesPage({ caseId }: { caseId?: string }) {
  return caseId ? <CaseDetail id={caseId} /> : <CaseList />;
}

function CaseList() {
  const cs = useApi<Case[]>('/api/cases');
  const { simNow } = useLive();
  const [status, setStatus] = useState('active');
  const rows = useMemo(() => (cs.data ?? []).filter((c) => status === 'all' || (status === 'active' ? c.status !== 'closed' : c.status === status)), [cs.data, status]);
  if (cs.error) return <ErrorBox message={cs.error} onRetry={cs.reload} />;
  if (!cs.data) return <Loading label="Loading investigations" />;
  return (
    <div className="page">
      <header className="page-head">
        <h1>Investigations</h1>
        <p>Cases open automatically from recurring defects, repeat repairs, cross-system links, degradation and sensor faults. Unhandled cases escalate when their SLA expires.</p>
      </header>
      <div className="toolbar">
        <div className="segmented" role="group" aria-label="Status">
          {[['active', 'Active'], ['escalated', 'Escalated'], ['closed', 'Closed'], ['all', 'All']].map(([k, l]) => (
            <button key={k} aria-pressed={status === k} onClick={() => setStatus(k)}>{l}</button>
          ))}
        </div>
        <span className="toolbar-count">{rows.length} cases</span>
        <DownloadButton path="/api/reports/cases.csv" filename="aips-cases.csv" />
      </div>
      {rows.length ? (
        <div className="panel">
          <table className="table">
            <thead><tr><th>Case</th><th>Aircraft</th><th>Type</th><th>Priority</th><th>Status</th><th>Owner</th><th>SLA</th></tr></thead>
            <tbody>
              {rows.map((c) => {
                const breached = c.status !== 'closed' && simNow && Date.parse(c.sla_due) < Date.parse(simNow);
                return (
                  <tr key={c.id} className="clickable" onClick={() => go(`cases/${c.id}`)}>
                    <td><strong>{c.title}</strong><br /><small>{c.id}</small></td>
                    <td>{c.tail}</td>
                    <td>{caseKindLabel[c.kind] ?? c.kind}<br /><small>{systemLabel[c.system]}</small></td>
                    <td><Sev s={c.priority} /></td>
                    <td><span className={`cstatus cs-${c.status}`}>{STATUS_LABEL[c.status]}{c.escalation_level ? ` (L${c.escalation_level})` : ''}</span></td>
                    <td>{c.assignee ?? <span className="muted">{c.team}</span>}</td>
                    <td className={`tabular ${breached ? 'stale' : ''}`}>{c.status === 'closed' ? '—' : simNow ? relative(c.sla_due, simNow).replace('ago', 'overdue') : utc(c.sla_due)}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      ) : <Empty>No cases with this status.</Empty>}
    </div>
  );
}

function CaseDetail({ id }: { id: string }) {
  const d = useApi<{ case: Case; hypotheses: Hypothesis[]; events: FaultEvent[] }>(`/api/cases/${encodeURIComponent(id)}`);
  const p = usePrincipal();
  const act = useAction();
  const [assignee, setAssignee] = useState('');
  const [team, setTeam] = useState('');
  const [note, setNote] = useState('');
  if (d.error) return <ErrorBox message={d.error} onRetry={d.reload} />;
  if (!d.data) return <Loading label="Loading case" />;
  const c = d.data.case;
  const canAssign = can(p, 'mcc_controller', 'engineer');
  const canStatus = can(p, 'engineer', 'safety_manager', 'mcc_controller');
  const post = (path: string, body: unknown) => act.run(async () => { await api(`/api/cases/${encodeURIComponent(id)}/${path}`, { method: 'POST', body }); setNote(''); d.reload(); });

  return (
    <div className="page">
      <a href="#/cases" className="back">Investigations</a>
      <header className="page-head row">
        <div>
          <h1>{c.title}</h1>
          <p>{c.id} on <a href={`#/aircraft/${c.tail}`}>{c.tail}</a>. {caseKindLabel[c.kind] ?? c.kind}, {systemLabel[c.system]}. Opened {utc(c.opened_at)}; SLA due {utc(c.sla_due)}.</p>
        </div>
        <span className={`cstatus cs-${c.status} big`}>{STATUS_LABEL[c.status]}{c.escalation_level ? `, escalation level ${c.escalation_level}` : ''}</span>
      </header>
      <div className="grid-2">
        <Panel title="History">
          <ol className="timeline">
            {[...c.history].reverse().map((h, i) => (
              <li key={i} className={`tl tl-case-${h.action}`}>
                <time className="tabular">{utc(h.at)}</time>
                <div><strong>{h.action.replace(/_/g, ' ')}</strong> by {h.actor}<p>{h.note}</p></div>
              </li>
            ))}
          </ol>
        </Panel>
        <div className="stack">
          {(canAssign || canStatus) && c.status !== 'closed' && (
            <Panel title="Actions">
              {canAssign && (
                <form className="form-row" onSubmit={(e) => { e.preventDefault(); if (assignee.trim()) void post('assign', { assignee: assignee.trim(), team: team || null }); }}>
                  <label>Assign to<input value={assignee} onChange={(e) => setAssignee(e.target.value)} placeholder="Name of engineer" required /></label>
                  <label>Team<select value={team} onChange={(e) => setTeam(e.target.value)}><option value="">{c.team}</option>{TEAMS.filter((t) => t !== c.team).map((t) => <option key={t}>{t}</option>)}</select></label>
                  <button className="btn" disabled={act.pending}>Assign</button>
                </form>
              )}
              {canStatus && (
                <div className="form-col">
                  <label htmlFor="case-note">Note or corrective action</label>
                  <textarea id="case-note" rows={3} value={note} onChange={(e) => setNote(e.target.value)} placeholder="Closing a case needs the corrective action taken (at least 10 characters)." />
                  <div className="btn-row">
                    <button className="btn btn-quiet" disabled={act.pending || !note.trim()} onClick={() => post('status', { status: 'in_progress', note })}>Mark in progress</button>
                    <button className="btn" disabled={act.pending} onClick={() => post('status', { status: 'closed', note })}>Close case</button>
                  </div>
                </div>
              )}
              {act.error && <p className="err" role="alert">{act.error}</p>}
            </Panel>
          )}
          <Panel title="Linked evidence">
            {d.data.hypotheses.map((h) => (
              <p key={h.id}><a href={`#/memory?tail=${h.tail}&h=${h.id}`}><strong>{h.title}</strong></a> <GradeChip g={h.grade} /><br /><small>{h.explanation.narrative}</small></p>
            ))}
            {d.data.events.length ? (
              <table className="table compact">
                <thead><tr><th>Time</th><th>Code</th><th>Message</th></tr></thead>
                <tbody>{d.data.events.map((e) => <tr key={e.id}><td className="tabular">{utc(e.ts)}</td><td>{e.code}</td><td>{e.message}</td></tr>)}</tbody>
              </table>
            ) : !d.data.hypotheses.length && <Empty>This case was opened from parameter analytics; see the aircraft&apos;s health tab.</Empty>}
          </Panel>
        </div>
      </div>
    </div>
  );
}
