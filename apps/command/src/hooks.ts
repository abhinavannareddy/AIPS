import { createContext, useCallback, useContext, useEffect, useRef, useState } from 'react';
import { api } from './api';
import type { Principal } from './types';

/** Increments whenever the backend publishes a new analysis snapshot (throttled). */
export const LiveContext = createContext<{ version: number; simNow: string | null; live: boolean; connected: boolean }>({ version: 0, simNow: null, live: false, connected: true });
export const PrincipalContext = createContext<Principal | null>(null);

export function useLive() { return useContext(LiveContext); }
export function usePrincipal() { return useContext(PrincipalContext); }
export function can(p: Principal | null, ...roles: string[]): boolean {
  if (!p) return false;
  return p.roles.some((r) => r === 'admin' || roles.includes(r));
}

export interface Loadable<T> { data: T | null; error: string | null; loading: boolean; reload: () => void }

/** Fetch with live refresh. Keeps showing the last good data while refreshing. */
export function useApi<T>(path: string | null, opts: { live?: boolean } = { live: true }): Loadable<T> {
  const { version } = useLive();
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState<boolean>(!!path);
  const [nonce, setNonce] = useState(0);
  const lastPath = useRef<string | null>(null);
  const liveKey = opts.live === false ? 0 : version;

  useEffect(() => {
    if (!path) { setLoading(false); return; }
    let cancelled = false;
    if (lastPath.current !== path) { setData(null); setLoading(true); lastPath.current = path; }
    api<T>(path)
      .then((d) => { if (!cancelled) { setData(d); setError(null); } })
      .catch((e: Error) => { if (!cancelled) setError(e.message); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [path, liveKey, nonce]);

  const reload = useCallback(() => setNonce((n) => n + 1), []);
  return { data, error, loading, reload };
}

/** Wraps a mutation with pending/error state. */
export function useAction() {
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const run = useCallback(async <R,>(fn: () => Promise<R>): Promise<R | undefined> => {
    setPending(true); setError(null);
    try { return await fn(); } catch (e) { setError((e as Error).message); return undefined; } finally { setPending(false); }
  }, []);
  return { pending, error, run, clearError: () => setError(null) };
}

export function useHashRoute(): [string[], URLSearchParams] {
  const read = () => {
    const raw = window.location.hash.replace(/^#\/?/, '');
    const [p, q] = raw.split('?');
    return { parts: (p || 'overview').split('/').map(decodeURIComponent), query: new URLSearchParams(q || '') };
  };
  const [route, setRoute] = useState(read);
  useEffect(() => {
    const on = () => setRoute(read());
    window.addEventListener('hashchange', on);
    return () => window.removeEventListener('hashchange', on);
  }, []);
  return [route.parts, route.query];
}

export function go(path: string): void { window.location.hash = `#/${path}`; }
