import { useEffect, useState } from 'react';
import { api, openStream } from '../api';
import { Empty, ErrorBox, Loading, Notice, Panel } from '../components/ui';
import { num, utc } from '../format';
import { can, go, useAction, useApi, usePrincipal } from '../hooks';
import type { AircraftView, Incident } from '../types';

const SCENARIOS: { key: string; label: string; note: string }[] = [
  { key: 'dual_incapacitation', label: 'Both pilots unresponsive after cockpit attack', note: 'Four independent indicators' },
  { key: 'single_incapacitation', label: 'One pilot medically incapacitated', note: 'Pilot monitoring report and alertness monitor' },
  { key: 'cockpit_security', label: 'Cockpit security event', note: 'Door access attempts and crew report' },
  { key: 'cabin_fire', label: 'Smoke in the aft cabin', note: 'Smoke detector and crew report' },
  { key: 'medical_emergency', label: 'Passenger medical emergency', note: 'Crew report and AED request' },
  { key: 'false_indicator', label: 'Single weak indicator (false trigger test)', note: 'Should not notify anyone' },
  { key: 'conflicting_indicators', label: 'Conflicting indicators', note: 'Flight deck answers a call while sensors suggest incapacitation' },
];
const STATE_LABEL: Record<string, string> = {
  monitoring: 'Monitoring, no notification', notifying: 'Notifying crew', acknowledged: 'Crew response confirmed',
  escalated: 'Escalated', cancelled: 'Cancelled', resolved: 'Resolved',
};
const CHANNEL_LABEL: Record<string, string> = {
  crew_device_push: 'Crew device push', cabin_interphone: 'Cabin interphone', airline_ops_messaging: 'Airline operations', atc_datalink: 'ATC (approved aircraft link)',
};
const DELIVERY_LABEL: Record<string, string> = { pending: 'Sending', delivered: 'Delivered (simulated)', failed: 'Failed', not_transmitted: 'Logged, not transmitted' };

export function GuardianPage({ incidentId }: { incidentId?: string }) {
  const [tick, setTick] = useState(0);
  useEffect(() => openStream('/api/emergency/stream', () => setTick((t) => t + 1)), []);
  return incidentId ? <IncidentView id={incidentId} tick={tick} /> : <DrillList tick={tick} />;
}

function DrillList({ tick }: { tick: number }) {
  const list = useApi<Incident[]>(`/api/emergency/drills?t=${Math.floor(tick / 3)}`, { live: false });
  const fleet = useApi<AircraftView[]>('/api/fleet', { live: false });
  const p = usePrincipal();
  const act = useAction();
  const [tail, setTail] = useState('');
  const [scenario, setScenario] = useState(SCENARIOS[0].key);
  const trainer = can(p, 'safety_manager', 'flight_ops', 'engineer', 'mcc_controller');
  const start = () => act.run(async () => {
    const ac = fleet.data?.find((a) => a.tail === tail) ?? fleet.data?.[0];
    if (!ac) throw new Error('Choose an aircraft');
    const inc = await api<Incident>('/api/emergency/drills', { method: 'POST', body: { tail: ac.tail, type_code: ac.type_code, flight: ac.next_departure?.number ?? '', dest: ac.next_departure?.dest ?? '', scenario, training: true } });
    go(`guardian/${inc.id}`);
  });
  return (
    <div className="page">
      <header className="page-head">
        <h1>Guardian emergency drills</h1>
        <p>Rehearse the onboard emergency workflow: detection, crew notification on two independent channels, acknowledgement tracking and escalation.</p>
      </header>
      <Notice tone="safety">Training mode only. Notification channels are simulated; nothing is sent to real crew, ATC or emergency services. Live mode is not available in Phase 0.</Notice>
      <div className="grid-2">
        {trainer && (
          <Panel title="Start a training drill">
            <div className="form-col">
              <label>Aircraft
                <select value={tail} onChange={(e) => setTail(e.target.value)}>
                  <option value="">Choose aircraft</option>
                  {(fleet.data ?? []).map((a) => <option key={a.tail} value={a.tail}>{a.tail} ({a.type_code})</option>)}
                </select>
              </label>
              <fieldset className="radio-list">
                <legend>Scenario</legend>
                {SCENARIOS.map((s) => (
                  <label key={s.key}><input type="radio" name="scenario" value={s.key} checked={scenario === s.key} onChange={() => setScenario(s.key)} /><span>{s.label}<small>{s.note}</small></span></label>
                ))}
              </fieldset>
              <button className="btn" onClick={start} disabled={act.pending || !tail}>Start training drill</button>
              {act.error && <p className="err" role="alert">{act.error}</p>}
            </div>
          </Panel>
        )}
        <Panel title="Drills">
          {list.error ? <ErrorBox message={list.error} onRetry={list.reload} /> : !list.data ? <Loading /> : list.data.length ? (
            <ul className="plain-list">
              {list.data.map((i) => (
                <li key={i.id}><a href={`#/guardian/${i.id}`}><strong>{SCENARIOS.find((s) => s.key === i.scenario)?.label ?? i.scenario}</strong></a>
                  <p>{i.tail}, started {utc(i.created_at)} by {i.created_by}. <span className={`istate is-${i.state}`}>{STATE_LABEL[i.state]}</span></p></li>
              ))}
            </ul>
          ) : <Empty>No drills yet. Start one to rehearse the workflow.</Empty>}
        </Panel>
      </div>
    </div>
  );
}

