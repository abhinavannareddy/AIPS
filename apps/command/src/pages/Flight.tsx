import { useMemo, useState } from 'react';
import { Empty, ErrorBox, GradeChip, Loading, Notice, Panel, Sev } from '../components/ui';
import { age, num, utc } from '../format';
import { go, useApi } from '../hooks';
import type { FlightEvent, FlightView, Notam, TrackPoint, Weather } from '../types';

export const flightKindLabel: Record<string, string> = {
  unstable_approach: 'Unstable approach',
  rapid_descent: 'Rapid descent',
  speed_exceedance_below10k: 'Speed above 250 kt below FL100',
  cruise_altitude_deviation: 'Cruise altitude deviation',
  weak_climb_performance: 'Weak climb performance',
  track_data_gap: 'Track data gap',
  delayed_data: 'Delayed flight data',
};

export function FlightPage({ flightId }: { flightId?: string }) {
  return flightId ? <FlightDetail flightId={flightId} /> : <FlightBoard />;
}

function FlightBoard() {
  const ev = useApi<FlightEvent[]>('/api/flight/events');
  const [kind, setKind] = useState('');
  const [q, setQ] = useState('');
  const rows = useMemo(() => (ev.data ?? []).filter((e) => (!kind || e.kind === kind)
    && (!q || `${e.tail} ${e.number}`.toLowerCase().includes(q.toLowerCase()))), [ev.data, kind, q]);
  if (ev.error) return <ErrorBox message={ev.error} onRetry={ev.reload} />;
  if (!ev.data) return <Loading label="Analysing flight data" />;
  return (
    <div className="page">
      <header className="page-head">
        <h1>Flight intelligence</h1>
        <p>Observable flight-data events from trajectory summaries, with the weather at the time and any faults recorded on the same flight. These are facts in the data, not findings about crews.</p>
      </header>
      <Notice tone="info">Trajectory data comes from the Replay Lab ADS-B/QAR substitute. Events never establish intent or cause; corroboration means a fault was recorded within 15 minutes on the same flight.</Notice>
      <div className="toolbar">
        <select aria-label="Event type" value={kind} onChange={(e) => setKind(e.target.value)}>
          <option value="">All event types</option>
          {Object.entries(flightKindLabel).map(([k, v]) => <option key={k} value={k}>{v}</option>)}
        </select>
        <input type="search" placeholder="Registration or flight" value={q} onChange={(e) => setQ(e.target.value)} aria-label="Search flight events" />
        <span className="toolbar-count">{rows.length} of {ev.data.length}</span>
      </div>
      {rows.length ? (
        <div className="panel">
          <table className="table">
            <thead><tr><th>Time</th><th>Flight</th><th>Aircraft</th><th>Event</th><th>Observed</th><th>Severity</th><th>Evidence</th></tr></thead>
            <tbody>
              {rows.map((e) => (
                <tr key={e.id} className="clickable" onClick={() => go(`flight/${encodeURIComponent(e.flight_id)}`)}>
                  <td className="tabular">{utc(e.at)}</td>
                  <td><a href={`#/flight/${encodeURIComponent(e.flight_id)}`} onClick={(x) => x.stopPropagation()}><strong>{e.number}</strong></a><br /><small>{e.origin}–{e.dest}</small></td>
                  <td>{e.tail}</td>
                  <td>{flightKindLabel[e.kind] ?? e.kind}{e.weather_note && <><br /><small>{e.weather_note}</small></>}</td>
                  <td><small>{e.observed}<br /><span className="muted">Threshold {e.threshold}</span></small></td>
                  <td><Sev s={e.severity} /></td>
                  <td><GradeChip g={e.evidence_grade} />{e.correlated_events.length > 0 && <small className="ack">{e.correlated_events.length} linked fault{e.correlated_events.length > 1 ? 's' : ''}</small>}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : <Empty>No flight events match these filters.</Empty>}
    </div>
  );
}

/** Altitude profile with speed overlay. Chart-paper styling shared with the parameter charts. */
function TrackChart({ track, marks }: { track: TrackPoint[]; marks: { t: number; label: string }[] }) {
  const W = 760, H = 220, L = 52, R = 52, T = 12, B = 28;
  const tMax = Math.max(...track.map((p) => p.t_min), 1);
  const aMax = Math.max(...track.map((p) => p.alt_ft), 1000);
  const sMax = Math.max(...track.map((p) => p.speed_kt), 100);
  const x = (t: number) => L + (t / tMax) * (W - L - R);
  const ya = (a: number) => T + (1 - a / aMax) * (H - T - B);
  const ys = (s: number) => T + (1 - s / sMax) * (H - T - B);
  const alt = track.map((p) => `${x(p.t_min).toFixed(1)},${ya(p.alt_ft).toFixed(1)}`).join(' ');
  const spd = track.map((p) => `${x(p.t_min).toFixed(1)},${ys(p.speed_kt).toFixed(1)}`).join(' ');
  const ticks = [0, 0.25, 0.5, 0.75, 1];
  return (
    <figure className="chart">
      <svg viewBox={`0 0 ${W} ${H}`} role="img" aria-label={`Altitude profile, peak ${num(aMax)} ft, ${tMax} minutes`}>
        {ticks.map((f) => <line key={f} x1={L} x2={W - R} y1={T + f * (H - T - B)} y2={T + f * (H - T - B)} className="grid" />)}
        {ticks.map((f) => <text key={`a${f}`} x={L - 6} y={T + f * (H - T - B) + 4} textAnchor="end" className="axis">{num(aMax * (1 - f))}</text>)}
        {ticks.map((f) => <text key={`s${f}`} x={W - R + 6} y={T + f * (H - T - B) + 4} className="axis">{num(sMax * (1 - f))}</text>)}
        {ticks.map((f) => <text key={`t${f}`} x={x(f * tMax)} y={H - 8} textAnchor="middle" className="axis">{Math.round(f * tMax)} min</text>)}
        {marks.map((m, i) => <line key={i} x1={x(m.t)} x2={x(m.t)} y1={T} y2={H - B} className="mark-line"><title>{m.label}</title></line>)}
        <polyline points={spd} className="line-speed" fill="none" />
        <polyline points={alt} className="line-alt" fill="none" />
      </svg>
      <figcaption><span className="key key-alt" /> Altitude (ft, left) <span className="key key-speed" /> Ground speed (kt, right) <span className="key key-mark" /> Flight event</figcaption>
    </figure>
  );
}

function Wx({ w, label }: { w: Weather; label: string }) {
  return (
    <div className="wx">
      <h3>{label} {w.airport}</h3>
      <code>{w.metar}</code>
      <p><small>Wind {w.wind_dir}° {w.wind_kt} kt{w.gust_kt ? ` gusting ${w.gust_kt}` : ''}, visibility {w.visibility_m >= 9999 ? '10 km or more' : `${num(w.visibility_m)} m`}{w.ceiling_ft ? `, ceiling ${num(w.ceiling_ft)} ft` : ''}{w.convective ? ', convective activity' : ''}. Observed {utc(w.observed)}.</small></p>
    </div>
  );
}

function NotamList({ list }: { list: Notam[] }) {
  if (!list.length) return <p className="muted"><small>No NOTAMs in effect.</small></p>;
  return <ul className="plain-list">{list.map((n) => <li key={n.id}><strong>{n.id}</strong> {n.text}<br /><small>{utc(n.from)} to {utc(n.to)}</small></li>)}</ul>;
}

function FlightDetail({ flightId }: { flightId: string }) {
  const v = useApi<FlightView>(`/api/flight/${encodeURIComponent(flightId)}`);
  if (v.error) return <ErrorBox message={v.error} onRetry={v.reload} />;
  if (!v.data) return <Loading label="Reconstructing flight" />;
  const d = v.data;
  const dep = Date.parse(d.flight.dep);
  const marks = d.flight_events.map((e) => ({ t: Math.max(0, (Date.parse(e.at) - dep) / 60000), label: flightKindLabel[e.kind] ?? e.kind }));
  return (
    <div className="page">
      <a href="#/flight" className="back">Flight intelligence</a>
      <header className="page-head">
        <h1>{d.flight.number} {d.flight.origin}–{d.flight.dest}</h1>
        <p><a href={`#/aircraft/${d.flight.tail}`}>{d.flight.tail}</a>, cycle {num(d.flight.cycle)}. Off-block {utc(d.flight.dep)}, on-block {utc(d.flight.arr)}.</p>
      </header>
      <div className="grid-pf">
        <Panel title="Trajectory" className="span-2">
          {d.track && d.track.length > 1 ? <TrackChart track={d.track} marks={marks} /> : <Empty>No trajectory data received for this flight.</Empty>}
        </Panel>
        <Panel title="Data quality">
          {d.data_quality ? (
            <dl className="evidence">
              <div><dt>Source</dt><dd>{({ ads_b: 'ADS-B', qar: 'QAR', acars_position: 'ACARS position' } as Record<string, string>)[d.data_quality.source] ?? d.data_quality.source}</dd></div>
              <div><dt>Samples</dt><dd>{num(d.data_quality.samples)}</dd></div>
              <div><dt>Largest gap</dt><dd className={d.data_quality.complete ? '' : 'stale'}>{age(d.data_quality.max_gap_s)}</dd></div>
              <div><dt>Received</dt><dd>{utc(d.data_quality.received_at)} ({d.data_quality.delay_minutes} min after arrival)</dd></div>
            </dl>
          ) : <Empty>No trajectory summary.</Empty>}
        </Panel>
        <Panel title="Weather" className="span-2">
          <div className="grid-2"><Wx w={d.weather.departure} label="Departure" /><Wx w={d.weather.arrival} label="Arrival" /></div>
        </Panel>
        <Panel title="NOTAMs">
          <h3 className="sub">{d.flight.origin}</h3><NotamList list={d.notams.departure} />
          <h3 className="sub">{d.flight.dest}</h3><NotamList list={d.notams.arrival} />
        </Panel>
        <Panel title={`Flight events (${d.flight_events.length})`} className="span-3">
          {d.flight_events.length ? (
            <ol className="findings">
              {d.flight_events.map((e) => (
                <li key={e.id}>
                  <Sev s={e.severity} />
                  <div>
                    <strong>{flightKindLabel[e.kind] ?? e.kind}</strong>
                    <p>{e.observed} (threshold {e.threshold}) at {utc(e.at)}.{e.weather_note ? ` ${e.weather_note}` : ''}</p>
                    {e.correlation_note && <p><small>{e.correlation_note}</small></p>}
                    <p><small className="muted">{e.limitation}</small></p>
                  </div>
                  <span className="f-side"><GradeChip g={e.evidence_grade} /></span>
                </li>
              ))}
            </ol>
          ) : <Empty>No flight-data events on this flight.</Empty>}
        </Panel>
        <Panel title="Post-flight summary" className="span-3" aside={<span>{d.post_flight.status}</span>}>
          {d.post_flight.new_defects.length ? (
            <table className="table compact">
              <thead><tr><th>Time</th><th>Fault</th><th>Severity</th><th>History on this aircraft</th></tr></thead>
              <tbody>{d.post_flight.new_defects.map((x) => (
                <tr key={x.event.id}><td className="tabular">{utc(x.event.ts)}</td><td><strong>{x.event.code}</strong> {x.event.message}</td>
                  <td><Sev s={x.event.severity} /></td><td>{x.assessment} ({x.prior_occurrences_on_tail} earlier)</td></tr>
              ))}</tbody>
            </table>
          ) : <p className="muted">No defects recorded on this flight.</p>}
          {d.post_flight.corrective_actions.length > 0 && (
            <>
              <h3 className="sub">Corrective actions</h3>
              <ul className="plain-list">{d.post_flight.corrective_actions.map((w) => (
                <li key={w.id}><strong>{w.id}</strong> {w.action.replace(/_/g, ' ')} by {w.team}{w.closed ? `, closed ${utc(w.closed)}` : ', open'}{w.signed_off ? '' : ' (no sign-off recorded)'}</li>
              ))}</ul>
            </>
          )}
        </Panel>
      </div>
    </div>
  );
}
