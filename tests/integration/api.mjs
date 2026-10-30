// AIPS API integration tests against running services (dev auth mode).
// Usage: node tests/integration/api.mjs [coreUrl] [emergencyUrl] [phase]
//   phase "main" (default) runs the full suite; phase "after-restart" checks persistence.
import fs from 'node:fs';

const CORE = process.argv[2] ?? 'http://localhost:8090';
const EMG = process.argv[3] ?? 'http://localhost:8091';
const PHASE = process.argv[4] ?? 'main';
const STATE_FILE = '.data/it-state.json';
const results = [];

async function call(base, path, { role = 'engineer', method = 'GET', body, org, raw = false, headers = {} } = {}) {
  const h = { ...headers };
  if (role) { h['x-aips-role'] = h['x-aips-role'] ?? role; h['x-aips-user'] = h['x-aips-user'] ?? `it.${role}`; }
  if (org) h['x-aips-org'] = org;
  if (body !== undefined) h['content-type'] = 'application/json';
  const r = await fetch(base + path, { method, headers: h, body: body === undefined ? undefined : JSON.stringify(body) });
  const text = await r.text();
  let json = null;
  try { json = JSON.parse(text); } catch { /* text body */ }
  return raw ? { status: r.status, text } : { status: r.status, json, text };
}
const core = (p, o) => call(CORE, p, o);
const emg = (p, o) => call(EMG, p, o);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// Collect server-sent-event messages from /api/stream until stop() is called.
function watchStream(headers) {
  const ctrl = new AbortController();
  const msgs = [];
  (async () => {
    try {
      const r = await fetch(`${CORE}/api/stream`, { headers, signal: ctrl.signal });
      const dec = new TextDecoder();
      let buf = '';
      for await (const chunk of r.body) {
        buf += dec.decode(chunk, { stream: true });
        let i;
        while ((i = buf.indexOf('\n\n')) >= 0) {
          const data = buf.slice(0, i).split('\n').filter((l) => l.startsWith('data:')).map((l) => l.slice(5).trim()).join('\n');
          buf = buf.slice(i + 2);
          if (data) { try { msgs.push(JSON.parse(data)); } catch { /* keep-alive */ } }
        }
      }
    } catch { /* aborted by stop() */ }
  })();
  return { msgs, stop: () => ctrl.abort() };
}

async function test(module, name, fn) {
  const t0 = performance.now();
  try {
    await fn();
    results.push({ module, name, ok: true, ms: +(performance.now() - t0).toFixed(1) });
    console.log(`  PASS  [${module}] ${name}`);
  } catch (e) {
    results.push({ module, name, ok: false, error: e.message });
    console.log(`  FAIL  [${module}] ${name}: ${e.message}`);
  }
}
function eq(a, b, msg) { if (a !== b) throw new Error(`${msg}: expected ${JSON.stringify(b)}, got ${JSON.stringify(a)}`); }
function ok(c, msg) { if (!c) throw new Error(msg); }

async function waitFor(fn, ms = 15000) {
  const end = Date.now() + ms;
  for (;;) {
    const v = await fn();
    if (v) return v;
    if (Date.now() > end) throw new Error('timed out waiting for condition');
    await sleep(500);
  }
}

