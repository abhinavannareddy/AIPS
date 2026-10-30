// Live Keycloak tests: real OIDC tokens from a running Keycloak with the shipped AIPS realm, validated
// by a running aips-core in keycloak mode. Includes a real browser login (authorisation code + PKCE)
// with the first-login password change and TOTP enrolment, and a second login that must ask for TOTP.
// Usage: NODE_PATH=$(npm root -g) node tests/live/keycloak.mjs
import { createRequire } from 'node:module';
import crypto from 'node:crypto';
import fs from 'node:fs';
const { chromium } = createRequire(import.meta.url)('playwright');
const KC = process.env.KC ?? 'http://localhost:8081';
const CORE = process.env.CORE ?? 'http://localhost:8090';
const REALM = `${KC}/realms/aips`;
const results = [];
async function test(name, fn) {
  try { await fn(); results.push({ name, ok: true }); console.log(`  PASS  ${name}`); }
  catch (e) { results.push({ name, ok: false, error: e.message }); console.log(`  FAIL  ${name}: ${e.message}`); }
}
const ok = (c, m) => { if (!c) throw new Error(m); };
const api = async (path, token, extra = {}) => {
  const r = await fetch(CORE + path, { headers: { ...(token ? { authorization: `Bearer ${token}` } : {}), ...extra } });
  return { status: r.status, json: await r.json().catch(() => null) };
};
function totp(secretB32, algo = 'sha256', step = 30, digits = 6, t = Date.now()) {
  const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567';
  const clean = secretB32.replace(/[\s=]/g, '').toUpperCase();
  let bits = ''; for (const c of clean) bits += alphabet.indexOf(c).toString(2).padStart(5, '0');
  const key = Buffer.from(bits.match(/.{8}/g).map((b) => parseInt(b, 2)));
  const ctr = Buffer.alloc(8); ctr.writeBigUInt64BE(BigInt(Math.floor(t / 1000 / step)));
  const h = crypto.createHmac(algo, key).update(ctr).digest();
  const o = h[h.length - 1] & 0xf;
  return String(((h.readUInt32BE(o) & 0x7fffffff) % 10 ** digits)).padStart(digits, '0');
}

console.log('AIPS live Keycloak tests');
const tok = await (await fetch(`${REALM}/protocol/openid-connect/token`, { method: 'POST',
  body: new URLSearchParams({ grant_type: 'client_credentials', client_id: 'aips-workflow', client_secret: process.env.WORKFLOW_SECRET ?? 'aips-dev-workflow-secret' }) })).json();

await test('service identity: client-credentials token is accepted with its realm role only', async () => {
  ok(tok.access_token, `no token: ${JSON.stringify(tok)}`);
  // aips-core refreshes its JWKS on an unknown key id at most once per 60 s (kid-flood protection),
  // so right after a Keycloak restart with new keys the first minute may still return 401.
  let me = await api('/api/me', tok.access_token);
  for (let i = 0; i < 14 && me.status === 401; i++) { await new Promise((r) => setTimeout(r, 5000)); me = await api('/api/me', tok.access_token); }
  ok(me.status === 200 && me.json.auth_mode === 'keycloak', `me ${me.status}`);
  ok(me.json.roles.includes('mcc_controller') && !me.json.roles.includes('admin'), `roles ${me.json.roles}`);
  ok((await api('/api/cases', tok.access_token)).status === 200, 'cases readable');
  ok((await api('/api/admin/erasure-requests', tok.access_token)).status === 403, 'admin endpoint forbidden');
});
await test('missing, forged and dev-header credentials are rejected (401)', async () => {
  ok((await api('/api/me')).status === 401, 'no token');
  ok((await api('/api/me', null, { 'x-aips-role': 'admin', 'x-aips-user': 'mallory' })).status === 401, 'dev headers ignored in keycloak mode');
  const [h, p, s] = tok.access_token.split('.');
  const flipped = s.slice(0, -2) + (s.at(-2) === 'A' ? 'B' : 'A') + s.at(-1);
  ok((await api('/api/me', `${h}.${p}.${flipped}`)).status === 401, 'bad signature');
  const claims = JSON.parse(Buffer.from(p, 'base64url'));
  const forged = Buffer.from(JSON.stringify({ ...claims, realm_access: { roles: ['admin'] } })).toString('base64url');
  ok((await api('/api/me', `${h}.${forged}.${s}`)).status === 401, 'payload tampering (privilege escalation)');
  ok((await api('/api/me', `eyJhbGciOiJub25lIn0.${p}.`)).status === 401, 'alg=none');
});
await test('token from another issuer (Keycloak master realm) is rejected', async () => {
  const m = await (await fetch(`${KC}/realms/master/protocol/openid-connect/token`, { method: 'POST',
    body: new URLSearchParams({ grant_type: 'password', client_id: 'admin-cli', username: 'admin', password: process.env.KC_ADMIN_PASSWORD ?? 'admin-dev-only' }) })).json();
  ok(m.access_token, 'master token obtained');
  ok((await api('/api/me', m.access_token)).status === 401, 'wrong issuer/audience');
});
await test('direct password grant is disabled for the UI client (no password-only login path)', async () => {
  const r = await fetch(`${REALM}/protocol/openid-connect/token`, { method: 'POST',
    body: new URLSearchParams({ grant_type: 'password', client_id: 'aips-command', username: 'demo.engineer', password: 'ChangeMe-Dev-Only-1!' }) });
  ok(r.status === 400 || r.status === 401, `status ${r.status}`);
});

