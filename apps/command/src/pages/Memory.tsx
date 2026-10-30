import { useMemo, useState } from 'react';
import { api } from '../api';
import { MemoryGraph } from '../components/MemoryGraph';
import { Empty, ErrorBox, GradeChip, Loading, Notice, Panel, Sev } from '../components/ui';
import { num, systemLabel, utc } from '../format';
import { can, go, useAction, useApi, usePrincipal } from '../hooks';
import type { Grade, GraphView, HypothesisBundle } from '../types';

const KIND_LABEL = { recurrence: 'Recurring defect', repeat_after_fix: 'Repeat after repair', cross_system: 'Cross-system link' } as const;

export function MemoryPage({ tail, focus }: { tail: string | null; focus: string | null }) {
  const all = useApi<HypothesisBundle[]>('/api/memory/hypotheses');
  const [gradeFilter, setGradeFilter] = useState<string>('');
  const list = useMemo(() => (all.data ?? []).filter((b) => (!tail || b.hypothesis.tail === tail) && (!gradeFilter || b.hypothesis.grade === gradeFilter)), [all.data, tail, gradeFilter]);
  const selected = (all.data ?? []).find((b) => b.hypothesis.id === focus) ?? (tail ? list[0] : undefined);
  const graphTail = selected?.hypothesis.tail ?? tail;
  const graph = useApi<GraphView>(graphTail ? `/api/memory/graph?tail=${encodeURIComponent(graphTail)}&source=neo4j` : null);

  if (all.error) return <ErrorBox message={all.error} onRetry={all.reload} />;
  if (!all.data) return <Loading label="Loading Cross-Flight Memory" />;
  const tails = [...new Set(all.data.map((b) => b.hypothesis.tail))].sort();

  return (
    <div className="page">
      <header className="page-head">
        <h1>Cross-Flight Memory</h1>
        <p>Links between faults, repairs and recurrences across flights. Every link is graded by its evidence; only an engineer can confirm one.</p>
      </header>
      <Notice tone="caution">A suspected or corroborated link is not an established engineering cause. Treat it as a lead for investigation.</Notice>
      <div className="toolbar">
        <select aria-label="Aircraft" value={tail ?? ''} onChange={(e) => go(e.target.value ? `memory?tail=${e.target.value}` : 'memory')}>
          <option value="">All aircraft ({all.data.length} hypotheses)</option>
          {tails.map((t) => <option key={t} value={t}>{t}</option>)}
        </select>
        <select aria-label="Evidence grade" value={gradeFilter} onChange={(e) => setGradeFilter(e.target.value)}>
          <option value="">All grades</option><option value="suspected">Suspected</option><option value="corroborated">Corroborated</option>
          <option value="engineering_confirmed">Engineering-confirmed</option><option value="rejected">Rejected</option>
        </select>
      </div>
      <div className="memory">
        <Panel title="Hypotheses" className="memory-list">
          {list.length ? (
            <ul className="hyp-list">
              {list.map((b) => {
                const h = b.hypothesis;
                return (
                  <li key={h.id}>
                    <button aria-current={selected?.hypothesis.id === h.id} onClick={() => go(`memory?tail=${h.tail}&h=${h.id}`)}>
                      <span className="hyp-top"><strong>{h.tail}</strong><GradeChip g={h.grade} /></span>
                      <span className="hyp-title">{h.title}</span>
                      <small>{KIND_LABEL[h.kind]}, {h.explanation.occurrences} occurrences, last {utc(h.explanation.last_seen)}</small>
                    </button>
                  </li>
                );
              })}
            </ul>
          ) : <Empty>No hypotheses for this selection.</Empty>}
        </Panel>
        <div className="memory-detail">
          {graph.data && <Panel title={`Memory graph for ${graphTail}`}><MemoryGraph g={graph.data} focus={selected?.hypothesis.id ?? null} onSelect={(id) => go(`memory?tail=${graphTail}&h=${id}`)} /></Panel>}
          {graph.error && <ErrorBox message={graph.error} />}
          {selected ? <HypothesisDetail b={selected} onChange={all.reload} /> : <Empty>Select a hypothesis to see why its events were linked.</Empty>}
        </div>
      </div>
    </div>
  );
}

