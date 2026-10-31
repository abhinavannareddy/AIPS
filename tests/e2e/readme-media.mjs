// Captures the README walkthrough and feature images from a running dev-mode stack.
// Usage: node tests/e2e/readme-media.mjs [coreUrl] [outDir]
// Needs aips-core (AIPS_AUTH_MODE=dev) and aips-emergency for the Guardian shots.
import { createRequire } from 'node:module';
const { chromium } = createRequire(import.meta.url)('playwright');
import fs from 'node:fs';

const base = process.argv[2] ?? 'http://localhost:8090';
const out = process.argv[3] ?? 'docs/media';
const TAIL = 'SE-RBA'; // the Replay Lab scenario with a repeat defect after two repairs
fs.mkdirSync(out, { recursive: true });
const browser = await chromium.launch();

async function session(role, viewport = { width: 1440, height: 900 }) {
  const ctx = await browser.newContext({ viewport, deviceScaleFactor: 1.5 });
  await ctx.addInitScript((r) => localStorage.setItem('aips.devRole', r), role);
  return { ctx, page: await ctx.newPage() };
}

async function go(page, hash, expectText) {
  await page.goto(`${base}/#/${hash}`);
  await page.getByText(expectText, { exact: false }).first().waitFor({ timeout: 10000 });
  await page.waitForTimeout(700);
}

const shot = (page, name, opts = {}) => page.screenshot({ path: `${out}/${name}.png`, ...opts });
const main = (page, name) => page.locator('main').screenshot({ path: `${out}/${name}.png` });

// --- The walkthrough: one aircraft, one repeat defect, end to end ---
const { ctx, page } = await session('engineer');
await go(page, 'overview', 'Fleet status');
await shot(page, 'step1-fleet');
await go(page, `aircraft/${TAIL}`, 'Remaining useful life');
await shot(page, 'step2-aircraft');
await go(page, 'memory', 'Cross-Flight Memory');
await page.locator('.hyp-list button', { hasText: 'Repeat defect after repair' }).first().click();
await page.getByText('Why these events were linked').waitFor({ timeout: 10000 });
await page.waitForTimeout(800);
await main(page, 'step3-memory');
await go(page, 'cases', 'Investigations');
await page.locator('tbody tr.clickable', { hasText: TAIL }).first().click();
await page.getByText('Linked evidence').waitFor({ timeout: 10000 });
await shot(page, 'step4-case');
await go(page, 'preflight', 'Pre-departure board');
await page.locator('tbody tr.clickable', { hasText: TAIL }).first().click();
await page.getByText('Findings, highest priority first').waitFor({ timeout: 10000 });
await main(page, 'step5-preflight');
await page.getByRole('button', { name: 'Record that I reviewed these findings' }).click();
await page.waitForTimeout(1200);

// --- Feature close-ups ---
await go(page, `aircraft/${TAIL}`, 'Remaining useful life');
await page.locator('.param').first().click();
await page.waitForTimeout(1000);
await shot(page, 'feature-parameter-trend', { fullPage: true });
await page.getByRole('tab', { name: 'Safety record' }).click();
await page.waitForTimeout(800);
await shot(page, 'feature-passport');
await go(page, 'flight', 'Flight intelligence');
await page.locator('tbody tr.clickable').first().click();
await page.getByText('Post-flight summary').waitFor({ timeout: 10000 });
await page.waitForTimeout(600);
await shot(page, 'feature-flight');
await go(page, 'reliability', 'Fleet-level patterns');
await shot(page, 'feature-reliability');
await go(page, 'metrics', 'Detection quality');
await shot(page, 'feature-metrics');
await go(page, 'replay', 'Replay Lab');
await shot(page, 'feature-replay');
await go(page, 'alerts', 'Alerts');
await shot(page, 'feature-alerts');
await ctx.close();

const sm = await session('safety_manager');
await go(sm.page, 'audit', 'Audit trail');
await shot(sm.page, 'step6-audit');
await go(sm.page, 'guardian', 'Guardian emergency drills');
await sm.page.getByLabel('Aircraft').selectOption({ index: 1 });
await sm.page.getByRole('button', { name: 'Start training drill' }).click();
await sm.page.getByText('Training drill').first().waitFor({ timeout: 10000 });
await sm.page.waitForTimeout(2500);
await shot(sm.page, 'feature-guardian-notifying');
await sm.page.waitForTimeout(30000); // default escalation fires after 30 s without acknowledgement
await shot(sm.page, 'feature-guardian-escalated');
await sm.ctx.close();

const ls = await session('lessor');
await go(ls.page, 'fleet', 'Aircraft');
await shot(ls.page, 'feature-lessor-isolation');
await ls.ctx.close();

await browser.close();
console.log(`README media written to ${out}`);