const NEWPW = `Aips-Live-${crypto.randomBytes(4).toString('hex')}-9!`;
let secret = null;
let changed = false;
const browser = await chromium.launch();
await test('browser login (code + PKCE): temporary password change and mandatory TOTP enrolment', async () => {
  const ctx = await browser.newContext();
  const page = await ctx.newPage();
  await page.goto(CORE + '/');
  await page.waitForURL(/realms\/aips/, { timeout: 15000 });
  ok(page.url().includes('code_challenge_method=S256'), 'PKCE S256 in the authorisation request');
  await page.fill('#username', 'demo.engineer');
  await page.fill('#password', 'ChangeMe-Dev-Only-1!');
  await page.click('#kc-login');
  for (let step = 0; step < 4 && !page.url().startsWith(CORE); step++) {
    await page.waitForLoadState('domcontentloaded');
    if (await page.locator('#password-new').count()) {
      changed = true;
      await page.fill('#password-new', NEWPW);
      await page.fill('#password-confirm', NEWPW);
      await page.locator('input[type=submit], button[type=submit]').first().click();
    } else if (await page.locator('#totp').count()) {
      const link = page.locator('#mode-manual');
      if (await link.count()) await link.click();
      secret = (await page.locator('#kc-totp-secret-key').innerText()).trim();
      await page.fill('#totp', totp(secret));
      if (await page.locator('#userLabel').count()) await page.fill('#userLabel', 'live-test');
      await page.locator('input[type=submit], button[type=submit]').first().click();
    }
    await page.waitForTimeout(1500);
  }
  await page.waitForURL((u) => u.toString().startsWith(CORE), { timeout: 15000 });
  await page.getByText('Fleet status').first().waitFor({ timeout: 15000 }).catch(async (e) => {
    await page.screenshot({ path: '.data/kc-fail.png' });
    console.log('        url:', page.url(), '\n        text:', (await page.locator('body').innerText()).slice(0, 400));
    throw e;
  });
  ok(secret, 'TOTP enrolment was required');
  ok(changed, 'the shipped temporary password had to be changed at first login');
  await page.screenshot({ path: 'docs/screenshots/27-keycloak-login.png' });
  await ctx.close();
});
await test('second login requires the TOTP code; a wrong code is refused', async () => {
  ok(secret, 'needs enrolment from previous test');
  const ctx = await browser.newContext();
  const page = await ctx.newPage();
  await page.goto(CORE + '/');
  await page.waitForURL(/realms\/aips/, { timeout: 15000 });
  await page.fill('#username', 'demo.engineer');
  await page.fill('#password', NEWPW);
  await page.click('#kc-login');
  await page.locator('#otp').waitFor({ timeout: 10000 }).catch(async (e) => {
    await page.screenshot({ path: '.data/kc-fail2.png' });
    console.log('        url:', page.url(), '\n        text:', (await page.locator('body').innerText()).slice(0, 300));
    throw e;
  });
  await page.fill('#otp', '000000');
  await page.click('#kc-login');
  await page.waitForTimeout(1000);
  ok(!page.url().startsWith(CORE), 'wrong OTP must not sign in');
  // Codes are single-use: wait for the next 30 s step so the enrolment code is not replayed.
  await page.waitForTimeout(30_000 - (Date.now() % 30_000) + 1000);
  await page.fill('#otp', totp(secret));
  await page.click('#kc-login');
  await page.waitForURL((u) => u.toString().startsWith(CORE), { timeout: 15000 });
  await page.getByText('Fleet status').first().waitFor({ timeout: 15000 });
  await ctx.close();
});
await browser.close();
const passed = results.filter((r) => r.ok).length;
console.log(`\n${passed}/${results.length} passed`);
fs.mkdirSync('.data', { recursive: true });
fs.writeFileSync('.data/keycloak-results.json', JSON.stringify(results, null, 2));
process.exit(passed === results.length ? 0 : 1);
