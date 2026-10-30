import { useMemo, useState } from 'react';
import { Strip } from '../components/Strip';
import { Empty, ErrorBox, Loading } from '../components/ui';
import { useApi } from '../hooks';
import type { AircraftView } from '../types';

const FILTERS: { key: string; label: string; test: (a: AircraftView) => boolean }[] = [
  { key: 'all', label: 'All', test: () => true },
  { key: 'attention', label: 'Attention', test: (a) => a.status === 'attention' },
  { key: 'review', label: 'Review', test: (a) => a.status === 'review' },
  { key: 'insufficient_data', label: 'Insufficient data', test: (a) => a.status === 'insufficient_data' },
  { key: 'nominal', label: 'Nominal', test: (a) => a.status === 'nominal' },
];

export function FleetPage() {
  const fleet = useApi<AircraftView[]>('/api/fleet');
  const [filter, setFilter] = useState('all');
  const [q, setQ] = useState('');
  const [type, setType] = useState('');
  const types = useMemo(() => [...new Set((fleet.data ?? []).map((a) => a.type_code))].sort(), [fleet.data]);
  const rows = useMemo(() => {
    const f = FILTERS.find((x) => x.key === filter)!;
    const needle = q.trim().toUpperCase();
    return (fleet.data ?? []).filter((a) => f.test(a) && (!type || a.type_code === type)
      && (!needle || a.tail.includes(needle) || (a.next_departure?.number ?? '').includes(needle)));
  }, [fleet.data, filter, q, type]);

  if (fleet.error) return <ErrorBox message={fleet.error} onRetry={fleet.reload} />;
  if (!fleet.data) return <Loading label="Loading aircraft" />;

  return (
    <div className="page">
      <header className="page-head">
        <h1>Aircraft</h1>
        <p>Sorted by health score, lowest first. Select an aircraft to open its safety record.</p>
      </header>
      <div className="toolbar" role="search">
        <div className="segmented" role="group" aria-label="Filter by status">
          {FILTERS.map((f) => {
            const n = fleet.data!.filter(f.test).length;
            return <button key={f.key} aria-pressed={filter === f.key} onClick={() => setFilter(f.key)}>{f.label} <span className="tabular">{n}</span></button>;
          })}
        </div>
        <select aria-label="Aircraft type" value={type} onChange={(e) => setType(e.target.value)}>
          <option value="">All types</option>
          {types.map((t) => <option key={t}>{t}</option>)}
        </select>
        <input type="search" placeholder="Registration or flight" value={q} onChange={(e) => setQ(e.target.value)} aria-label="Search registration or flight" />
      </div>
      <div className="strip-head" aria-hidden>
        <span /><span>Registration</span><span>Next leg</span><span>Health</span><span>Alerts</span><span>Pre-departure</span><span>Data</span>
      </div>
      {rows.length ? <div className="strips">{rows.map((a) => <Strip key={a.tail} a={a} />)}</div>
        : <Empty>No aircraft match these filters. Clear the search or choose another status.</Empty>}
    </div>
  );
}
