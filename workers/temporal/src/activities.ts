// Activities: authenticated calls back into aips-core. Retried by Temporal on failure.
import { ApplicationFailure } from '@temporalio/activity';

const CORE = process.env.AIPS_CORE_URL ?? 'http://aips-core:8090';

let cached: { token: string; exp: number } | undefined;

/** Service identity via Keycloak client credentials (short-lived token, cached until 30 s before expiry). */
async function authHeaders(): Promise<Record<string, string>> {
  if (process.env.AIPS_AUTH_MODE === 'dev') {
    if (process.env.AIPS_ENV === 'production') throw new Error('dev auth forbidden in production');
    return { 'x-aips-role': 'mcc_controller', 'x-aips-user': 'aips-workflow' };
  }
  if (!cached || cached.exp < Date.now() + 30_000) {
    const issuer = process.env.AIPS_OIDC_ISSUER ?? 'http://keycloak:8080/realms/aips';
    const body = new URLSearchParams({
      grant_type: 'client_credentials',
      client_id: process.env.AIPS_WORKFLOW_CLIENT_ID ?? 'aips-workflow',
      client_secret: process.env.AIPS_WORKFLOW_CLIENT_SECRET ?? '',
    });
    const r = await fetch(`${issuer}/protocol/openid-connect/token`, { method: 'POST', body });
    if (!r.ok) throw new Error(`token request failed: ${r.status}`);
    const j = (await r.json()) as { access_token: string; expires_in: number };
    cached = { token: j.access_token, exp: Date.now() + j.expires_in * 1000 };
  }
  return { authorization: `Bearer ${cached.token}` };
}

export async function caseStatus(caseId: string): Promise<string> {
  const r = await fetch(`${CORE}/api/cases/${encodeURIComponent(caseId)}`, { headers: await authHeaders() });
  if (r.status === 404) return 'missing';
  if (!r.ok) throw new Error(`case lookup failed: ${r.status}`);
  const j = (await r.json()) as { case: { status: string } };
  return j.case.status;
}

export async function escalateCase(caseId: string, level: number, reason: string): Promise<void> {
  const r = await fetch(`${CORE}/api/cases/${encodeURIComponent(caseId)}/escalate`, {
    method: 'POST',
    headers: { 'content-type': 'application/json', ...(await authHeaders()) },
    body: JSON.stringify({ level, reason }),
  });
  if (r.status === 403 || r.status === 400) {
    throw ApplicationFailure.nonRetryable(`escalation rejected: ${r.status}`);
  }
  if (!r.ok) throw new Error(`escalation failed: ${r.status}`);
}

export async function escalatePreflight(flightId: string): Promise<string> {
  const r = await fetch(`${CORE}/api/preflight/${encodeURIComponent(flightId)}/escalate`, {
    method: 'POST',
    headers: await authHeaders(),
  });
  if (r.status === 404) return 'no report (departed or removed)';
  if (r.status === 403 || r.status === 400) throw ApplicationFailure.nonRetryable(`preflight escalation rejected: ${r.status}`);
  if (!r.ok) throw new Error(`preflight escalation failed: ${r.status}`);
  const j = (await r.json()) as { escalated: boolean; reason?: string };
  return j.escalated ? 'escalated' : `not escalated (${j.reason ?? 'no reason'})`;
}
