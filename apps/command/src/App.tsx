import { useEffect, useRef, useState } from 'react';
import { api, openStream } from './api';
import { Shell } from './components/Shell';
import { ErrorBox, Loading } from './components/ui';
import { LiveContext, PrincipalContext, useHashRoute } from './hooks';
import { AdminPage } from './pages/Admin';
import { AircraftPage } from './pages/Aircraft';
import { AlertsPage } from './pages/Alerts';
import { AuditPage } from './pages/Audit';
import { CasesPage } from './pages/Cases';
import { FleetPage } from './pages/Fleet';
import { FlightPage } from './pages/Flight';
import { GuardianPage } from './pages/Guardian';
import { MemoryPage } from './pages/Memory';
import { MetricsPage } from './pages/Metrics';
import { OverviewPage } from './pages/Overview';
import { PreflightPage } from './pages/Preflight';
import { ReliabilityPage } from './pages/Reliability';
import { ReplayPage } from './pages/Replay';
import type { Principal } from './types';

export function App() {
  const [principal, setPrincipal] = useState<Principal | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [live, setLive] = useState({ version: 0, simNow: null as string | null, live: false, connected: true });
  const pending = useRef<{ simNow: string; live: boolean } | null>(null);
  const [parts, query] = useHashRoute();

  useEffect(() => {
    api<Principal>('/api/me').then(setPrincipal).catch((e: Error) => setError(e.message));
    api<{ sim_now: string; live: boolean }>('/api/health').then((h) => setLive((l) => ({ ...l, simNow: h.sim_now, live: h.live }))).catch(() => undefined);
    // Throttle refreshes: at most one re-fetch wave every 4 s, however fast snapshots arrive.
    const close = openStream('/api/stream', (m) => {
      const msg = m as { type: string; sim_now: string; live: boolean };
      if (msg.type === 'snapshot') pending.current = { simNow: msg.sim_now, live: msg.live };
    }, (connected) => setLive((l) => (l.connected === connected ? l : { ...l, connected })));
    const timer = window.setInterval(() => {
      const p = pending.current;
      if (p) { pending.current = null; setLive((l) => ({ ...l, version: l.version + 1, simNow: p.simNow, live: p.live })); }
    }, 4000);
    return () => { close(); window.clearInterval(timer); };
  }, []);

  if (error) return <div className="boot"><ErrorBox message={error} /></div>;
  if (!principal) return <div className="boot"><Loading label="Signing in" /></div>;

  const [page, id] = parts;
  let view;
  switch (page) {
    case 'fleet': view = <FleetPage />; break;
    case 'aircraft': view = <AircraftPage tail={id} />; break;
    case 'memory': view = <MemoryPage tail={query.get('tail')} focus={query.get('h')} />; break;
    case 'alerts': view = <AlertsPage />; break;
    case 'flight': view = <FlightPage flightId={id} />; break;
    case 'reliability': view = <ReliabilityPage />; break;
    case 'admin': view = <AdminPage />; break;
    case 'preflight': view = <PreflightPage flightId={id} />; break;
    case 'cases': view = <CasesPage caseId={id} />; break;
    case 'guardian': view = <GuardianPage incidentId={id} />; break;
    case 'metrics': view = <MetricsPage />; break;
    case 'audit': view = <AuditPage />; break;
    case 'replay': view = <ReplayPage />; break;
    default: view = principal.roles.includes('cabin_crew') && principal.roles.length === 1 ? <GuardianPage /> : <OverviewPage />;
  }

  return (
    <PrincipalContext.Provider value={principal}>
      <LiveContext.Provider value={live}>
        <Shell active={page === 'aircraft' ? 'fleet' : page}>{view}</Shell>
      </LiveContext.Provider>
    </PrincipalContext.Provider>
  );
}
