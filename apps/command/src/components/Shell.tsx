import type { ReactNode } from 'react';
import { getConfig, getDevRole, logout, setDevRole } from '../api';
import { utc } from '../format';
import { useLive, usePrincipal } from '../hooks';
import type { Role } from '../types';

interface NavItem { key: string; label: string; roles?: Role[] }
const NAV: { group: string; items: NavItem[] }[] = [
  { group: 'Fleet', items: [{ key: 'overview', label: 'Overview' }, { key: 'fleet', label: 'Aircraft' }] },
  { group: 'Intelligence', items: [{ key: 'memory', label: 'Cross-Flight Memory' }, { key: 'alerts', label: 'Alerts', roles: ['engineer', 'mcc_controller', 'flight_ops', 'safety_manager'] },
    { key: 'flight', label: 'Flight intelligence', roles: ['engineer', 'mcc_controller', 'flight_ops', 'safety_manager'] },
    { key: 'reliability', label: 'Reliability', roles: ['engineer', 'mcc_controller', 'safety_manager', 'lessor'] }] },
  { group: 'Operations', items: [
    { key: 'preflight', label: 'Pre-departure', roles: ['engineer', 'mcc_controller', 'flight_ops', 'safety_manager'] },
    { key: 'cases', label: 'Investigations', roles: ['engineer', 'mcc_controller', 'flight_ops', 'safety_manager'] },
  ] },
  { group: 'Guardian', items: [{ key: 'guardian', label: 'Emergency drills', roles: ['safety_manager', 'flight_ops', 'engineer', 'mcc_controller', 'cabin_crew'] }] },
  { group: 'Assurance', items: [
    { key: 'metrics', label: 'Detection quality', roles: ['engineer', 'mcc_controller', 'safety_manager'] },
    { key: 'audit', label: 'Audit trail', roles: ['engineer', 'mcc_controller', 'safety_manager'] },
    { key: 'replay', label: 'Replay Lab' },
    { key: 'admin', label: 'Data administration', roles: ['admin'] },
  ] },
];

const ROLE_LABEL: Record<Role, string> = {
  engineer: 'Reliability engineer', mcc_controller: 'Maintenance control', flight_ops: 'Flight operations',
  safety_manager: 'Safety manager', cabin_crew: 'Cabin crew', lessor: 'Lessor (Aurora)', admin: 'Administrator',
};

export function Shell({ active, children }: { active: string; children: ReactNode }) {
  const p = usePrincipal();
  const { simNow, live, connected } = useLive();
  const cfg = getConfig();
  const allowed = (i: NavItem) => !i.roles || !p || p.roles.some((r) => r === 'admin' || i.roles!.includes(r));

  return (
    <div className="shell">
      <a className="skip" href="#main">Skip to content</a>
      <aside className="rail" aria-label="Primary">
        <div className="mark">
          <svg viewBox="0 0 32 32" width="28" height="28" aria-hidden><rect width="32" height="32" rx="7" /><path d="M6 21 16 7l10 14h-6l-4-6-4 6z" /></svg>
          <div><strong>AIPS</strong><span>Command</span></div>
        </div>
        <nav>
          {NAV.map((g) => {
            const items = g.items.filter(allowed);
            if (!items.length) return null;
            return (
              <div key={g.group} className="nav-group">
                <p>{g.group}</p>
                {items.map((i) => (
                  <a key={i.key} href={`#/${i.key}`} aria-current={active === i.key ? 'page' : undefined}>{i.label}</a>
                ))}
              </div>
            );
          })}
        </nav>
        <p className="rail-foot">Advisory system. Airworthiness and dispatch decisions remain with authorised personnel.</p>
      </aside>
      <div className="main-col">
        <header className="topbar">
          <div className={`clock ${!connected ? 'clock-lost' : live ? 'clock-live' : 'clock-replay'}`} aria-live="polite">
            <span className="clock-dot" aria-hidden />
            <span className="clock-time">{simNow ? utc(simNow) : '—'}</span>
            {/* P4: a dropped live connection is shown, never a frozen "Live feed". */}
            <span className="clock-mode">{!connected ? 'Connection lost · data may be stale' : live ? 'Live feed' : 'Replaying history'}</span>
          </div>
          <div className="who">
            {cfg.authMode === 'dev' ? (
              <label className="role-switch">
                <span>Viewing as</span>
                <select value={getDevRole()} onChange={(e) => { setDevRole(e.target.value as Role); window.location.reload(); }}>
                  {(Object.keys(ROLE_LABEL) as Role[]).map((r) => <option key={r} value={r}>{ROLE_LABEL[r]}</option>)}
                </select>
                <em title="Development sign-in. Production uses Keycloak single sign-on.">dev sign-in</em>
              </label>
            ) : (
              <>
                <span className="user">{p?.name}</span>
                <button className="btn btn-quiet" onClick={logout}>Sign out</button>
              </>
            )}
          </div>
        </header>
        <main id="main" tabIndex={-1}>{children}</main>
      </div>
    </div>
  );
}
