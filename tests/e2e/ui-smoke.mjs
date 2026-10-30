// End-to-end UI smoke test: visits every screen as the right role, fails on console errors,
// exercises key actions, and saves screenshots. Usage: node tests/e2e/ui-smoke.mjs [baseUrl] [outDir]
import { createRequire } from 'node:module';
const { chromium } = createRequire(import.meta.url)('playwright');
import fs from 'node:fs';

const base = process.argv[2] ?? 'http://localhost:8090';
const out = process.argv[3] ?? '.data/screens';
fs.mkdirSync(out, { recursive: true });
const browser = await chromium.launch();
const failures = [];

async function session(role) {
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
  await ctx.addInitScript((r) => localStorage.setItem('aips.devRole', r), role);
  const page = await ctx.newPage();
  page.on('console', (m) => { if (m.type() === 'error') failures.push(`[${role}] console: ${m.text()}`); });
  page.on('pageerror', (e) => failures.push(`[${role}] pageerror: ${e.message}`));
  return { ctx, page };
}

async function visit(page, hash, name, expectText) {
  await page.goto(`${base}/#/${hash}`);
  // Not 'networkidle': the app's /api/stream EventSource stays open, so it may never fire.
  await page.waitForLoadState('load');
  try {
    await page.getByText(expectText, { exact: false }).first().waitFor({ timeout: 8000 });
  } catch {
    failures.push(`${name}: expected text "${expectText}" not found`);
  }
  await page.screenshot({ path: `${out}/${name}.png`, fullPage: true });
}

const { ctx, page } = await session('engineer');
await visit(page, 'overview', '01-overview', 'Fleet status');
await visit(page, 'fleet', '02-fleet', 'Sorted by health score');
const tail = await page.locator('.strip-tail strong').first().innerText();
await visit(page, `aircraft/${tail}`, '03-aircraft-health', 'Remaining useful life');
await page.locator('.param').first().click();
await page.waitForTimeout(800);
await page.screenshot({ path: `${out}/04-aircraft-series.png`, fullPage: true });
await page.getByRole('tab', { name: 'Safety record' }).click();
await page.waitForTimeout(600);
await page.screenshot({ path: `${out}/05-aircraft-record.png`, fullPage: true });
await visit(page, 'memory', '06-memory', 'Cross-Flight Memory');
await page.locator('.hyp-list button').first().click();
await page.getByText('Why these events were linked').waitFor({ timeout: 8000 }).catch(() => failures.push('memory detail did not open'));
await page.screenshot({ path: `${out}/07-memory-detail.png`, fullPage: true });
await visit(page, 'alerts', '08-alerts', 'Alerts');
await visit(page, 'preflight', '09-preflight-board', 'Pre-departure board');
await page.locator('tbody tr.clickable').first().click();
await page.getByText('advisory', { exact: false }).first().waitFor({ timeout: 8000 }).catch(() => failures.push('preflight detail missing disclaimer'));
await page.screenshot({ path: `${out}/10-preflight-detail.png`, fullPage: true });
await visit(page, 'cases', '11-cases', 'Investigations');
await page.locator('tbody tr.clickable').first().click();
await page.waitForTimeout(800);
await page.screenshot({ path: `${out}/12-case-detail.png`, fullPage: true });
await visit(page, 'metrics', '13-metrics', 'Detection quality');
await visit(page, 'replay', '14-replay', 'Replay Lab');
await visit(page, 'flight', '21-flight-board', 'Flight intelligence');
await page.locator('tbody tr.clickable').first().click();
await page.getByText('Post-flight summary').waitFor({ timeout: 8000 }).catch(() => failures.push('flight detail did not open'));
await page.waitForTimeout(500);
await page.screenshot({ path: `${out}/22-flight-detail.png`, fullPage: true });
await visit(page, 'reliability', '23-reliability', 'Fleet-level patterns');
await visit(page, 'alerts', '24-alerts-ack', 'Alerts');
const ackBtn = page.getByRole('button', { name: 'Acknowledge' }).first();
if (await ackBtn.count()) {
  await ackBtn.click();
  await page.getByText('Acknowledged by').first().waitFor({ timeout: 8000 }).catch(() => failures.push('alert acknowledge did not register'));
}
await page.screenshot({ path: `${out}/24-alerts-ack.png`, fullPage: false });
await ctx.close();

const ad = await session('admin');
await visit(ad.page, 'admin', '25-admin-erasure', 'Data administration');
await visit(ad.page, 'metrics', '26-model-registry', 'Remaining-life model');
await ad.ctx.close();

const sm = await session('safety_manager');
await visit(sm.page, 'guardian', '15-guardian', 'Guardian emergency drills');
await sm.page.getByLabel('Aircraft').selectOption({ index: 1 });
await sm.page.getByRole('button', { name: 'Start training drill' }).click();
await sm.page.getByText('Training drill').first().waitFor({ timeout: 8000 }).catch(() => failures.push('drill did not start'));
await sm.page.waitForTimeout(2500);
await sm.page.screenshot({ path: `${out}/16-drill-notifying.png`, fullPage: true });
await sm.page.waitForTimeout(24000);
await sm.page.screenshot({ path: `${out}/17-drill-escalated.png`, fullPage: true });
await visit(sm.page, 'audit', '18-audit', 'Audit trail');
await sm.ctx.close();

const ls = await session('lessor');
await visit(ls.page, 'fleet', '19-lessor-fleet', 'Aircraft');
const n = await ls.page.locator('.strip').count();
if (n === 0 || n >= 50) failures.push(`lessor should see only its own aircraft, saw ${n}`);
await ls.ctx.close();

const mob = await browser.newContext({ viewport: { width: 390, height: 844 } });
const mp = await mob.newPage();
await mp.goto(`${base}/#/overview`); await mp.waitForLoadState('load'); await mp.waitForTimeout(800);
await mp.screenshot({ path: `${out}/20-mobile-overview.png`, fullPage: false });
await browser.close();

if (failures.length) { console.error('UI SMOKE FAILED\n' + failures.join('\n')); process.exit(1); }
console.log(`UI SMOKE PASSED: screenshots in ${out}`);
