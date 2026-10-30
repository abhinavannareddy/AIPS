import { useState } from 'react';
import { api } from '../api';
import { RulBar, SeriesChart, type SeriesResp } from '../components/charts';
import { AlertRow } from './Alerts';
import { Empty, ErrorBox, GradeChip, Kv, Loading, OverallPill, Panel, Sev, Spark } from '../components/ui';
import { age, caseKindLabel, num, utc } from '../format';
import { can, useAction, useApi, usePrincipal } from '../hooks';
import type { AircraftDetail, RecordHistory, TimelineItem } from '../types';

const TABS = ['Health', 'Safety record', 'Components', 'Investigations'] as const;
type Tab = (typeof TABS)[number];

export function AircraftPage({ tail }: { tail?: string }) {
  const d = useApi<AircraftDetail>(tail ? `/api/aircraft/${encodeURIComponent(tail)}` : null);
  const [tab, setTab] = useState<Tab>('Health');
  const p = usePrincipal();
  const exp = useAction();
  if (!tail) return <Empty>Choose an aircraft from the <a href="#/fleet">aircraft list</a>.</Empty>;
  if (d.error) return <ErrorBox message={d.error} onRetry={d.reload} />;
  if (!d.data) return <Loading label={`Loading ${tail}`} />;
  const a = d.data;
  const operational = can(p, 'engineer', 'mcc_controller', 'flight_ops', 'safety_manager');

  const exportPassport = () => exp.run(async () => {
    const doc = await api<unknown>(`/api/aircraft/${encodeURIComponent(tail)}/passport/export`);
    const url = URL.createObjectURL(new Blob([JSON.stringify(doc, null, 2)], { type: 'application/json' }));
    const link = document.createElement('a');
    link.href = url; link.download = `aips-passport-${tail}.json`; link.click();
    URL.revokeObjectURL(url);
  });

  return (
    <div className="page">
      <header className="ac-head">
        <div className={`ac-reg strip-${a.view.status}`}>
          <h1>{a.aircraft.tail}</h1>
          <p>{a.aircraft.type_code}, MSN {a.aircraft.msn}</p>
        </div>
        <dl className="ac-facts">
          <Kv k="Operator" v={a.aircraft.operator} />
          <Kv k="Lessor" v={a.aircraft.lessor ?? 'Owned'} />
          <Kv k="Base" v={a.aircraft.base} />
          <Kv k="Cycles" v={<span className="tabular">{num(a.view.cycles)}</span>} />
          <Kv k="Latest data" v={<span className={a.view.data_age_seconds > 43200 ? 'stale' : ''}>{age(a.view.data_age_seconds)} ago</span>} />
          <Kv k="Next departure" v={a.view.next_departure ? `${a.view.next_departure.number} ${utc(a.view.next_departure.std)}` : '—'} />
        </dl>
        <div className="ac-actions">
          <OverallPill o={a.view.preflight} />
          {a.preflight && operational && <a className="btn" href={`#/preflight/${encodeURIComponent(a.preflight.flight_id)}`}>Open pre-departure report</a>}
          <button className="btn btn-quiet" onClick={exportPassport} disabled={exp.pending}>{exp.pending ? 'Exporting…' : 'Export safety record'}</button>
        </div>
      </header>
      {exp.error && <ErrorBox message={exp.error} />}
      <div className="tabs" role="tablist">
        {TABS.map((t) => <button key={t} role="tab" aria-selected={tab === t} onClick={() => setTab(t)}>{t}</button>)}
      </div>
      {tab === 'Health' && <HealthTab a={a} tail={tail} operational={operational} onChange={d.reload} />}
      {tab === 'Safety record' && <TimelineTab tail={tail} />}
      {tab === 'Components' && <ComponentsTab a={a} />}
      {tab === 'Investigations' && <InvestigationsTab a={a} />}
    </div>
  );
}