if (PHASE === 'main') {
  console.log('AIPS integration tests (main)');
  // Go-live is the simulated time at core start-up, so the first value seen is at or after it.
  const SIM_AT_START = Date.parse((await core('/api/health', { role: null })).json.sim_now);
  const fleet = (await core('/api/fleet')).json;

  await test('Platform', 'health, readiness, liveness and Prometheus metrics', async () => {
    const h = await core('/api/health', { role: null });
    eq(h.status, 200, 'health'); eq(h.json.onnxruntime, true, 'ONNX Runtime linked'); eq(h.json.rul_model_loaded, true, 'RUL model loaded');
    eq((await core('/readyz', { role: null, raw: true })).status, 200, 'readyz');
    eq((await core('/livez', { role: null, raw: true })).status, 200, 'livez');
    const m = await core('/metrics', { role: null, raw: true });
    ok(m.text.includes('aips_recompute_milliseconds') && m.text.includes('# TYPE aips_alerts gauge'), 'prometheus exposition');
  });

  await test('Sentinel', 'missing credentials are rejected (401)', async () => {
    eq((await core('/api/fleet', { role: null })).status, 401, 'no role');
    eq((await core('/api/fleet', { role: 'superuser' })).status, 401, 'unknown role');
    eq((await core('/api/fleet', { role: null, headers: { authorization: 'Bearer forged.token.value' } })).status, 401, 'forged bearer in dev mode');
  });

  await test('Sentinel', 'role-based access is enforced (403)', async () => {
    eq((await core('/api/fleet', { role: 'cabin_crew' })).status, 403, 'cabin crew cannot read fleet');
    eq((await core('/api/audit', { role: 'flight_ops' })).status, 403, 'ops cannot read audit');
    eq((await core('/api/replay/regenerate', { role: 'engineer', method: 'POST', body: { seed: 1 } })).status, 403, 'engineer cannot regenerate');
    const h = (await core('/api/memory/hypotheses')).json[0].hypothesis;
    eq((await core(`/api/memory/hypotheses/${h.id}/grade`, { role: 'flight_ops', method: 'POST', body: { grade: 'rejected' } })).status, 403, 'ops cannot grade');
    eq((await core(`/api/memory/hypotheses/${h.id}/grade`, { role: 'mcc_controller', method: 'POST', body: { grade: 'rejected' } })).status, 403, 'mcc cannot grade');
  });

  await test('Sentinel', 'tenant isolation: a lessor sees only its own aircraft', async () => {
    const own = fleet.filter((a) => a.lessor === 'Aurora Aviation Capital');
    const other = fleet.find((a) => a.lessor !== 'Aurora Aviation Capital');
    const r = await core('/api/fleet', { role: 'lessor', org: 'Aurora Aviation Capital' });
    eq(r.status, 200, 'lessor fleet');
    eq(r.json.length, own.length, 'only own aircraft');
    eq((await core(`/api/aircraft/${other.tail}`, { role: 'lessor', org: 'Aurora Aviation Capital' })).status, 403, 'foreign aircraft forbidden');
    eq((await core('/api/fleet', { role: 'lessor' })).json.length, 0, 'lessor without org sees nothing');
    const exp = await core(`/api/aircraft/${own[0].tail}/passport/export`, { role: 'lessor', org: 'Aurora Aviation Capital' });
    eq(exp.status, 200, 'export own'); ok(String(exp.json.flights).includes('redacted'), 'operational fields redacted for lessor');
  });

  await test('Sentinel', 'injection and traversal inputs are handled safely', async () => {
    for (const p of ["/api/aircraft/'%20OR%201=1--", '/api/aircraft/..%2F..%2Fetc%2Fpasswd', "/api/cases/x';DROP%20TABLE%20aircraft;--", '/api/preflight/%3Cscript%3E']) {
      const r = await core(p);
      ok([403, 404].includes(r.status), `${p} returned ${r.status}`);
    }
    const s = await core('/api/aircraft/SE-RAA/series?param=../../etc');
    ok(s.status === 400 || s.status === 404 || s.status === 422, `bad param enum rejected (${s.status})`);
    const big = await core('/api/ingest/events', { method: 'POST', body: { junk: 'x'.repeat(3_000_000) } });
    eq(big.status, 413, 'oversized body rejected');
    const bad = await core('/api/ingest/events', { method: 'POST', body: { tail: 'SE-RAA' } });
    ok(bad.status === 422 || bad.status === 400, `malformed event rejected (${bad.status})`);
  });

  await test('Sentinel', 'static UI cannot escape its directory', async () => {
    const r = await core('/..%2F..%2F..%2Fetc%2Fpasswd', { role: null, raw: true });
    ok(!r.text.includes('root:x:0:0'), 'no /etc/passwd disclosure');
  });

  // ---- End-to-end journey: ingest -> anomaly -> hypothesis -> case -> corrective action -> passport ----
  const quiet = fleet.find((a) => a.hypotheses === 0 && a.status === 'nominal');
  // PSP-03 regression: the live stream must not carry other operators' aircraft to a lessor.
  const LESSOR_ORG = 'Aurora Aviation Capital';
  const ownTails = new Set(fleet.filter((a) => a.lessor === LESSOR_ORG).map((a) => a.tail));
  const engStream = watchStream({ 'x-aips-role': 'engineer', 'x-aips-user': 'it.engineer' });
  const lessorStream = watchStream({ 'x-aips-role': 'lessor', 'x-aips-user': 'it.lessor', 'x-aips-org': LESSOR_ORG });
  const code = '29-99-42';
  let caseId = null;
  await test('Integrity/Predict', 'ingested repeat faults become a graded recurrence hypothesis', async () => {
    // Events are backdated up to 3 simulated minutes. Wait until that is still after go-live,
    // or the case counts as imported backlog and time-to-case is not measured (flaky when the
    // suite starts right after the core becomes ready, as in CI).
    const sim = await waitFor(async () => {
      const s = (await core('/api/health', { role: null })).json.sim_now;
      return Date.parse(s) - SIM_AT_START >= 4 * 60_000 ? s : null;
    }, 60000);
    for (let i = 0; i < 3; i++) {
      const ts = new Date(Date.parse(sim) - (3 - i) * 60_000).toISOString();
      const r = await core('/api/ingest/events', { method: 'POST', body: {
        id: `IT-${Date.now()}-${i}`, tail: quiet.tail, flight_id: null, ts, ata: 29, system: 'other', code,
        component_id: null, severity: 'caution', message: 'HYD G SYS LO PR', source: 'tech_log', ingested_at: ts } });
      eq(r.status, 200, 'ingest accepted');
    }
    const h = await waitFor(async () => (await core(`/api/memory/hypotheses?tail=${quiet.tail}`)).json.find((b) => b.hypothesis.codes.includes(code)));
    eq(h.hypothesis.kind, 'recurrence', 'kind');
    ok(['suspected', 'corroborated'].includes(h.hypothesis.grade), 'machine grade is never engineering-confirmed');
    eq(h.events.length, 3, 'three linked events');
    ok(h.hypothesis.explanation.matching_fields.includes('fault_code'), 'explanation lists matched fields');
  });

  await test('Workflow', 'a case opens automatically and time-to-case is measured', async () => {
    const c = await waitFor(async () => (await core('/api/cases')).json.find((x) => x.tail === quiet.tail && x.kind === 'recurring_defect'));
    caseId = c.id;
    eq(c.status, 'open', 'new case open');
    const m = (await core('/api/metrics')).json.metrics.kpis.find((k) => k.key === 'time_to_case');
    ok(m.value !== null && m.value < 15, `time to case measured (${m.value} min)`);
  });

  await test('Sentinel', 'live stream scopes new alerts to aircraft the lessor owns', async () => {
    // Non-vacuous: wait until a foreign-aircraft alert was really broadcast (engineer sees it).
    try { await waitFor(async () => engStream.msgs.some((m) => (m.new_alerts ?? []).some((a) => !ownTails.has(a.tail)))); }
    finally { engStream.stop(); lessorStream.stop(); }
    ok(lessorStream.msgs.some((m) => m.type === 'snapshot'), 'lessor still receives snapshot messages');
    const leaked = lessorStream.msgs.flatMap((m) => m.new_alerts ?? []).filter((a) => !ownTails.has(a.tail));
    eq(leaked.length, 0, `foreign-aircraft alerts on the lessor stream (${leaked.map((a) => a.tail).join(', ')})`);
  });

  await test('Workflow', 'assignment, human override and closure reasons', async () => {
    eq((await core(`/api/cases/${caseId}/assign`, { role: 'mcc_controller', method: 'POST', body: { assignee: 'K. Lindgren', team: 'Hydraulics & Landing Gear' } })).status, 200, 'assign');
    eq((await core(`/api/cases/${caseId}/status`, { method: 'POST', body: { status: 'closed', note: 'short' } })).status, 400, 'closure without corrective action refused');
    eq((await core(`/api/cases/${caseId}/status`, { method: 'POST', body: { status: 'closed', note: 'Replaced green EDP pressure switch; leak check satisfactory' } })).status, 200, 'close');
    const c = (await core(`/api/cases/${caseId}`)).json.case;
    eq(c.status, 'closed', 'closed'); eq(c.assignee, 'K. Lindgren', 'assignee kept');
    ok(c.history.some((h) => h.action === 'assigned') && c.history.some((h) => h.action === 'closed'), 'history recorded');
  });

  await test('Passport', 'safety record shows the journey and exports in the documented schema', async () => {
    const tl = (await core(`/api/aircraft/${quiet.tail}/timeline`)).json;
    ok(tl.some((i) => i.kind === 'fault' && i.title.includes(code)), 'faults in record');
    ok(tl.some((i) => i.kind === 'case'), 'case in record');
    const x = (await core(`/api/aircraft/${quiet.tail}/passport/export`)).json;
    eq(x.schema, 'aips.passport/1.0', 'schema'); ok(x.fault_events.some((e) => e.code === code), 'events exported');
  });

  let gradedId = null;
  await test('Cross-Flight Memory', 'only engineers grade; confirmation needs a justification', async () => {
    const h = (await core(`/api/memory/hypotheses?tail=${quiet.tail}`)).json.find((b) => b.hypothesis.codes.includes(code)).hypothesis;
    gradedId = h.id;
    eq((await core(`/api/memory/hypotheses/${h.id}/grade`, { method: 'POST', body: { grade: 'engineering_confirmed', note: '' } })).status, 400, 'no justification refused');
    eq((await core(`/api/memory/hypotheses/${h.id}/grade`, { method: 'POST', body: { grade: 'engineering_confirmed', note: 'Pressure switch found contaminated at removal' } })).status, 200, 'confirm');
    const g = (await core(`/api/memory/hypotheses?tail=${quiet.tail}`)).json.find((b) => b.hypothesis.id === h.id).hypothesis;
    eq(g.grade, 'engineering_confirmed', 'effective grade'); ok(g.machine_grade !== 'engineering_confirmed', 'machine grade unchanged');
    ok(g.graded_by, 'decision attributed');
  });

  await test('Cross-Flight Memory', 'graph view links hypotheses, events and repairs', async () => {
    const g = (await core(`/api/memory/graph?tail=${quiet.tail}`)).json;
    ok(g.nodes.some((n) => n.kind === 'hypothesis') && g.edges.some((e) => e.kind === 'LINKS'), 'graph has links');
    for (const e of g.edges) ok(g.nodes.some((n) => n.id === e.from) && g.nodes.some((n) => n.id === e.to), 'no dangling edges');
  });

  await test('Integrity', 'alerts carry evidence; feedback is recorded', async () => {
    const al = (await core('/api/alerts')).json;
    ok(al.length > 0, 'alerts present');
    for (const a of al) ok(a.reason && a.model_version && a.sources.length && a.sources.every((s) => typeof s.data_age_seconds === 'number'), `alert ${a.id} lacks evidence`);
    const target = al.find((a) => !a.feedback);
    eq((await core(`/api/alerts/${encodeURIComponent(target.id)}/feedback`, { role: 'mcc_controller', method: 'POST', body: { useful: true, comment: 'IT' } })).status, 200, 'feedback');
    const m = (await core('/api/metrics')).json.metrics;
    ok(m.feedback_count >= 1 && m.feedback_precision !== null, 'feedback counted');
  });

  await test('Prevent', 'stale data yields Insufficient data, never a clear result', async () => {
    const pf = (await core('/api/preflight')).json;
    const stale = fleet.find((a) => a.status === 'insufficient_data');
    const r = pf.find((x) => x.tail === stale.tail);
    eq(r.overall, 'insufficient_data', 'stale aircraft');
    for (const x of pf) ok(['insufficient_data', 'review_required', 'no_findings_in_available_data'].includes(x.overall), 'closed vocabulary');
    const d = (await core(`/api/preflight/${encodeURIComponent(r.flight_id)}`)).json;
    ok(d.disclaimer.includes('does not declare airworthiness'), 'disclaimer present');
    ok(!JSON.stringify(pf).toLowerCase().includes('safe to fly'), 'no safe-to-fly language');
  });

  await test('Prevent', 'MEL, documentation and configuration findings are classified', async () => {
    const pf = (await core('/api/preflight')).json;
    const statuses = new Set(pf.flatMap((p) => p.categories.map((c) => `${c.category}:${c.status}`)));
    ok(statuses.has('safety_restrictions:outstanding'), 'overdue MEL outstanding');
    ok(statuses.has('safety_restrictions:requires_approval'), 'open MEL requires approval');
    ok(statuses.has('documentation:missing'), 'unsigned work order missing');
    ok(statuses.has('configuration:inconsistent'), 'serial on two aircraft inconsistent');
  });

  await test('Prevent', 'review acknowledgement is role-gated and audited', async () => {
    const r = (await core('/api/preflight')).json[0];
    eq((await core(`/api/preflight/${encodeURIComponent(r.flight_id)}/acknowledge`, { role: 'safety_manager', method: 'POST' })).status, 403, 'safety manager cannot ack');
    eq((await core(`/api/preflight/${encodeURIComponent(r.flight_id)}/acknowledge`, { role: 'flight_ops', method: 'POST' })).status, 200, 'ops ack');
    eq((await core(`/api/preflight/${encodeURIComponent(r.flight_id)}`)).json.acknowledged_by, 'it.flight_ops', 'ack recorded');
  });

  await test('Predict', 'RUL estimates are ranges, reproducible, and flag the degrading engine', async () => {
    const a = await core('/api/fleet');
    const low = a.json.filter((x) => x.rul_min_p10 !== null && x.rul_min_p10 < 800);
    ok(low.length >= 1, 'degrading engine flagged');
    const d1 = (await core(`/api/aircraft/${low[0].tail}`)).json.rul;
    const d2 = (await core(`/api/aircraft/${low[0].tail}`)).json.rul;
    for (const r of d1) ok(r.p10 <= r.p50 && r.p50 <= r.p90, 'monotone quantiles');
    eq(JSON.stringify(d1.map((r) => r.p50)), JSON.stringify(d2.map((r) => r.p50)), 'deterministic inference');
  });

  await test('Audit', 'hash chain is intact and covers human decisions', async () => {
    const v = (await core('/api/audit/verify', { role: 'safety_manager' })).json;
    eq(v.intact, true, 'intact');
    const a = (await core('/api/audit?limit=500', { role: 'safety_manager' })).json.entries;
    for (const act of ['hypothesis.graded', 'case.assigned', 'case.closed', 'alert.feedback', 'preflight.acknowledged', 'event.ingested']) ok(a.some((e) => e.action === act), `audit has ${act}`);
  });

  // ---- Guardian ----
  const ac = fleet[0];
  await test('Guardian', 'live mode is refused; training is enforced', async () => {
    eq((await emg('/api/emergency/drills', { role: 'safety_manager', method: 'POST', body: { tail: ac.tail, type_code: ac.type_code, scenario: 'cabin_fire', training: false } })).status, 403, 'live refused');
    eq((await emg('/api/emergency/drills', { role: 'cabin_crew', method: 'POST', body: { tail: ac.tail, type_code: ac.type_code, scenario: 'cabin_fire' } })).status, 403, 'crew cannot start drills');
    eq((await emg('/api/emergency/drills', { role: 'safety_manager', method: 'POST', body: { tail: '../x', type_code: 'A', scenario: 'cabin_fire' } })).status, 400, 'invalid tail');
  });

  await test('Guardian', 'a single weak indicator never notifies anyone', async () => {
    const r = (await emg('/api/emergency/drills', { role: 'safety_manager', method: 'POST', body: { tail: ac.tail, type_code: ac.type_code, scenario: 'false_indicator' } })).json;
    eq(r.state, 'monitoring', 'monitoring'); eq(r.qualified, false, 'not qualified'); eq(r.deliveries.length, 0, 'no notifications');
  });

  await test('Guardian', 'dual incapacitation: two channels, unavailable ack does not count, escalation, cancel', async () => {
    const r = (await emg('/api/emergency/drills', { role: 'safety_manager', method: 'POST', body: { tail: ac.tail, type_code: ac.type_code, scenario: 'dual_incapacitation' } })).json;
    eq(r.training, true, 'training'); eq(r.state, 'notifying', 'notifying');
    const chans = new Set(r.deliveries.map((d) => d.channel));
    ok(chans.has('crew_device_push') && chans.has('cabin_interphone'), 'two independent channels');
    eq(r.steps[6].status, 'not_available', 'no certified autoland on type');
    const u = (await emg(`/api/emergency/drills/${r.id}/ack`, { role: 'cabin_crew', method: 'POST', body: { crew_id: 'CC1', available: false } })).json;
    eq(u.state, 'notifying', 'unavailable does not stop escalation');
    eq((await emg(`/api/emergency/drills/${r.id}/ack`, { role: 'cabin_crew', method: 'POST', body: { crew_id: 'CC4', available: true } })).status, 400, 'un-notified crew cannot ack');
    await emg(`/api/emergency/drills/${r.id}/simulate`, { role: 'safety_manager', method: 'POST', body: { action: 'feed_loss' } });
    const e = await waitFor(async () => { const x = (await emg(`/api/emergency/drills/${r.id}`, { role: 'safety_manager' })).json; return x.escalation_level >= 1 ? x : null; }, 30000);
    eq(e.state, 'escalated', 'escalated without confirmed response'); eq(e.feed_lost, true, 'feed loss does not clear');
    ok(e.deliveries.some((d) => d.channel === 'airline_ops_messaging'), 'operations notified');
    eq((await emg(`/api/emergency/drills/${r.id}/cancel`, { role: 'cabin_crew', method: 'POST', body: { reason: '' } })).status, 400, 'cancel needs reason');
    eq((await emg(`/api/emergency/drills/${r.id}/cancel`, { role: 'cabin_crew', method: 'POST', body: { reason: 'Captain confirmed fit and in control' } })).json.state, 'cancelled', 'cancelled');
    eq((await emg(`/api/emergency/drills/${r.id}/verify`, { role: 'safety_manager' })).json.intact, true, 'journal intact');
    fs.writeFileSync('.data/it-drill.txt', r.id);
  });

  await test('Guardian', 'ATC message is logged but never transmitted in training', async () => {
    const r = (await emg('/api/emergency/drills', { role: 'safety_manager', method: 'POST', body: { tail: ac.tail, type_code: ac.type_code, scenario: 'cabin_fire' } })).json;
    const x = await waitFor(async () => { const y = (await emg(`/api/emergency/drills/${r.id}`, { role: 'safety_manager' })).json; return y.escalation_level >= 2 ? y : null; }, 60000);
    const atc = x.deliveries.find((d) => d.channel === 'atc_datalink');
    eq(atc.status, 'not_transmitted', 'not transmitted');
  });
  await test('Guardian', 'AUT-03 ATC message carries destination weather and alternates; GRD-08 procedure card is a labelled training example', async () => {
    const r = (await emg('/api/emergency/drills', { role: 'safety_manager', method: 'POST', body: { tail: ac.tail, type_code: ac.type_code, flight: 'NA1028', dest: 'ZRH', scenario: 'dual_incapacitation' } })).json;
    ok(r.procedure && r.procedure.training_example && r.procedure.source.startsWith('TRAINING EXAMPLE'), 'procedure card labelled');
    const x = await waitFor(async () => { const y = (await emg(`/api/emergency/drills/${r.id}`, { role: 'safety_manager' })).json; return y.atc_message ? y : null; }, 60000);
    ok(x.atc_message.text.startsWith('TRAINING DRILL'), 'training prefix');
    ok(x.atc_message.destination_metar.startsWith('ZRH'), 'destination METAR');
    eq(x.atc_message.alternates.length, 2, 'alternates');
    ok(x.atc_message.status.includes('NOT transmitted'), 'not transmitted');
    eq(x.deliveries.find((d) => d.channel === 'atc_datalink').status, 'not_transmitted', 'delivery not transmitted');
    const cards = (await emg('/api/emergency/procedures', { role: 'cabin_crew' })).json;
    ok(cards.length >= 4 && cards.every((c) => c.training_example), 'cards endpoint');
    await emg(`/api/emergency/drills/${r.id}/resolve`, { role: 'safety_manager', method: 'POST', body: { reason: 'test complete' } });
  });
  await test('Guardian', 'EMG-04 conflicting indicators are shown and never trigger notifications', async () => {
    const r = (await emg('/api/emergency/drills', { role: 'safety_manager', method: 'POST', body: { tail: ac.tail, type_code: ac.type_code, scenario: 'conflicting_indicators' } })).json;
    ok(r.disagreement && r.disagreement.includes('disagree'), 'disagreement statement');
    eq(r.qualified, false, 'not qualified'); eq(r.deliveries.length, 0, 'nobody notified');
    ok(r.indicators.some((i) => i.contradicts), 'contradicting indicator marked');
    ok(r.journal.entries.some((e) => e.action === 'disagreement.flagged'), 'journaled');
  });

  // ---------------- Gap-closing features (technical audit, phase 2)
  await test('Flight', 'flight events detected from flight data, each with limitation and weather context', async () => {
    const r = await core('/api/flight/events');
    eq(r.status, 200, 'status');
    const kinds = new Set(r.json.map((e) => e.kind));
    for (const k of ['unstable_approach', 'rapid_descent', 'speed_exceedance_below10k', 'track_data_gap'])
      ok(kinds.has(k), `detected ${k} (have ${[...kinds].join(',')})`);
    ok(r.json.every((e) => e.limitation && e.limitation.includes('does not establish intent')), 'limitation on every event');
    const rd = r.json.find((e) => e.kind === 'rapid_descent');
    ok(rd.correlated_events.length >= 1, 'rapid descent correlated with cabin-pressure fault');
    eq(rd.evidence_grade, 'corroborated', 'correlation grades as corroborated, not confirmed');
    eq((await core('/api/flight/events', { role: 'cabin_crew' })).status, 403, 'cabin crew cannot read flight data');
  });
  await test('Flight', 'flight view: track, weather, NOTAMs, data quality, post-flight summary', async () => {
    const ev = (await core('/api/flight/events?kind=unstable_approach')).json[0];
    const v = await core(`/api/flight/${ev.flight_id}`);
    eq(v.status, 200, 'status');
    ok(v.json.track.length > 5, 'track reconstructed');
    ok(v.json.weather.departure.metar.startsWith(v.json.flight.origin), 'METAR context for departure airport');
    ok(Array.isArray(v.json.notams.departure) && Array.isArray(v.json.notams.arrival), 'NOTAMs');
    ok(v.json.post_flight && 'new_defects' in v.json.post_flight, 'post-flight summary');
    eq((await core('/api/flight/does-not-exist')).status, 404, 'unknown flight 404');
  });
  await test('Workflow', 'WRK-03 alert acknowledgement is attributed, persisted and idempotent', async () => {
    const a = (await core('/api/alerts')).json.find((x) => !x.acknowledged);
    eq((await core(`/api/alerts/${a.id}/acknowledge`, { method: 'POST', role: 'flight_ops' })).status, 403, 'ops cannot ack');
    const r = await core(`/api/alerts/${a.id}/acknowledge`, { method: 'POST', role: 'mcc_controller' });
    eq(r.status, 200, 'ack');
    eq((await core(`/api/alerts/${a.id}/acknowledge`, { method: 'POST', role: 'mcc_controller' })).status, 409, 'second ack conflicts');
    await sleep(700);
    const again = (await core('/api/alerts')).json.find((x) => x.id === a.id);
    eq(again.acknowledged.by, 'it.mcc_controller', 'acknowledged_by');
    const audit = (await core('/api/audit?limit=50', { role: 'safety_manager' })).json.entries;
    ok(audit.some((e) => e.action === 'alert.acknowledged' && e.subject === a.id), 'audited');
  });
  await test('Passport', 'PSP-02 corrections are versioned, attributed and never overwrite the original', async () => {
    let evId = null;
    for (const ac of (await core('/api/fleet')).json) {
      evId = (await core(`/api/aircraft/${ac.tail}/timeline`)).json.find((x) => x.kind === 'fault')?.ref;
      if (evId) break;
    }
    ok(evId, 'found a fault event id');
    const bad = await core('/api/records/corrections', { method: 'POST', body: { record_type: 'fault_event', record_id: evId, field: 'ata', new_value: '300', reason: 'typo in ATA chapter' } });
    eq(bad.status, 400, 'invalid ATA rejected');
    eq((await core('/api/records/corrections', { method: 'POST', body: { record_type: 'fault_event', record_id: evId, field: 'message', new_value: 'x', reason: '' } })).status, 400, 'reason required');
    eq((await core('/api/records/corrections', { method: 'POST', role: 'lessor', body: { record_type: 'fault_event', record_id: evId, field: 'message', new_value: 'x', reason: 'lessor edit' } })).status, 403, 'lessor cannot correct');
    const c1 = await core('/api/records/corrections', { method: 'POST', body: { record_type: 'fault_event', record_id: evId, field: 'message', new_value: 'CORRECTED MESSAGE V2', reason: 'transcription error in ACARS gateway' } });
    eq(c1.status, 200, 'correction 1'); eq(c1.json.version, 1, 'v1');
    const c2 = await core('/api/records/corrections', { method: 'POST', body: { record_type: 'fault_event', record_id: evId, field: 'message', new_value: 'CORRECTED MESSAGE V3', reason: 'second review by certifying engineer' } });
    eq(c2.json.version, 2, 'v2'); eq(c2.json.old_value, 'CORRECTED MESSAGE V2', 'old value is the previous correction');
    const h = (await core(`/api/records/fault_event/${evId}/history`)).json;
    eq(h.corrections.length, 2, 'two versions'); eq(h.current_version, 3, 'current version');
    ok(h.original.message !== 'CORRECTED MESSAGE V3', 'original preserved');
    fs.writeFileSync('.data/it-correction.txt', evId);
  });
  await test('Predict', 'PRD-06 maintenance effectiveness by ATA chapter and team', async () => {
    const r = (await core('/api/reliability')).json;
    ok(r.by_ata.length > 3 && r.by_team.length >= 1, 'groups');
    ok(r.by_ata.every((x) => x.effectiveness >= 0 && x.effectiveness <= 1), 'bounded');
    ok(r.limitation.includes('does not assess individual'), 'limitation');
  });
  await test('Command', 'CMD-05 CSV reports with correct headers and formula neutralisation', async () => {
    for (const n of ['alerts', 'cases', 'preflight']) {
      const r = await core(`/api/reports/${n}.csv`, { raw: true });
      eq(r.status, 200, `${n} status`);
      ok(r.text.split('\r\n')[0].includes('tail'), `${n} header`);
      ok(!/\r\n[=+@]/.test(r.text), `${n} no formula-leading cells`);
    }
    eq((await core('/api/reports/secret.csv', { raw: true })).status, 404, 'unknown report');
    eq((await core('/api/reports/alerts.csv', { role: 'lessor', raw: true })).status, 403, 'lessor denied');
  });
  await test('Memory', 'CFM-05 fleet patterns are k-anonymous aggregates without tail numbers', async () => {
    const tails = (await core('/api/fleet')).json.map((a) => a.tail);
    const r = await core('/api/memory/fleet-patterns', { role: 'lessor', org: 'Aurora Aviation Capital' });
    eq(r.status, 200, 'status');
    ok(r.json.patterns.every((p) => p.aircraft >= r.json.k_anonymity), 'k-anonymity');
    ok(!tails.some((t) => r.text.includes(t)), 'no tail disclosed');
  });
  await test('Prevent', 'PRV-04 worker escalation endpoint: role-gated, idempotent, audited', async () => {
    const reps = (await core('/api/preflight', { role: 'flight_ops' })).json;
    const r = reps[0];
    eq((await core(`/api/preflight/${r.flight_id}/escalate`, { method: 'POST' })).status, 403, 'engineer cannot');
    const x = await core(`/api/preflight/${r.flight_id}/escalate`, { method: 'POST', role: 'admin' });
    eq(x.status, 200, 'admin/worker');
    ok(typeof x.json.escalated === 'boolean', 'decision reported');
    const full = (await core(`/api/preflight/${r.flight_id}`, { role: 'flight_ops' })).json;
    ok(full.context && full.context.departure_weather && Array.isArray(full.context.notams), 'weather and NOTAM context in report');
  });
  await test('Predict', 'NFR-10 model registry: hash-verified rollback and re-activation, drift reported', async () => {
    const m = (await core('/api/models/rul')).json;
    ok(m.registry.length >= 2, 'two versions'); eq(m.active, '1.1.0', 'latest active');
    ok(m.drift && Array.isArray(m.drift.features), 'drift');
    eq((await core('/api/models/rul/activate', { method: 'POST', body: { version: '1.0.0', reason: 'rollback test' } })).status, 403, 'engineer cannot');
    eq((await core('/api/models/rul/activate', { method: 'POST', role: 'admin', body: { version: '9.9.9', reason: 'nope nope' } })).status, 404, 'unknown');
    const r = await core('/api/models/rul/activate', { method: 'POST', role: 'admin', body: { version: '1.0.0', reason: 'rollback drill' } });
    eq(r.status, 200, 'rollback'); eq(r.json.previous, '1.1.0', 'previous');
    eq((await core('/api/models/rul')).json.active, '1.0.0', 'rolled back');
    eq((await core('/api/models/rul/activate', { method: 'POST', role: 'admin', body: { version: '1.1.0', reason: 'restore after drill' } })).status, 200, 'restore');
    // Leave 1.0.0 active so the after-restart phase can prove the activation is durable.
    eq((await core('/api/models/rul/activate', { method: 'POST', role: 'admin', body: { version: '1.0.0', reason: 'durability check' } })).status, 200, 'leave rollback active');
  });
  await test('Integrity', 'DAT-06 ingest rejects an unknown schema version; INT-01 redelivery is idempotent', async () => {
    const tail = (await core('/api/fleet')).json[0].tail;
    const ev = { id: 'it-schema-1', tail, flight_id: null, ts: new Date().toISOString(), ata: 21, system: 'other', code: '21-00-01',
      component_id: null, severity: 'advisory', message: 'schema test', source: 'tech_log', ingested_at: new Date().toISOString() };
    eq((await core('/api/ingest/events', { method: 'POST', role: 'admin', body: ev, headers: { 'x-aips-schema': 'aips.fault.v0' } })).status, 415, 'wrong version');
    eq((await core('/api/ingest/events', { method: 'POST', role: 'admin', body: { ...ev, code: '' } })).status, 400, 'missing code');
    const dupId = `it-dup-${Date.now()}`;
    const first = await core('/api/ingest/events', { method: 'POST', role: 'admin', body: { ...ev, id: dupId } });
    eq(first.status, 200, 'first delivery accepted'); eq(first.json.duplicate, false, 'first is new');
    const second = await core('/api/ingest/events', { method: 'POST', role: 'admin', body: { ...ev, id: dupId } });
    eq(second.json.duplicate, true, 'redelivery recognised as duplicate (B7)');
  });
  await test('Data', 'DAT-03 erasure requires two different administrators and removes the aircraft', async () => {
    const fleet = (await core('/api/fleet')).json;
    const victim = fleet[fleet.length - 1].tail;
    const req = await core('/api/admin/erasure-requests', { method: 'POST', role: 'admin', body: { tail: victim, reason: 'aircraft left the operator; contract end' } });
    eq(req.status, 200, 'request');
    eq((await core(`/api/admin/erasure-requests/${req.json.id}/approve`, { method: 'POST', role: 'admin' })).status, 403, 'same admin refused');
    const ap = await core(`/api/admin/erasure-requests/${req.json.id}/approve`, { method: 'POST', role: 'admin', headers: { 'x-aips-user': 'it.admin2' } });
    eq(ap.status, 200, 'second admin');
    ok(ap.json.rows_erased > 0, 'rows erased in PostgreSQL');
    await sleep(700);
    eq((await core(`/api/aircraft/${victim}`)).status, 404, 'aircraft gone');
    const v = (await core('/api/audit/verify', { role: 'safety_manager' })).json;
    eq(v.intact, true, 'audit chain intact after erasure');
    fs.writeFileSync('.data/it-erased.txt', victim);
  });

  fs.writeFileSync(STATE_FILE, JSON.stringify({ caseId, gradedId, tail: quiet.tail }));
}

