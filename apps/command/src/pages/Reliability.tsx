import { Empty, ErrorBox, Loading, Notice, Panel } from '../components/ui';
import { num } from '../format';
import { can, useApi, usePrincipal } from '../hooks';
import type { FleetPatterns, RelRow, Reliability } from '../types';

function RelTable({ rows, keyLabel }: { rows: RelRow[]; keyLabel: string }) {
  const sorted = [...rows].sort((a, b) => a.effectiveness - b.effectiveness);
  return (
    <table className="table compact">
      <thead><tr><th>{keyLabel}</th><th className="num">Actions</th><th className="num">Repeat ≤30 d</th><th>Effectiveness</th><th className="num">Open</th><th className="num">No sign-off</th><th className="num">Median days</th></tr></thead>
      <tbody>{sorted.map((r) => (
        <tr key={String(r.key)}>
          <td><strong>{keyLabel === 'ATA' ? `ATA ${r.key}` : r.key}</strong></td>
          <td className="num tabular">{r.corrective_actions}</td>
          <td className="num tabular">{r.repeat_within_30d}</td>
          <td><span className="meter" role="img" aria-label={`${num(r.effectiveness * 100)} percent`}><i style={{ width: `${r.effectiveness * 100}%` }} className={r.effectiveness < 0.7 ? 'meter-low' : ''} /></span> <span className="tabular">{num(r.effectiveness * 100)}%</span></td>
          <td className="num tabular">{r.open}</td>
          <td className={`num tabular ${r.closed_without_signoff ? 'err' : ''}`}>{r.closed_without_signoff}</td>
          <td className="num tabular">{r.median_days_to_close === null ? '—' : num(r.median_days_to_close, 1)}</td>
        </tr>
      ))}</tbody>
    </table>
  );
}

export function ReliabilityPage() {
  const p = usePrincipal();
  const internal = can(p, 'engineer', 'mcc_controller', 'safety_manager');
  const r = useApi<Reliability>(internal ? '/api/reliability' : null);
  const f = useApi<FleetPatterns>('/api/memory/fleet-patterns');
  return (
    <div className="page">
      <header className="page-head">
        <h1>Reliability</h1>
        <p>How often corrective actions hold, by ATA chapter and team, and fleet-level patterns that can be shared without identifying any aircraft.</p>
      </header>
      {internal && (r.error ? <ErrorBox message={r.error} onRetry={r.reload} /> : !r.data ? <Loading label="Computing reliability" /> : (
        <>
          <Notice tone="info">{r.data.definition} {r.data.limitation}</Notice>
          <div className="grid-2">
            <Panel title="By ATA chapter (weakest first)"><RelTable rows={r.data.by_ata} keyLabel="ATA" /></Panel>
            <Panel title="By maintenance team"><RelTable rows={r.data.by_team} keyLabel="Team" /></Panel>
          </div>
        </>
      ))}
      <Panel title="Fleet-level patterns" aside={f.data ? `k = ${f.data.k_anonymity}; ${f.data.suppressed_groups} small groups suppressed` : undefined}>
        {f.error ? <ErrorBox message={f.error} onRetry={f.reload} /> : !f.data ? <Loading label="Aggregating" /> : f.data.patterns.length ? (
          <>
            <table className="table compact">
              <thead><tr><th>Type</th><th>Pattern</th><th>Systems</th><th className="num">Aircraft</th><th className="num">Patterns</th><th className="num">Engineering-confirmed</th></tr></thead>
              <tbody>{f.data.patterns.map((x, i) => (
                <tr key={i}><td>{x.type_code}</td><td>{x.kind.replace(/_/g, ' ')}</td><td>{x.systems.replace(/_/g, ' ')}</td>
                  <td className="num tabular">{x.aircraft}</td><td className="num tabular">{x.patterns}</td><td className="num tabular">{x.engineering_confirmed}</td></tr>
              ))}</tbody>
            </table>
            <p className="footnote">{f.data.note}</p>
          </>
        ) : <Empty>No pattern is shared by enough aircraft to be disclosed.</Empty>}
      </Panel>
    </div>
  );
}
