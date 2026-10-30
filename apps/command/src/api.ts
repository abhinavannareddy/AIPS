// HTTP client, authentication (Keycloak or explicit dev roles) and live-update stream.
import Keycloak from 'keycloak-js';
import type { Role } from './types';

export interface RuntimeConfig {
  authMode: 'keycloak' | 'dev';
  keycloak: { url: string; realm: string; clientId: string };
  emergencyApi: string;
  environment: string;
}

let config: RuntimeConfig | null = null;
let kc: Keycloak | null = null;
const DEV_ROLE_KEY = 'aips.devRole';

function storageGet(k: string): string | null {
  try { return window.localStorage.getItem(k); } catch { return null; }
}
function storageSet(k: string, v: string): void {
  try { window.localStorage.setItem(k, v); } catch { /* storage unavailable: role lives for this page only */ }
}

let devRole: Role = (storageGet(DEV_ROLE_KEY) as Role) || 'engineer';

export function getDevRole(): Role { return devRole; }
export function setDevRole(r: Role): void { devRole = r; storageSet(DEV_ROLE_KEY, r); }
export function getConfig(): RuntimeConfig {
  if (!config) throw new Error('configuration not loaded');
  return config;
}

export async function bootstrap(): Promise<RuntimeConfig> {
  const r = await fetch('/config.json');
  if (!r.ok) throw new Error(`Could not load configuration (HTTP ${r.status}). Is aips-core running?`);
  config = (await r.json()) as RuntimeConfig;
  if (import.meta.env.DEV) config.emergencyApi = '';
  if (config.authMode === 'keycloak') {
    kc = new Keycloak(config.keycloak);
    await kc.init({ onLoad: 'login-required', pkceMethod: 'S256', checkLoginIframe: false });
    window.setInterval(() => { void kc?.updateToken(60); }, 30_000);
  }
  return config;
}

export function logout(): void { void kc?.logout(); }

async function authHeaders(): Promise<Record<string, string>> {
  if (config?.authMode === 'keycloak' && kc) {
    await kc.updateToken(30);
    return { authorization: `Bearer ${kc.token}` };
  }
  return { 'x-aips-role': devRole, 'x-aips-user': `demo.${devRole}`, 'x-aips-org': devRole === 'lessor' ? 'Aurora Aviation Capital' : '' };
}

export class ApiError extends Error {
  constructor(public status: number, message: string) { super(message); }
}

function base(path: string): string {
  if (path.startsWith('/api/emergency') && config?.emergencyApi) return `${config.emergencyApi}${path}`;
  return path;
}

export async function api<T>(path: string, init?: { method?: string; body?: unknown }): Promise<T> {
  const headers: Record<string, string> = { ...(await authHeaders()) };
  if (init?.body !== undefined) headers['content-type'] = 'application/json';
  let r: Response;
  try {
    r = await fetch(base(path), { method: init?.method ?? 'GET', headers, body: init?.body !== undefined ? JSON.stringify(init.body) : undefined });
  } catch {
    throw new ApiError(0, 'The service is not reachable. Check that it is running and try again.');
  }
  if (!r.ok) {
    let msg = `Request failed (HTTP ${r.status})`;
    try { const j = (await r.json()) as { error?: string }; if (j.error) msg = j.error; } catch { /* non-JSON body */ }
    throw new ApiError(r.status, msg);
  }
  return (await r.json()) as T;
}

/** Server-sent events. EventSource cannot set headers, so credentials travel as query parameters.
 * Credentials are rebuilt on every (re)connect, so an expired token is never reused, and
 * onStatus reports whether the stream is connected (P4: a lost feed must be visible). */
export function openStream(path: string, onMessage: (data: unknown) => void, onStatus?: (connected: boolean) => void): () => void {
  let es: EventSource | null = null;
  let closed = false;
  let retry = 1000;
  const connect = async (): Promise<void> => {
    if (closed) return;
    const params = new URLSearchParams();
    if (config?.authMode === 'keycloak' && kc) {
      try { await kc.updateToken(30); } catch { /* expired session: the 401 below surfaces as disconnected */ }
      if (kc.token) params.set('access_token', kc.token);
    } else { params.set('role', devRole); params.set('user', `demo.${devRole}`); }
    if (closed) return;
    es = new EventSource(`${base(path)}?${params.toString()}`);
    es.onopen = () => { retry = 1000; onStatus?.(true); };
    es.onmessage = (ev) => { try { onMessage(JSON.parse(ev.data)); } catch { /* ignore keep-alives */ } };
    es.onerror = () => { es?.close(); onStatus?.(false); window.setTimeout(() => void connect(), retry); retry = Math.min(retry * 2, 15000); };
  };
  void connect();
  return () => { closed = true; es?.close(); };
}

/** Authenticated file download (CSV reports). Fetches with credentials, then saves via a blob URL. */
export async function download(path: string, filename: string): Promise<void> {
  const r = await fetch(base(path), { headers: await authHeaders() });
  if (!r.ok) throw new ApiError(r.status, `Download failed (HTTP ${r.status})`);
  const url = URL.createObjectURL(await r.blob());
  const a = document.createElement('a');
  a.href = url; a.download = filename; document.body.appendChild(a); a.click(); a.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
}
