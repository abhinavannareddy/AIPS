// Pure escalation-schedule logic shared by the workflow and its unit tests.
export type Priority = 'info' | 'advisory' | 'caution' | 'warning';

export interface CaseOpened {
  type: 'case.opened';
  case_id: string;
  tail: string;
  priority: Priority;
  sla_due: string; // RFC 3339
  opened_at: string;
  team: string;
}

export interface EscalationStep {
  level: number;
  atMs: number; // epoch milliseconds
  reason: string;
}

/** Level 1 at the SLA deadline; level 2 after a further half SLA window (min 1 h, max 12 h). */
export function escalationPlan(c: Pick<CaseOpened, 'opened_at' | 'sla_due' | 'priority'>): EscalationStep[] {
  const opened = Date.parse(c.opened_at);
  const due = Date.parse(c.sla_due);
  if (Number.isNaN(opened) || Number.isNaN(due) || due < opened) {
    throw new Error('invalid case timestamps');
  }
  const window = due - opened;
  const extra = Math.min(Math.max(window / 2, 3_600_000), 12 * 3_600_000);
  return [
    { level: 1, atMs: due, reason: `SLA breached (${c.priority}); escalated to duty engineering manager` },
    { level: 2, atMs: due + extra, reason: 'Still unresolved after escalation; escalated to head of continuing airworthiness' },
  ];
}

export function isCaseOpened(v: unknown): v is CaseOpened {
  const o = v as Record<string, unknown>;
  return !!o && o.type === 'case.opened' && typeof o.case_id === 'string' && /^[A-Z0-9-]{4,64}$/.test(o.case_id)
    && typeof o.sla_due === 'string' && typeof o.opened_at === 'string'
    // Same rule as escalationPlan: a bad date would otherwise fail the workflow task forever.
    && !Number.isNaN(Date.parse(o.opened_at)) && Date.parse(o.sla_due) >= Date.parse(o.opened_at);
}

/** PRV-04: published by aips-core when a pre-departure report first shows a significant concern. */
export interface PreflightConcern {
  type: 'preflight.concern';
  flight_id: string;
  tail: string;
  std: string; // RFC 3339 scheduled departure
  escalate_before_min: number;
}

export function isPreflightConcern(v: unknown): v is PreflightConcern {
  const o = v as Record<string, unknown>;
  return !!o && o.type === 'preflight.concern' && typeof o.flight_id === 'string' && /^[A-Za-z0-9._:-]{4,96}$/.test(o.flight_id)
    && typeof o.std === 'string' && !Number.isNaN(Date.parse(o.std))
    && typeof o.escalate_before_min === 'number' && o.escalate_before_min > 0 && o.escalate_before_min <= 24 * 60;
}

/** Escalate this long before departure if nobody has reviewed the concern by then. */
export function preflightEscalationAt(c: Pick<PreflightConcern, 'std' | 'escalate_before_min'>): number {
  const std = Date.parse(c.std);
  if (Number.isNaN(std)) throw new Error('invalid std');
  return std - c.escalate_before_min * 60_000;
}
