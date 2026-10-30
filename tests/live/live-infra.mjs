// Live-infrastructure tests: aips-core + Temporal worker against a REAL Kafka-protocol broker and a
// REAL Temporal server (no mocks). Process control is injected through environment variables so
// the same test runs locally or in CI:
//   CORE_STOP, CORE_START, WORKER_STOP, WORKER_START   shell commands
//   KAFKA_BROKERS (default localhost:9092), TEMPORAL_ADDRESS (default localhost:7233)
// Usage: NODE_PATH=workers/temporal/node_modules node tests/live/live-infra.mjs
import { createRequire } from 'node:module';
import { execSync } from 'node:child_process';
import fs from 'node:fs';
const req = createRequire(new URL('../../workers/temporal/package.json', import.meta.url));
const { Kafka, logLevel } = req('kafkajs');
const { Client, Connection } = req('@temporalio/client');

const CORE = process.env.CORE ?? 'http://localhost:8090';
const H = (role = 'engineer') => ({ 'x-aips-role': role, 'x-aips-user': `live.${role}`, 'content-type': 'application/json' });
const get = async (p, role) => (await fetch(CORE + p, { headers: H(role) })).json();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
async function test(name, fn) {
  const t0 = Date.now();
  try { await fn(); results.push({ name, ok: true, ms: Date.now() - t0 }); console.log(`  PASS  ${name}`); }
  catch (e) { results.push({ name, ok: false, error: e.message }); console.log(`  FAIL  ${name}: ${e.message}`); }
}
const ok = (c, m) => { if (!c) throw new Error(m); };
async function waitFor(fn, ms = 30000, step = 500) {
  const end = Date.now() + ms;
  while (Date.now() < end) { const v = await fn().catch(() => null); if (v) return v; await sleep(step); }
  throw new Error('timed out');
}
const metric = async (name) => {
  const t = await (await fetch(`${CORE}/metrics`)).text();
  return Number((t.match(new RegExp(`^${name} (\\S+)`, 'm')) ?? [])[1] ?? NaN);
};
const sh = (cmd) => execSync(cmd, { stdio: 'inherit', shell: '/bin/bash' });

const kafka = new Kafka({ clientId: 'aips-live-test', brokers: (process.env.KAFKA_BROKERS ?? 'localhost:9092').split(','), logLevel: logLevel.NOTHING });
const producer = kafka.producer();
await producer.connect();
const send = (value, headers = { 'x-aips-schema': 'aips.fault.v1' }, key = 'k') =>
  producer.send({ topic: 'aips.ingest', messages: [{ key, value, headers }] });
const temporal = new Client({ connection: await Connection.connect({ address: process.env.TEMPORAL_ADDRESS ?? 'localhost:7233' }), namespace: 'aips' });
async function workflows(prefix) {
  const out = [];
  for await (const w of temporal.workflow.list({ query: `WorkflowId STARTS_WITH "${prefix}"` })) out.push(w);
  return out;
}

console.log('AIPS live infrastructure tests (Kafka protocol broker + Temporal server)');
const health = await get('/api/health');
ok(health.kafka && health.workflow === 'temporal' && health.postgres, 'core must run with Kafka, Temporal mode and PostgreSQL');
const fleet = await get('/api/fleet');
const tail = fleet.find((a) => a.status === 'nominal')?.tail ?? fleet[0].tail;
const sim = health.sim_now;
const ev = (id, extra = {}) => JSON.stringify({ id, tail, flight_id: null, ts: sim, ata: 36, system: 'other', code: '36-11-99',
  component_id: null, severity: 'advisory', message: 'LIVE KAFKA TEST BLEED', source: 'acars', ingested_at: sim, ...extra });
const run = Date.now();

