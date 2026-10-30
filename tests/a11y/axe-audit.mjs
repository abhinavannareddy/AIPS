// NFR-14 accessibility audit: runs axe-core (WCAG 2.0/2.1/2.2 A and AA rules) on every screen.
// Usage: NODE_PATH=$(npm root -g) node tests/a11y/axe-audit.mjs [baseUrl] [reportFile]
import { createRequire } from 'node:module';
import fs from 'node:fs';
const req = createRequire(import.meta.url);
const { chromium } = req('playwright');
const axePath = req.resolve('axe-core/axe.min.js', { paths: [new URL('../../apps/command', import.meta.url).pathname] });
const axeSrc = fs.readFileSync(axePath, 'utf8');
const base = process.argv[2] ?? 'http://localhost:8090';
const report = process.argv[3] ?? '.data/a11y-report.json';

const screens = [
  ['engineer', 'overview'], ['engineer', 'fleet'], ['engineer', 'aircraft/SE-RAA'], ['engineer', 'memory'],
  ['engineer', 'alerts'], ['engineer', 'flight'], ['engineer', 'reliability'], ['engineer', 'preflight'],
  ['engineer', 'cases'], ['engineer', 'metrics'], ['engineer', 'replay'], ['safety_manager', 'guardian'],
  ['safety_manager', 'audit'], ['admin', 'admin'], ['cabin_crew', 'guardian'], ['lessor', 'fleet'],
];
const browser = await chromium.launch();
const results = [];
for (const [role, hash] of screens) {
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
  await ctx.addInitScript((r) => localStorage.setItem('aips.devRole', r), role);
  const page = await ctx.newPage();
  await page.goto(`${base}/#/${hash}`);
  // Not 'networkidle': the app's /api/stream EventSource stays open, so it may never fire.
  await page.waitForLoadState('load');
  await page.waitForTimeout(1200);
  await page.addScriptTag({ content: axeSrc });
  const r = await page.evaluate(async () => {
    // eslint-disable-next-line no-undef
    const res = await axe.run(document, { runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa'] } });
    return res.violations.map((v) => ({ id: v.id, impact: v.impact, help: v.help, nodes: v.nodes.length, sample: v.nodes.slice(0, 3).map((n) => n.target.join(' ')) }));
  });
  results.push({ role, screen: hash, violations: r });
  console.log(`${r.length ? 'FAIL' : 'PASS'}  ${role.padEnd(15)} #/${hash}${r.length ? '  ' + r.map((v) => `${v.id}(${v.nodes})`).join(', ') : ''}`);
  await ctx.close();
}
await browser.close();
fs.mkdirSync('.data', { recursive: true });
fs.writeFileSync(report, JSON.stringify({ engine: 'axe-core', at: new Date().toISOString(), results }, null, 2));
const bad = results.filter((x) => x.violations.length).length;
console.log(`\n${results.length - bad}/${results.length} screens without WCAG A/AA violations`);
process.exit(bad ? 1 : 0);