function HealthTab({ a, tail, operational, onChange }: { a: AircraftDetail; tail: string; operational: boolean; onChange: () => void }) {
  const [param, setParam] = useState<string | null>(null);
  const series = useApi<SeriesResp>(param && operational ? `/api/aircraft/${encodeURIComponent(tail)}/series?param=${param}` : null);
  const rulMax = Math.max(4000, ...a.rul.map((r) => r.p90));
  return (
    <div className="grid-ac">
      <Panel title="Systems" className="span-3">
        <ul className="systems">
          {a.systems.map((s) => (
            <li key={s.system} className={`sys sys-${a.view.status === 'insufficient_data' ? 'stale' : s.worst ?? 'none'}`}>
              <span>{s.label}</span>
              {a.view.status === 'insufficient_data' ? <strong>No current data</strong>
                : s.worst ? <><Sev s={s.worst} /><small>{s.alerts} alert{s.alerts === 1 ? '' : 's'}</small></> : <strong>No alerts</strong>}
            </li>
          ))}
        </ul>
      </Panel>

      <Panel title="Parameters" aside={<span className="footnote">Last 40 flights. Select one for its history and baseline.</span>} className="span-2">
        <div className="params">
          {a.params.map((p) => {
            const out = p.last !== null && (p.last < p.lo || p.last > p.hi);
            return (
              <button key={p.param} className="param" aria-pressed={param === p.param} onClick={() => setParam(param === p.param ? null : p.param)} disabled={!operational}>
                <span>{p.label}</span>
                <strong className="tabular">{num(p.last, Math.abs(p.last ?? 0) < 20 ? 2 : 0)} <small>{p.unit}</small></strong>
                <Spark values={p.spark} warn={out} />
              </button>
            );
          })}
        </div>
        {param && (series.data ? <SeriesChart s={series.data} /> : series.error ? <ErrorBox message={series.error} /> : <Loading label="Loading history" />)}
      </Panel>

      <Panel title="Remaining useful life">
        {a.rul.length ? a.rul.map((r) => <RulBar key={r.component_id} r={r} scaleMax={rulMax} />)
          : <Empty>Not enough engine trend data for an estimate.</Empty>}
        {a.rul[0] && <p className="footnote">{a.rul[0].model_version}. Bars show the 10th to 90th percentile range.</p>}
      </Panel>

      <Panel title={`Alerts (${a.alerts.length})`} className="span-3">
        {a.alerts.length ? <ul className="alert-list">{a.alerts.map((al) => <AlertRow key={al.id} a={al} onChange={onChange} />)}</ul>
          : <Empty>No alerts for this aircraft in the available data.</Empty>}
      </Panel>
    </div>
  );
}

const FIELDS: Record<string, string[]> = { fault_event: ['message', 'code', 'ata', 'severity'], work_order: ['note', 'signed_off'] };

/** PSP-02: attributed, versioned corrections. The original record is never overwritten. */
function RecordCorrection({ rtype, id, onSaved }: { rtype: string; id: string; onSaved: () => void }) {
  const p = usePrincipal();
  const [open, setOpen] = useState(false);
  const hist = useApi<RecordHistory>(open ? `/api/records/${rtype}/${encodeURIComponent(id)}/history` : null, { live: false });
  const act = useAction();
  const [field, setField] = useState(FIELDS[rtype][0]);
  const [value, setValue] = useState('');
  const [reason, setReason] = useState('');
  const canCorrect = can(p, 'engineer', 'mcc_controller');
  if (!can(p, 'engineer', 'mcc_controller', 'safety_manager', 'lessor')) return null;
  const save = () => act.run(async () => {
    await api('/api/records/corrections', { method: 'POST', body: { record_type: rtype, record_id: id, field, new_value: value, reason } });
    setValue(''); setReason(''); hist.reload(); onSaved();
  });
  return (
    <div className="tl-correct">
      <button className="btn btn-small btn-quiet" aria-expanded={open} onClick={() => setOpen(!open)}>{open ? 'Close' : 'History'}</button>
      {open && (
        <div className="correct-box">
          {hist.data ? (
            hist.data.corrections.length ? (
              <ol className="plain-list">{hist.data.corrections.map((c) => (
                <li key={c.id}><strong>v{c.version + 1}</strong> {c.field}: “{c.old_value}” → “{c.new_value}”<br /><small>{c.by}, {utc(c.at)}. {c.reason}</small></li>
              ))}</ol>
            ) : <p className="muted"><small>Original record, never corrected.</small></p>
          ) : hist.error ? <p className="err">{hist.error}</p> : <Loading />}
          {canCorrect && (
            <div className="form-row">
              <label className="form-col">Field<select value={field} onChange={(e) => setField(e.target.value)}>{FIELDS[rtype].map((f) => <option key={f}>{f}</option>)}</select></label>
              <label className="form-col">New value<input value={value} onChange={(e) => setValue(e.target.value)} /></label>
              <label className="form-col grow">Reason<input value={reason} onChange={(e) => setReason(e.target.value)} /></label>
              <button className="btn btn-small" disabled={act.pending || !value || reason.trim().length < 5} onClick={save}>Record correction</button>
            </div>
          )}
          {act.error && <p className="err">{act.error}</p>}
        </div>
      )}
    </div>
  );
}

