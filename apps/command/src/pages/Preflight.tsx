import { useState } from 'react';
import { api } from '../api';
import { DownloadButton, Empty, ErrorBox, GradeChip, Loading, Notice, OverallPill, Panel, Sev, StatusPill } from '../components/ui';
import { age, relative, utc } from '../format';
import { can, go, useAction, useApi, useLive, usePrincipal } from '../hooks';
import type { PreflightReport, PreflightRow } from '../types';

export function PreflightPage({ flightId }: { flightId?: string }) {
  return flightId ? <PreflightDetail flightId={flightId} /> : <PreflightBoard />;
}

function PreflightBoard() {
  const rows = useApi<PreflightRow[]>('/api/preflight');
  const { simNow } = useLive();
  const [only, setOnly] = useState(false);
  if (rows.error) return <ErrorBox message={rows.error} onRetry={rows.reload} />;
  if (!rows.data) return <Loading label="Assembling pre-departure reports" />;
  const list = rows.data.filter((r) => !only || r.overall !== 'no_findings_in_available_data');
  const ms = rows.data.reduce((s, r) => s + r.generation_ms, 0) / Math.max(rows.data.length, 1);
  return (
    <div className="page">
      <header className="page-head">
        <h1>Pre-departure board</h1>
        <p>One evidence report per next departure, generated in {ms.toFixed(2)} ms on average. Reports rank findings; they never clear an aircraft.</p>
      </header>
      <div className="toolbar">
        <label className="check"><input type="checkbox" checked={only} onChange={(e) => setOnly(e.target.checked)} /> Only departures needing review</label>
        <span className="toolbar-count">{list.length} departures</span>
        <DownloadButton path="/api/reports/preflight.csv" filename="aips-preflight.csv" />
      </div>
      {list.length ? (
        <div className="panel">
          <table className="table">
            <thead><tr><th>STD</th><th>Flight</th><th>Aircraft</th><th>Assessment</th><th>Categories</th><th>Top finding</th><th>Review</th></tr></thead>
            <tbody>
              {list.map((r) => (
                <tr key={r.flight_id} className="clickable" onClick={() => go(`preflight/${encodeURIComponent(r.flight_id)}`)}>
                  <td className="tabular"><strong>{utc(r.std, false)}</strong><br /><small>{simNow ? relative(r.std, simNow) : ''}</small></td>
                  <td><strong>{r.number}</strong><br /><small>{r.origin}–{r.dest}</small></td>
                  <td>{r.tail}<br /><small>{r.type_code}</small></td>
                  <td><OverallPill o={r.overall} /></td>
                  <td><span className="cat-dots" aria-label="Category statuses">{r.categories.map((c) => <i key={c.category} className={`cd cd-${c.status}`} title={`${c.label}: ${c.status.replace(/_/g, ' ')}`} />)}</span></td>
                  <td className="clip">{r.top ?? '—'}</td>
                  <td>{r.acknowledged_by ? <small>Reviewed by {r.acknowledged_by}</small> : <small className="muted">Not reviewed</small>}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : <Empty>No departures need review in the available data.</Empty>}
    </div>
  );
}

function PreflightDetail({ flightId }: { flightId: string }) {
  const r = useApi<PreflightReport>(`/api/preflight/${encodeURIComponent(flightId)}`);
  const p = usePrincipal();
  const act = useAction();
  if (r.error) return <ErrorBox message={r.error} onRetry={r.reload} />;
  if (!r.data) return <Loading label="Loading report" />;
  const d = r.data;
  const ack = () => act.run(async () => { await api(`/api/preflight/${encodeURIComponent(flightId)}/acknowledge`, { method: 'POST' }); r.reload(); });
  return (
    <div className="page">
      <a href="#/preflight" className="back">Pre-departure board</a>
      <header className="page-head row">
        <div>
          <h1>{d.number} {d.origin}–{d.dest}</h1>
          <p><a href={`#/aircraft/${d.tail}`}>{d.tail}</a>, {d.type_code}. Scheduled {utc(d.std)}. Report generated {utc(d.generated_at)} in {d.generation_ms.toFixed(2)} ms.</p>
        </div>
        <OverallPill o={d.overall} />
      </header>
      <Notice tone="safety">{d.disclaimer}</Notice>
      {d.escalated_at && <Notice tone="caution">Escalated to the maintenance control duty controller at {utc(d.escalated_at)} because a significant concern was not reviewed 45 minutes before departure.</Notice>}
      {d.overall === 'insufficient_data' && <Notice tone="caution">Critical aircraft data is stale, so AIPS cannot assess the technical categories. Obtain current data before relying on this report.</Notice>}

      <div className="grid-pf">
        <Panel title="Categories" className="span-2">
          <ul className="cats">
            {d.categories.map((c) => (
              <li key={c.category} className={`cat cat-${c.status}`}>
                <span>{c.label}</span>
                <StatusPill s={c.status} />
                <small>{c.findings.length ? `${c.findings.length} finding${c.findings.length > 1 ? 's' : ''}` : 'No findings in available data'}</small>
              </li>
            ))}
          </ul>
        </Panel>
        <Panel title="Data freshness">
          <table className="table compact">
            <tbody>{d.freshness.map((f) => (
              <tr key={f.source}><td>{f.source}</td><td className={`tabular ${f.stale ? 'stale' : ''}`}>{age(f.age_seconds)}{f.stale ? ' (stale)' : ''}</td></tr>
            ))}</tbody>
          </table>
          <div className="ack-box">
            {d.acknowledged_by ? <p>Findings reviewed by <strong>{d.acknowledged_by}</strong> at {utc(d.acknowledged_at)}.</p>
              : can(p, 'flight_ops', 'engineer', 'mcc_controller') ? (
                <>
                  <button className="btn" onClick={ack} disabled={act.pending}>Record that I reviewed these findings</button>
                  <p className="footnote">This records a review. It is not a dispatch or airworthiness decision.</p>
                </>
              ) : <p className="footnote">Not yet reviewed.</p>}
            {act.error && <p className="err">{act.error}</p>}
          </div>
        </Panel>
        {d.context && (
          <Panel title="Operational context" className="span-3" aside={<small>{d.context.source}</small>}>
            <div className="grid-3">
              {d.context.departure_weather && <div className="wx"><h3>Departure {d.context.departure_weather.airport}</h3><code>{d.context.departure_weather.metar}</code></div>}
              {d.context.arrival_weather && <div className="wx"><h3>Arrival {d.context.arrival_weather.airport}</h3><code>{d.context.arrival_weather.metar}</code></div>}
              <div className="wx"><h3>NOTAMs</h3>{d.context.notams.length ? <ul className="plain-list">{d.context.notams.map((n) => <li key={n.id}><strong>{n.id}</strong> {n.airport}: {n.text}</li>)}</ul> : <small className="muted">None in effect.</small>}</div>
            </div>
          </Panel>
        )}
        <Panel title={`Findings, highest priority first (${d.ranked.length})`} className="span-3">
          {d.ranked.length ? (
            <ol className="findings">
              {d.ranked.map((f, i) => (
                <li key={i}>
                  <StatusPill s={f.status} />
                  <div><strong>{f.title}</strong><p>{f.detail}</p><small>{d.categories.find((c) => c.category === f.category)?.label}</small></div>
                  <span className="f-side"><Sev s={f.severity} />{f.evidence_grade && <GradeChip g={f.evidence_grade} />}</span>
                </li>
              ))}
            </ol>
          ) : <Empty>No findings in the available data. This is not a statement that the aircraft is airworthy.</Empty>}
        </Panel>
      </div>
    </div>
  );
}
