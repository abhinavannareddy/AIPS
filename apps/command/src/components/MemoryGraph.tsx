import type { GraphView } from '../types';

const COLUMNS: Record<string, number> = { case: 0, hypothesis: 1, event: 2, work_order: 3, component: 3 };
const COL_TITLE = ['Investigations', 'Hypotheses', 'Fault events', 'Repairs and components'];

/** Layered left-to-right layout of the aircraft's memory graph; the focused hypothesis is highlighted. */
export function MemoryGraph({ g, focus, onSelect }: { g: GraphView; focus: string | null; onSelect: (hypId: string) => void }) {
  const nodes = g.nodes.filter((n) => n.kind in COLUMNS);
  const cols: typeof nodes[] = [[], [], [], []];
  nodes.forEach((n) => cols[COLUMNS[n.kind]].push(n));
  const rowH = 30;
  const colW = 210;
  const H = Math.max(160, Math.max(...cols.map((c) => c.length)) * rowH + 50);
  const W = colW * 4 + 20;
  const pos = new Map<string, { x: number; y: number }>();
  cols.forEach((c, ci) => {
    const offset = (H - 40 - c.length * rowH) / 2 + 36;
    c.forEach((n, ri) => pos.set(n.id, { x: 10 + ci * colW, y: offset + ri * rowH }));
  });
  const focusId = focus ? `h:${focus}` : null;
  const lit = new Set<string>();
  if (focusId) {
    lit.add(focusId);
    g.edges.forEach((e) => { if (e.from === focusId || e.to === focusId) { lit.add(e.from); lit.add(e.to); } });
    g.edges.forEach((e) => { if (lit.has(e.from) && (e.kind === 'INVOLVES' || e.kind === 'ADDRESSES')) lit.add(e.to); if (lit.has(e.to) && e.kind === 'ADDRESSES') lit.add(e.from); });
  }
  return (
    <div className="graph-wrap">
      <svg viewBox={`0 0 ${W} ${H}`} className="graph" role="img" aria-label="Cross-Flight Memory graph">
        {COL_TITLE.map((t, i) => <text key={t} x={10 + i * colW} y={18} className="graph-col">{t}</text>)}
        {g.edges.map((e, i) => {
          const a = pos.get(e.from), b = pos.get(e.to);
          if (!a || !b) return null;
          const [p, q] = a.x <= b.x ? [a, b] : [b, a];
          const x1 = p.x + 180, x2 = q.x, mx = (x1 + x2) / 2;
          const on = focusId ? lit.has(e.from) && lit.has(e.to) : true;
          return <path key={i} d={`M${x1},${p.y} C${mx},${p.y} ${mx},${q.y} ${x2},${q.y}`} className={`graph-edge ${on ? '' : 'dim'} edge-${e.kind}`} />;
        })}
        {nodes.map((n) => {
          const p = pos.get(n.id)!;
          const on = focusId ? lit.has(n.id) : true;
          const grade = (n.detail as { grade?: string }).grade;
          const click = n.kind === 'hypothesis' ? () => onSelect(String((n.detail as { id?: string }).id ?? n.id.slice(2))) : undefined;
          return (
            <g key={n.id} transform={`translate(${p.x},${p.y - 11})`} className={`gnode gnode-${n.kind} ${on ? '' : 'dim'} ${grade ? `g-${grade}` : ''}`}
              onClick={click} role={click ? 'button' : undefined} tabIndex={click ? 0 : undefined}
              onKeyDown={click ? (ev) => { if (ev.key === 'Enter') click(); } : undefined}>
              <rect width={180} height={22} rx={n.kind === 'hypothesis' ? 11 : 3} />
              <text x={10} y={15}>{n.label.length > 26 ? `${n.label.slice(0, 25)}…` : n.label}</text>
            </g>
          );
        })}
      </svg>
      <p className="footnote">Graph source: {g.source}. Rounded nodes are hypotheses; select one to trace its evidence.</p>
    </div>
  );
}