if (PHASE === 'after-restart') {
  console.log('AIPS integration tests (after restart: persistence and recovery)');
  const st = JSON.parse(fs.readFileSync(STATE_FILE, 'utf8'));
  await test('Reliability', 'human decisions survive a core restart (PostgreSQL)', async () => {
    const h = (await core(`/api/memory/hypotheses?tail=${st.tail}`)).json.find((b) => b.hypothesis.id === st.gradedId);
    ok(h, 'hypothesis rebuilt from persisted events'); eq(h.hypothesis.grade, 'engineering_confirmed', 'grade persisted');
    eq((await core(`/api/cases/${st.caseId}`)).json.case.status, 'closed', 'case state persisted');
  });
  await test('Reliability', 'corrections, acknowledgements and erasure survive restart', async () => {
    const evId = fs.readFileSync('.data/it-correction.txt', 'utf8');
    eq((await core(`/api/records/fault_event/${evId}/history`)).json.corrections.length, 2, 'corrections reloaded');
    ok((await core('/api/alerts')).json.some((a) => a.acknowledged), 'acknowledgement reloaded');
    const victim = fs.readFileSync('.data/it-erased.txt', 'utf8');
    eq((await core(`/api/aircraft/${victim}`)).status, 404, 'erased aircraft stays erased');
  });
  await test('Predict', 'NFR-10 model activation persists across restart', async () => {
    eq((await core('/api/models/rul')).json.active, '1.0.0', 'rolled-back version restored after restart');
    eq((await core('/api/models/rul/activate', { method: 'POST', role: 'admin', body: { version: '1.1.0', reason: 'restore after restart test' } })).status, 200, 'restore latest');
  });
  await test('Reliability', 'audit chain reloads intact after restart', async () => {
    const v = (await core('/api/audit/verify', { role: 'safety_manager' })).json;
    eq(v.intact, true, 'intact'); ok(v.entries > 5, 'entries reloaded');
  });
  await test('Reliability', 'a restart does not re-audit escalations that were already recorded', async () => {
    const a = (await core('/api/audit?limit=2000', { role: 'safety_manager' })).json.entries;
    const esc = a.filter((e) => e.actor === 'aips-workflow' && /^(alert|preflight|case)\.escalated$/.test(e.action));
    ok(esc.length > 0, 'escalation entries present (the check is not vacuous)');
    const keys = esc.map((e) => `${e.action} ${e.subject}`);
    const dup = keys.filter((k, i) => keys.indexOf(k) !== i);
    eq(dup.length, 0, `duplicate escalation entries after restart (${[...new Set(dup)].slice(0, 3).join('; ')})`);
  });
  await test('Reliability', 'emergency journal recovers after a service restart', async () => {
    const id = fs.readFileSync('.data/it-drill.txt', 'utf8');
    const x = (await emg(`/api/emergency/drills/${id}`, { role: 'safety_manager' })).json;
    eq(x.state, 'cancelled', 'state recovered');
    eq((await emg(`/api/emergency/drills/${id}/verify`, { role: 'safety_manager' })).json.intact, true, 'journal intact');
  });
}

const passed = results.filter((r) => r.ok).length;
console.log(`\n${passed}/${results.length} passed`);
fs.mkdirSync('.data', { recursive: true });
fs.writeFileSync(`.data/it-results-${PHASE}.json`, JSON.stringify(results, null, 2));
process.exit(passed === results.length ? 0 : 1);
