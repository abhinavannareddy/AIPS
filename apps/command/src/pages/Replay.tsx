import { useState } from 'react';
import { api } from '../api';
import { ErrorBox, Kv, Loading, Notice, Panel } from '../components/ui';
import { utc } from '../format';
import { can, useAction, useApi, usePrincipal } from '../hooks';
import type { AircraftView, Clock } from '../types';

export function ReplayPage() {
  const s = useApi<{ clock: Clock; live: boolean; seed: number; scenarios: number; generated_at: string }>('/api/replay');
  const fleet = useApi<AircraftView[]>('/api/fleet', { live: false });
  const p = usePrincipal();
  const act = useAction();
  const [hours, setHours] = useState(24);
  const [speed, setSpeed] = useState(240);
  const [seed, setSeed] = useState(20261031);
  const [ev, setEv] = useState({ tail: '', ata: 29, code: '29-11-07', message: 'HYD G ENG 1 PUMP LO PR', severity: 'caution' });
  const [ingested, setIngested] = useState<string | null>(null);
  const control = (body: unknown) => act.run(async () => { await api('/api/replay/control', { method: 'POST', body }); s.reload(); });
  if (s.error) return <ErrorBox message={s.error} onRetry={s.reload} />;
  if (!s.data) return <Loading />;
  const c = s.data.clock;
  const operator = can(p, 'engineer', 'safety_manager');

  const ingest = () => act.run(async () => {
    const now = new Date(Date.parse(c.sim_now)).toISOString();
    const id = `EV-MANUAL-${Date.now()}`;
    const r = await api<{ via: string }>('/api/ingest/events', { method: 'POST', body: {
      id, tail: ev.tail || fleet.data?.[0]?.tail, flight_id: null, ts: now, ata: Number(ev.ata), system: 'other', code: ev.code,
      component_id: null, severity: ev.severity, message: ev.message, source: 'tech_log', ingested_at: now,
    } });
    setIngested(`Event ${id} accepted via ${r.via}. Analysis will reflect it on the next pass.`);
  });

  return (
    <div className="page">
      <header className="page-head">
        <h1>Replay Lab</h1>
        <p>A synthetic 50-aircraft fleet with {s.data.scenarios} hidden fault scenarios. Replay history at speed to watch AIPS detect them.</p>
      </header>
      <Notice>All operators, registrations, part numbers and serials here are fictional. Phase 0 uses synthetic data only.</Notice>
      <div className="grid-2">
        <Panel title="Simulation clock">
          <dl className="kv-list">
            <Kv k="Simulation time" v={utc(c.sim_now)} />
            <Kv k="End of recorded data" v={utc(c.data_end)} />
            <Kv k="Mode" v={s.data.live ? 'Live (wall clock)' : `Replaying at ${c.speed}× ${c.running ? '' : '(paused)'}`} />
            <Kv k="Dataset seed" v={s.data.seed} />
          </dl>
          {operator ? (
            <div className="form-row">
              <label>Replay the last<select value={hours} onChange={(e) => setHours(Number(e.target.value))}>{[6, 24, 72, 168].map((h) => <option key={h} value={h}>{h < 48 ? `${h} hours` : `${h / 24} days`}</option>)}</select></label>
              <label>Speed<select value={speed} onChange={(e) => setSpeed(Number(e.target.value))}>{[60, 240, 900, 3600].map((x) => <option key={x} value={x}>{x}×</option>)}</select></label>
              <button className="btn" onClick={() => control({ action: 'replay', hours, speed })} disabled={act.pending}>Start replay</button>
              {c.running ? <button className="btn btn-quiet" onClick={() => control({ action: 'pause' })}>Pause</button>
                : <button className="btn btn-quiet" onClick={() => control({ action: 'resume' })}>Resume</button>}
              <button className="btn btn-quiet" onClick={() => control({ action: 'live' })}>Jump to live</button>
            </div>
          ) : <p className="footnote">Engineers and safety managers can control the replay.</p>}
          {can(p, 'admin') && (
            <div className="form-row">
              <label>New dataset seed<input type="number" value={seed} onChange={(e) => setSeed(Number(e.target.value))} /></label>
              <button className="btn btn-danger" onClick={() => act.run(async () => { await api('/api/replay/regenerate', { method: 'POST', body: { seed } }); s.reload(); })}>Regenerate fleet</button>
            </div>
          )}
        </Panel>
        {can(p, 'engineer') && (
          <Panel title="Ingest a fault report">
            <p className="footnote">Sends an event through the ingestion path (Kafka when connected). Repeat a code three times on one aircraft to watch a recurrence form.</p>
            <div className="form-col">
              <label>Aircraft<select value={ev.tail} onChange={(e) => setEv({ ...ev, tail: e.target.value })}>{(fleet.data ?? []).map((a) => <option key={a.tail}>{a.tail}</option>)}</select></label>
              <div className="form-row">
                <label>ATA chapter<input type="number" min={5} max={99} value={ev.ata} onChange={(e) => setEv({ ...ev, ata: Number(e.target.value) })} /></label>
                <label>Fault code<input value={ev.code} onChange={(e) => setEv({ ...ev, code: e.target.value })} /></label>
                <label>Severity<select value={ev.severity} onChange={(e) => setEv({ ...ev, severity: e.target.value })}><option value="advisory">Advisory</option><option value="caution">Caution</option><option value="warning">Warning</option></select></label>
              </div>
              <label>Message<input value={ev.message} onChange={(e) => setEv({ ...ev, message: e.target.value })} /></label>
              <button className="btn" onClick={ingest} disabled={act.pending}>Send fault report</button>
              {ingested && <p className="ok">{ingested}</p>}
            </div>
          </Panel>
        )}
      </div>
      {act.error && <ErrorBox message={act.error} />}
    </div>
  );
}
