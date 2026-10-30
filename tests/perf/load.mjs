// Load, spike and soak test for aips-core. Measures p50/p95/p99 latency, throughput, errors,
// and the server's CPU and resident memory (Linux /proc). Usage:
//   node tests/perf/load.mjs <label> <concurrency> <seconds> [mode=read|ingest] [pid]
import fs from 'node:fs';
const [label = 'load', conc = '50', secs = '20', mode = 'read', pid] = process.argv.slice(2);
const CORE = process.env.CORE ?? 'http://localhost:8090';
const H = { 'x-aips-role': 'engineer', 'x-aips-user': 'perf' };
const fleet = await (await fetch(`${CORE}/api/fleet`, { headers: H })).json();
const tails = fleet.map((a) => a.tail);
const reads = ['/api/overview', '/api/fleet', '/api/alerts', '/api/preflight', '/api/cases', '/api/memory/hypotheses'];
const lat = []; let errors = 0; let n = 0; let seq = 0;

function procStat() {
  if (!pid) return null;
  const st = fs.readFileSync(`/proc/${pid}/stat`, 'utf8').split(') ')[1].split(' ');
  const rss = Number(fs.readFileSync(`/proc/${pid}/status`, 'utf8').match(/VmRSS:\s+(\d+)/)[1]) / 1024;
  return { cpuTicks: Number(st[11]) + Number(st[12]), rssMb: rss };
}
const tick = 100; // USER_HZ on Linux
const before = procStat(); const t0 = performance.now();
let peakRss = before?.rssMb ?? 0;
const sampler = pid ? setInterval(() => { peakRss = Math.max(peakRss, procStat().rssMb); }, 500) : null;
const sim = (await (await fetch(`${CORE}/api/health`)).json()).sim_now;

async function worker() {
  const end = t0 + Number(secs) * 1000;
  while (performance.now() < end) {
    let path, init = { headers: H };
    if (mode === 'ingest') {
      const tail = tails[seq % tails.length]; const i = seq++;
      const ts = new Date(Date.parse(sim) - 60_000).toISOString();
      path = '/api/ingest/events';
      init = { method: 'POST', headers: { ...H, 'content-type': 'application/json' }, body: JSON.stringify({
        id: `PERF-${Date.now()}-${i}`, tail, flight_id: null, ts, ata: 36, system: 'other', code: `36-${String(i % 50).padStart(2, '0')}-77`,
        component_id: null, severity: 'info', message: 'PERF LOAD', source: 'acars', ingested_at: ts }) };
    } else {
      const r = Math.random();
      path = r < 0.25 ? `/api/aircraft/${tails[Math.floor(Math.random() * tails.length)]}` : reads[Math.floor(Math.random() * reads.length)];
    }
    const s = performance.now();
    try {
      const res = await fetch(CORE + path, init);
      await res.arrayBuffer();
      if (!res.ok) errors++;
    } catch { errors++; }
    lat.push(performance.now() - s); n++;
  }
}
await Promise.all(Array.from({ length: Number(conc) }, worker));
const elapsed = (performance.now() - t0) / 1000;
if (sampler) clearInterval(sampler);
const after = procStat();
lat.sort((a, b) => a - b);
const q = (p) => lat[Math.min(lat.length - 1, Math.floor(p * lat.length))].toFixed(1);
const out = {
  label, mode, concurrency: Number(conc), seconds: +elapsed.toFixed(1), requests: n, throughput_rps: +(n / elapsed).toFixed(1),
  p50_ms: +q(0.5), p95_ms: +q(0.95), p99_ms: +q(0.99), max_ms: +lat[lat.length - 1].toFixed(1), error_rate: +(errors / Math.max(n, 1)).toFixed(4),
  server_cpu_pct: before && after ? +(((after.cpuTicks - before.cpuTicks) / tick / elapsed) * 100).toFixed(1) : null,
  server_rss_mb_start: before ? +before.rssMb.toFixed(1) : null, server_rss_mb_peak: pid ? +peakRss.toFixed(1) : null,
};
console.log(JSON.stringify(out));
fs.mkdirSync('.data/perf', { recursive: true });
fs.appendFileSync('.data/perf/results.jsonl', JSON.stringify(out) + '\n');