function IncidentView({ id, tick }: { id: string; tick: number }) {
  const r = useApi<Incident>(`/api/emergency/drills/${id}?t=${tick}`, { live: false });
  const [verify, setVerify] = useState<{ intact: boolean; entries: number } | null>(null);
  const [reason, setReason] = useState('');
  const [now, setNow] = useState(Date.now());
  const p = usePrincipal();
  const act = useAction();
  useEffect(() => { const t = window.setInterval(() => setNow(Date.now()), 500); return () => window.clearInterval(t); }, []);
  if (r.error) return <ErrorBox message={r.error} onRetry={r.reload} />;
  if (!r.data) return <Loading label="Loading drill" />;
  const i = r.data;
  const active = ['monitoring', 'notifying', 'acknowledged', 'escalated'].includes(i.state);
  const post = (path: string, body: unknown) => act.run(async () => { await api(`/api/emergency/drills/${id}/${path}`, { method: 'POST', body }); r.reload(); });
  const crewActor = can(p, 'cabin_crew', 'safety_manager', 'flight_ops');
  const trainer = can(p, 'safety_manager', 'flight_ops', 'engineer', 'mcc_controller');
  const remaining = i.ack_deadline ? Math.max(0, (Date.parse(i.ack_deadline) - now) / 1000) : null;

  return (
    <div className="page">
      <a href="#/guardian" className="back">Emergency drills</a>
      <div className={`drill-banner is-${i.state}`} role="status">
        <span className="drill-tag">Training drill</span>
        <strong>{STATE_LABEL[i.state]}</strong>
        <span>{i.tail} ({i.type_code}) {i.flight}</span>
        {i.state === 'notifying' && remaining !== null && <span className="tabular">Escalates in {Math.ceil(remaining)} s without a confirmed response</span>}
        {i.feed_lost && <span className="feed-lost">Aircraft data lost {Math.round((now - Date.parse(i.last_data_at)) / 1000)} s ago; emergency state preserved</span>}
      </div>

      <ol className="stepper" aria-label="Emergency response stages">
        {i.steps.map((s) => (
          <li key={s.stage} className={`step step-${s.status}`}>
            <span className="step-n">{s.stage}</span>
            <div><strong>{s.name}</strong><p>{s.detail || (s.status === 'pending' ? 'Waiting' : '')}</p>{s.at && <small>{utc(s.at)}</small>}</div>
          </li>
        ))}
      </ol>

      <div className="grid-2">
        <Panel title="Detection" aside={<span className="tabular">{num(i.confidence * 100)}% confidence, {i.independent_classes} independent classes</span>}>
          <div className="meter" aria-label="Detection confidence"><i style={{ width: `${i.confidence * 100}%` }} /><span style={{ left: '90%' }} title="Qualification threshold 90%" /></div>
          <p className="footnote">Qualifies only at 90% fused confidence with at least two independent indicator classes and no contradicting evidence. {i.qualified ? 'This detection qualified.' : 'This detection did not qualify, so no one was notified.'}</p>
          {i.disagreement && <Notice tone="caution"><strong>Indicators disagree.</strong> {i.disagreement}</Notice>}
          <table className="table compact">
            <thead><tr><th>Indicator</th><th>Source</th><th>Reading</th><th className="num">Weight</th></tr></thead>
            <tbody>{i.indicators.map((x) => <tr key={x.name} className={x.contradicts ? 'contradicts' : ''}><td>{x.contradicts && <strong className="err">Against: </strong>}{x.name}</td><td>{x.source}</td><td>{x.value}</td><td className="num tabular">{x.weight.toFixed(2)}</td></tr>)}</tbody>
          </table>
        </Panel>

        <Panel title="Cabin crew" aside={<span>Escalation level {i.escalation_level}</span>}>
          <table className="table compact">
            <thead><tr><th>Crew</th><th>Position</th><th>Notified</th><th>Response</th><th /></tr></thead>
            <tbody>
              {i.crew.map((c) => (
                <tr key={c.id}>
                  <td><strong>{c.id}</strong> {c.name}{c.designated && <small className="desig">designated</small>}</td>
                  <td>{c.position}</td>
                  <td>{c.notified ? 'Yes' : 'No'}</td>
                  <td>{c.acknowledged_at ? (c.available ? `Available, ${utc(c.acknowledged_at, false)}` : 'Acknowledged, unavailable') : <span className="muted">No response</span>}</td>
                  <td>{active && c.notified && !c.acknowledged_at && crewActor && (
                    <span className="btn-row tight">
                      <button className="btn btn-small" onClick={() => post('ack', { crew_id: c.id, available: true })}>Available</button>
                      <button className="btn btn-small btn-quiet" onClick={() => post('ack', { crew_id: c.id, available: false })}>Unavailable</button>
                    </span>
                  )}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <p className="footnote">Being on board does not count as available. Only an acknowledgement that confirms availability stops escalation.</p>
        </Panel>

        <Panel title="Notifications">
          {i.deliveries.length ? (
            <table className="table compact">
              <thead><tr><th>Channel</th><th>Recipient</th><th>Status</th><th>Sent</th></tr></thead>
              <tbody>{i.deliveries.map((d, n) => (
                <tr key={n}><td>{CHANNEL_LABEL[d.channel] ?? d.channel}</td><td>{d.recipient}</td><td className={`dl dl-${d.status}`}>{DELIVERY_LABEL[d.status]}</td><td className="tabular">{utc(d.sent_at, false)}</td></tr>
              ))}</tbody>
            </table>
          ) : <Empty>No notifications sent.</Empty>}
        </Panel>

        {i.procedure && (
          <Panel title="Procedure card" aside={i.procedure.training_example ? <span className="err">Training example</span> : undefined}>
            <h3 className="sub">{i.procedure.title}</h3>
            <ol className="proc-steps">{i.procedure.steps.map((s, n) => <li key={n}>{s}</li>)}</ol>
            <p className="footnote">{i.procedure.source}</p>
          </Panel>
        )}
        {i.atc_message && (
          <Panel title="ATC and operations message" aside={<span className="err">{i.atc_message.status}</span>}>
            <pre className="atc-text">{i.atc_message.text}</pre>
            {i.atc_message.destination_metar && <p><small>Destination weather: <code>{i.atc_message.destination_metar}</code></small></p>}
            {i.atc_message.alternates.length > 0 && <ul className="plain-list">{i.atc_message.alternates.map((a) => <li key={a.icao}><strong>{a.icao}</strong> {a.name} <small><code>{a.metar}</code></small></li>)}</ul>}
          </Panel>
        )}
        <Panel title="Drill controls">
          {active ? (
            <div className="form-col">
              {trainer && (
                <div className="btn-row">
                  <button className="btn btn-quiet" onClick={() => post('simulate', { action: i.feed_lost ? 'feed_restore' : 'feed_loss' })}>{i.feed_lost ? 'Restore aircraft data' : 'Simulate data loss'}</button>
                  <button className="btn btn-quiet" onClick={() => post('simulate', { action: 'channel_failure', channel: 'crew_device_push' })} disabled={!!i.failed_channel}>Fail crew device channel</button>
                </div>
              )}
              <label htmlFor="close-reason">Reason</label>
              <input id="close-reason" value={reason} onChange={(e) => setReason(e.target.value)} placeholder="Why the drill is being cancelled or resolved" />
              <div className="btn-row">
                {can(p, 'cabin_crew', 'safety_manager', 'flight_ops') && <button className="btn btn-danger" onClick={() => post('cancel', { reason })}>Cancel as false trigger</button>}
                {can(p, 'safety_manager', 'flight_ops') && <button className="btn" onClick={() => post('resolve', { reason })}>Resolve drill</button>}
              </div>
            </div>
          ) : <p>Closed by {i.closed_by}: {i.closed_reason}</p>}
          {act.error && <p className="err" role="alert">{act.error}</p>}
        </Panel>

        <Panel title={`Tamper-evident journal (${i.journal.entries.length})`} aside={
          <button className="btn btn-small btn-quiet" onClick={() => act.run(async () => setVerify(await api(`/api/emergency/drills/${id}/verify`)))}>Verify chain</button>} className="span-2">
          {verify && <p className={verify.intact ? 'ok' : 'err'}>{verify.intact ? `Chain intact: ${verify.entries} entries verified.` : 'Chain broken. Investigate before relying on this record.'}</p>}
          <ol className="journal">
            {[...i.journal.entries].reverse().map((e) => (
              <li key={e.seq}><span className="tabular">#{e.seq} {utc(e.at, false)}</span><strong>{e.action}</strong><span>{e.actor}</span><code title={e.hash}>{e.hash.slice(0, 12)}</code></li>
            ))}
          </ol>
        </Panel>
      </div>
    </div>
  );
}
