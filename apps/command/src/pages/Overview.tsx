import { Strip } from '../components/Strip';
import { Empty, ErrorBox, GradeChip, Loading, OverallPill, Panel, Sev } from '../components/ui';
import { num, relative, utc } from '../format';
import { go, useApi, useLive } from '../hooks';
import type { AircraftView, Overview } from '../types';

export function OverviewPage() {
  const ov = useApi<Overview>('/api/overview');
  const fleet = useApi<AircraftView[]>('/api/fleet');
  const { simNow } = useLive();
  if (ov.error) return <ErrorBox message={ov.error} onRetry={ov.reload} />;
  if (!ov.data || !fleet.data) return <Loading label="Loading fleet status" />;
  const o = ov.data;
  const total = Math.max(o.fleet.total, 1);
  const segs = [
    { k: 'attention', n: o.fleet.attention, label: 'Attention' },
    { k: 'review', n: o.fleet.review, label: 'Review' },
    { k: 'insufficient_data', n: o.fleet.insufficient_data, label: 'Insufficient data' },
    { k: 'nominal', n: o.fleet.nominal, label: 'Nominal' },
  ];
  const focus = fleet.data.filter((a) => a.status !== 'nominal').slice(0, 9);
  const sysMax = Math.max(1, ...Object.values(o.alerts.by_system));
  const now = simNow ?? o.clock.sim_now;

  return (
    <div className="page">
      <header className="page-head">
        <h1>Fleet status</h1>
        <p>{o.fleet.total} aircraft · analysis at {utc(o.clock.sim_now)} · last pass {num(o.recompute_ms, 0)} ms</p>
      </header>

      <section className="fleet-bar" aria-label="Fleet status distribution">
        <div className="fleet-bar-track">
          {segs.filter((s) => s.n > 0).map((s) => (
            <span key={s.k} className={`seg seg-${s.k}`} style={{ flexGrow: s.n / total }} title={`${s.label}: ${s.n}`} />
          ))}
        </div>
        <ul>
          {segs.map((s) => (
            <li key={s.k}><span className={`dot seg-${s.k}`} aria-hidden /><strong className="tabular">{s.n}</strong> {s.label}</li>
          ))}
          <li className="sep"><strong className="tabular">{o.cases.open}</strong> open investigations{o.cases.escalated ? `, ${o.cases.escalated} escalated` : ''}</li>
          <li><strong className="tabular">{o.preflight.needing_review}</strong> departures need review</li>
        </ul>
      </section>

      <div className="grid-overview">
        <Panel title="Aircraft needing attention" aside={<a href="#/fleet">All aircraft</a>} className="span-2">
          {focus.length ? <div className="strips">{focus.map((a) => <Strip key={a.tail} a={a} />)}</div>
            : <Empty>Every aircraft is nominal in the available data.</Empty>}
        </Panel>

        <Panel title="Next departures" aside={<a href="#/preflight">Pre-departure board</a>}>
          <table className="table compact">
            <thead><tr><th>Flight</th><th>Aircraft</th><th>STD</th><th>Assessment</th></tr></thead>
            <tbody>
              {o.preflight.upcoming.map((u) => (
                <tr key={u.flight_id} className="clickable" onClick={() => go(`preflight/${encodeURIComponent(u.flight_id)}`)}>
                  <td><strong>{u.number}</strong><br /><small>{u.origin}–{u.dest}</small></td>
                  <td className="nowrap">{u.tail}</td>
                  <td className="tabular nowrap">{utc(u.std, false)}<br /><small>{relative(u.std, now)}</small></td>
                  <td><OverallPill o={u.overall} />{u.acknowledged_by && <small className="ack">Reviewed by {u.acknowledged_by}</small>}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Panel>

        <Panel title="Highest-priority alerts" aside={<a href="#/alerts">All alerts</a>} className="span-2">
          {o.top_alerts.length ? (
            <ul className="alert-list">
              {o.top_alerts.map((a) => (
                <li key={a.id}>
                  <Sev s={a.severity} />
                  <div>
                    <a href={`#/aircraft/${a.tail}`} className="alert-title"><strong>{a.tail}</strong> {a.title}</a>
                    <p>{a.reason}</p>
                  </div>
                  <GradeChip g={a.evidence_grade} />
                </li>
              ))}
            </ul>
          ) : <Empty>No alerts in the available data.</Empty>}
        </Panel>

        <Panel title="Alerts by system">
          <ul className="hbars">
            {Object.entries(o.alerts.by_system).sort((a, b) => b[1] - a[1]).map(([k, v]) => (
              <li key={k}><span>{k}</span><span className="hbar"><i style={{ width: `${(v / sysMax) * 100}%` }} /></span><strong className="tabular">{v}</strong></li>
            ))}
          </ul>
          <p className="footnote">Detection quality on replay data: recall {num(o.metrics.recall * 100)}%, precision {num(o.metrics.precision * 100)}%. <a href="#/metrics">How this is measured</a></p>
        </Panel>
      </div>
    </div>
  );
}