await test('INT-01 Kafka ingest: a valid event reaches analysis and PostgreSQL', async () => {
  await send(ev(`LIVE-${run}-1`));
  await waitFor(async () => (await get(`/api/aircraft/${tail}/timeline`)).some((i) => i.ref === `LIVE-${run}-1`));
});
await test('INT-01 duplicate delivery is ignored (idempotent by event id)', async () => {
  const d0 = await metric('aips_ingest_duplicates_total');
  await send(ev(`LIVE-${run}-1`));
  await waitFor(async () => (await metric('aips_ingest_duplicates_total')) === d0 + 1);
  const n = (await get(`/api/aircraft/${tail}/timeline`)).filter((i) => i.ref === `LIVE-${run}-1`).length;
  ok(n === 1, `event appears once (got ${n})`);
});
await test('DAT-06 malformed, wrong-schema and unknown-tail messages are rejected and counted', async () => {
  const r0 = await metric('aips_ingest_rejected_total');
  await send('{"not": "a fault event"');
  await send(ev(`LIVE-${run}-2`), { 'x-aips-schema': 'aips.fault.v0' });
  await send(ev(`LIVE-${run}-3`, { tail: 'ZZ-NOPE' }));
  await waitFor(async () => (await metric('aips_ingest_rejected_total')) === r0 + 3);
  const tl = await get(`/api/aircraft/${tail}/timeline`);
  ok(!tl.some((i) => i.ref === `LIVE-${run}-2`), 'wrong-schema event not ingested');
  // The consumer is still alive after poison messages.
  await send(ev(`LIVE-${run}-4`));
  await waitFor(async () => (await get(`/api/aircraft/${tail}/timeline`)).some((i) => i.ref === `LIVE-${run}-4`));
});
await test('WRK-05 Temporal: one durable SLA workflow per open case (started from Kafka aips.cases)', async () => {
  const cases = (await get('/api/cases')).filter((c) => c.status !== 'closed');
  ok(cases.length > 0, 'cases exist');
  const wfs = await waitFor(async () => { const w = await workflows('case-sla-'); return w.length >= cases.length ? w : null; }, 60000, 1500);
  const ids = new Set(wfs.map((w) => w.workflowId));
  const missing = cases.filter((c) => !ids.has(`case-sla-${c.id}`));
  ok(missing.length === 0, `missing workflows for ${missing.map((c) => c.id).join(', ')}`);
});
await test('PRV-04 Temporal: pre-departure concerns start workflows; past-due ones escalate via aips-core', async () => {
  const wfs = await waitFor(async () => { const w = await workflows('preflight-'); return w.length ? w : null; }, 60000, 1500);
  const done = await waitFor(async () => {
    for (const w of await workflows('preflight-')) {
      if (w.status.name !== 'COMPLETED') continue;
      const r = await temporal.workflow.getHandle(w.workflowId).result();
      if (String(r).includes('escalation point reached')) return { w, r };
    }
    return null;
  }, 90000, 2000);
  const fid = done.w.workflowId.replace(/^preflight-/, '');
  const audit = (await get('/api/audit?limit=500', 'safety_manager')).entries;
  ok(audit.some((e) => e.action === 'preflight.escalation_requested' && e.subject === fid && e.actor === 'aips-workflow')
     || String(done.r).includes('not escalated'), 'aips-core recorded the worker request (or declined it with a reason)');
  console.log(`        ${wfs.length} preflight workflows; ${done.w.workflowId}: ${done.r}`);
});
await test('Reliability: worker stops gracefully, misses nothing while down, and catches up on restart', async () => {
  if (!process.env.WORKER_STOP || !process.env.WORKER_START) throw new Error('WORKER_STOP/WORKER_START not set');
  sh(process.env.WORKER_STOP);
  await sleep(1500);
  // Open a new case while the worker is down: three repeats of a new code on a quiet aircraft.
  const quiet = fleet.find((a) => a.hypotheses === 0 && a.tail !== tail) ?? fleet[5];
  const code = `29-${String(run).slice(-2)}-77`;
  for (let i = 0; i < 3; i++) {
    const ts = new Date(Date.parse(sim) - (3 - i) * 60_000).toISOString();
    await send(JSON.stringify({ id: `LIVE-${run}-R${i}`, tail: quiet.tail, flight_id: null, ts, ata: 29, system: 'other', code,
      component_id: null, severity: 'caution', message: 'HYD LIVE TEST', source: 'tech_log', ingested_at: ts }));
  }
  const kase = await waitFor(async () => (await get('/api/cases')).find((c) => c.tail === quiet.tail && c.status !== 'closed'), 60000, 1000);
  sh(process.env.WORKER_START);
  await waitFor(async () => (await workflows(`case-sla-${kase.id}`)).length === 1, 60000, 1500);
});
await test('Reliability: core restart resumes the Kafka consumer from committed offsets (no loss)', async () => {
  if (!process.env.CORE_STOP || !process.env.CORE_START) throw new Error('CORE_STOP/CORE_START not set');
  sh(process.env.CORE_STOP);
  await send(ev(`LIVE-${run}-OFFLINE`));
  sh(process.env.CORE_START);
  await waitFor(async () => (await get(`/api/aircraft/${tail}/timeline`)).some((i) => i.ref === `LIVE-${run}-OFFLINE`), 90000, 1000);
});

await producer.disconnect();
const passed = results.filter((r) => r.ok).length;
console.log(`\n${passed}/${results.length} passed`);
fs.mkdirSync('.data', { recursive: true });
fs.writeFileSync('.data/live-infra-results.json', JSON.stringify(results, null, 2));
process.exit(passed === results.length ? 0 : 1);