function HypothesisDetail({ b, onChange }: { b: HypothesisBundle; onChange: () => void }) {
  const h = b.hypothesis;
  const x = h.explanation;
  const p = usePrincipal();
  const act = useAction();
  const [note, setNote] = useState('');
  const isEngineer = can(p, 'engineer');
  const grade = (g: Grade) => act.run(async () => {
    await api(`/api/memory/hypotheses/${h.id}/grade`, { method: 'POST', body: { grade: g, note } });
    setNote(''); onChange();
  });
  return (
    <Panel title={h.title} aside={<GradeChip g={h.grade} />}>
      <p className="lede">{x.narrative}</p>
      <div className="why">
        <h3>Why these events were linked</h3>
        <dl className="why-grid">
          <div><dt>Matched on</dt><dd>{x.matching_fields.map((f) => f.replace('_', ' ')).join(', ')}</dd></div>
          <div><dt>Occurrences</dt><dd className="tabular">{x.occurrences}</dd></div>
          <div><dt>Period</dt><dd>{utc(x.first_seen)} to {utc(x.last_seen)}</dd></div>
          <div><dt>Longest gap</dt><dd className="tabular">{num(x.max_gap_hours, 0)} h</dd></div>
          {x.median_gap_minutes !== null && <div><dt>Median separation</dt><dd className="tabular">{num(x.median_gap_minutes, 0)} min</dd></div>}
          {x.lift !== null && <div><dt>Versus chance</dt><dd>{num(x.expected_by_chance, 2)} expected by fleet base rate; observed {x.occurrences} (lift {num(x.lift, 0)}×)</dd></div>}
          {x.symptom_similarity !== null && <div><dt>Symptom text similarity</dt><dd className="tabular">{num(x.symptom_similarity * 100)}%</dd></div>}
          <div><dt>Repairs in between</dt><dd className="tabular">{x.fixes_between}</dd></div>
          <div><dt>Systems</dt><dd>{h.systems.map((s) => systemLabel[s]).join(' and ')}</dd></div>
          <div><dt>Engine proposal</dt><dd><GradeChip g={h.machine_grade} /> {h.model_version}</dd></div>
          {h.graded_by && <div><dt>Human decision</dt><dd>{h.graded_by}, {utc(h.graded_at)}</dd></div>}
        </dl>
      </div>
      <h3 className="sub">Evidence</h3>
      <table className="table compact">
        <thead><tr><th>Time</th><th>Code</th><th>Message</th><th>Source</th><th>Severity</th></tr></thead>
        <tbody>
          {b.events.map((e) => (
            <tr key={e.id}><td className="tabular">{utc(e.ts)}</td><td><strong>{e.code}</strong></td><td>{e.message}</td><td>{e.source.replace(/_/g, ' ')}</td><td><Sev s={e.severity} /></td></tr>
          ))}
          {b.work_orders.map((w) => (
            <tr key={w.id} className="row-wo"><td className="tabular">{utc(w.closed ?? w.opened)}</td><td>{w.id}</td><td>{w.action}: {w.note}</td><td>{w.team}</td><td>{w.signed_off ? 'Signed off' : 'No sign-off'}</td></tr>
          ))}
        </tbody>
      </table>
      {isEngineer ? (
        <div className="grade-form">
          <h3>Engineering assessment</h3>
          <label htmlFor="grade-note">Justification (required to confirm)</label>
          <textarea id="grade-note" rows={2} value={note} onChange={(e) => setNote(e.target.value)} placeholder="What evidence supports or rules out this link?" />
          <div className="btn-row">
            <button className="btn" disabled={act.pending} onClick={() => grade('engineering_confirmed')}>Confirm as engineering cause</button>
            <button className="btn btn-quiet" disabled={act.pending} onClick={() => grade('corroborated')}>Mark corroborated</button>
            <button className="btn btn-quiet" disabled={act.pending} onClick={() => grade('suspected')}>Return to suspected</button>
            <button className="btn btn-danger" disabled={act.pending} onClick={() => grade('rejected')}>Reject link</button>
          </div>
          {act.error && <p className="err" role="alert">{act.error}</p>}
        </div>
      ) : <p className="footnote">Only reliability engineers can change an evidence grade.</p>}
    </Panel>
  );
}