function TimelineTab({ tail }: { tail: string }) {
  const t = useApi<TimelineItem[]>(`/api/aircraft/${encodeURIComponent(tail)}/timeline`);
  const [kind, setKind] = useState('all');
  if (t.error) return <ErrorBox message={t.error} onRetry={t.reload} />;
  if (!t.data) return <Loading label="Loading safety record" />;
  const kinds = ['all', ...new Set(t.data.map((i) => i.kind))];
  const items = t.data.filter((i) => kind === 'all' || i.kind === kind);
  const kindLabel: Record<string, string> = { all: 'Everything', flight: 'Flights', fault: 'Faults', work_order: 'Corrective actions', hypothesis: 'Memory links', case: 'Investigations', mel: 'MEL' };
  return (
    <Panel title="Safety record (last 45 days)" aside={
      <select value={kind} onChange={(e) => setKind(e.target.value)} aria-label="Filter record">
        {kinds.map((k) => <option key={k} value={k}>{kindLabel[k] ?? k}</option>)}
      </select>}>
      {items.length ? (
        <ol className="timeline">
          {items.map((i, n) => (
            <li key={`${i.ref}-${n}`} className={`tl tl-${i.kind}`}>
              <time className="tabular">{utc(i.ts)}</time>
              <div>
                <strong>{i.title}</strong>
                <p>{i.detail}</p>
              </div>
              {i.severity && <Sev s={i.severity} />}
              {i.grade && <GradeChip g={i.grade} />}
              {(i.kind === 'fault' || i.kind === 'work_order') && <RecordCorrection rtype={i.kind === 'fault' ? 'fault_event' : 'work_order'} id={i.ref} onSaved={t.reload} />}
            </li>
          ))}
        </ol>
      ) : <Empty>Nothing recorded for this filter.</Empty>}
    </Panel>
  );
}

function ComponentsTab({ a }: { a: AircraftDetail }) {
  return (
    <Panel title="Installed components">
      <table className="table">
        <thead><tr><th>Position</th><th>Description</th><th>P/N</th><th>S/N</th><th>ATA</th><th className="num">Cycles since shop visit</th><th>Installed</th></tr></thead>
        <tbody>
          {a.components.map((c) => (
            <tr key={c.id}><td><strong>{c.position}</strong></td><td>{c.description}</td><td>{c.part_number}</td><td>{c.serial}</td>
              <td className="tabular">{c.ata}</td><td className="num tabular">{num(c.cycles_since_sv)}</td><td>{utc(c.installed_at).slice(0, 6)}</td></tr>
          ))}
        </tbody>
      </table>
      {a.mel.length > 0 && (
        <>
          <h3 className="sub">Minimum equipment list items</h3>
          <table className="table">
            <thead><tr><th>Reference</th><th>Item</th><th>Cat.</th><th>Opened</th><th>Due</th></tr></thead>
            <tbody>{a.mel.map((m) => <tr key={m.id}><td>{m.reference}</td><td>{m.title}</td><td>{m.category}</td><td>{utc(m.opened)}</td><td>{utc(m.due)}</td></tr>)}</tbody>
          </table>
        </>
      )}
    </Panel>
  );
}

function InvestigationsTab({ a }: { a: AircraftDetail }) {
  return (
    <div className="grid-2">
      <Panel title="Investigation cases">
        {a.cases.length ? (
          <ul className="plain-list">
            {a.cases.map((c) => (
              <li key={c.id}><a href={`#/cases/${c.id}`}><strong>{c.title}</strong></a><p>{caseKindLabel[c.kind] ?? c.kind}, {c.status.replace('_', ' ')}, assigned to {c.assignee ?? c.team}</p></li>
            ))}
          </ul>
        ) : <Empty>No investigations for this aircraft.</Empty>}
      </Panel>
      <Panel title="Cross-Flight Memory" aside={<a href={`#/memory?tail=${a.aircraft.tail}`}>Open graph</a>}>
        {a.hypotheses.length ? (
          <ul className="plain-list">
            {a.hypotheses.map((h) => (
              <li key={h.id}><a href={`#/memory?tail=${a.aircraft.tail}&h=${h.id}`}><strong>{h.title}</strong></a> <GradeChip g={h.grade} /><p>{h.explanation.narrative}</p></li>
            ))}
          </ul>
        ) : <Empty>No linked events for this aircraft.</Empty>}
      </Panel>
    </div>
  );
}
