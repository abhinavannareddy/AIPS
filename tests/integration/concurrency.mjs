// Concurrency and race tests against a running aips-core (dev auth, PostgreSQL).
// Usage: node tests/integration/concurrency.mjs [coreUrl]
const CORE = process.argv[2] ?? 'http://localhost:8090';
const h = (role, user = `race.${role}`) => ({ 'x-aips-role': role, 'x-aips-user': user, 'content-type': 'application/json' });
const call = async (path, role, method = 'GET', body, user) => {
  const r = await fetch(CORE + path, { method, headers: h(role, user), body: body ? JSON.stringify(body) : undefined });
  let json = null; try { json = await r.json(); } catch { /* */ }
  return { status: r.status, json };
};
let failed = 0;
const check = (c, msg) => { console.log(`  ${c ? 'PASS' : 'FAIL'}  ${msg}`); if (!c) failed++; };

console.log('AIPS concurrency tests');
const alerts = (await call('/api/alerts', 'engineer')).json.filter((a) => !a.acknowledged).slice(0, 5);
// 1. 40 simultaneous acknowledgements per alert: exactly one wins, the rest get 409.
for (const a of alerts) {
  const rs = await Promise.all(Array.from({ length: 40 }, (_, i) => call(`/api/alerts/${a.id}/acknowledge`, 'mcc_controller', 'POST', undefined, `race.u${i}`)));
  const ok = rs.filter((r) => r.status === 200).length;
  const conflict = rs.filter((r) => r.status === 409).length;
  check(ok === 1 && conflict === 39, `alert ${a.id.slice(0, 18)}…: ${ok} winner, ${conflict} conflicts of 40 concurrent acknowledgements`);
}
// 2. Mixed concurrent decisions (feedback, case assignment, corrections, reads) keep the audit chain intact.
const cases = (await call('/api/cases', 'engineer')).json.slice(0, 10);
const fleet = (await call('/api/fleet', 'engineer')).json;
const tl = (await call(`/api/aircraft/${fleet[1].tail}/timeline`, 'engineer')).json.filter((x) => x.kind === 'fault').slice(0, 10);
const ops = [];
for (let i = 0; i < 200; i++) {
  const k = i % 5;
  if (k === 0 && cases.length) ops.push(call(`/api/cases/${cases[i % cases.length].id}/assign`, 'mcc_controller', 'POST', { assignee: `eng.${i}` }));
  else if (k === 1) ops.push(call(`/api/alerts/${alerts[i % alerts.length].id}/feedback`, 'engineer', 'POST', { useful: i % 2 === 0, comment: `r${i}` }));
  else if (k === 2 && tl.length) ops.push(call('/api/records/corrections', 'engineer', 'POST', { record_type: 'fault_event', record_id: tl[i % tl.length].ref, field: 'message', new_value: `RACE ${i}`, reason: 'concurrency test correction' }));
  else ops.push(call('/api/overview', 'engineer'));
}
const t0 = performance.now();
const res = await Promise.all(ops);
const ms = performance.now() - t0;
const errors5xx = res.filter((r) => r.status >= 500).length;
check(errors5xx === 0, `200 mixed concurrent requests in ${ms.toFixed(0)} ms, ${errors5xx} server errors`);
await new Promise((r) => setTimeout(r, 1500));
const v = (await call('/api/audit/verify', 'safety_manager')).json;
check(v.intact === true, `audit chain intact after concurrent writes (${v.entries} entries)`);
// 3. Correction versions stay strictly sequential per record despite concurrency.
// Only the records that were actually corrected above (i % 5 === 2), so the check cannot pass empty.
const corrected = [...new Set(Array.from({ length: 200 }, (_, i) => i).filter((i) => i % 5 === 2).map((i) => tl[i % tl.length]?.ref))].filter(Boolean);
check(corrected.length > 0, `${corrected.length} records received concurrent corrections`);
for (const ref of corrected) {
  const hist = (await call(`/api/records/fault_event/${ref}/history`, 'engineer')).json;
  const vs = hist.corrections.map((c) => c.version);
  const seq = vs.length > 0 && vs.every((x, i) => x === i + 1);
  check(seq, `record ${ref}: correction versions 1..${vs.length} sequential, no duplicates`);
}
console.log(failed ? `\n${failed} FAILED` : '\nall concurrency checks passed');
process.exit(failed ? 1 : 0);
