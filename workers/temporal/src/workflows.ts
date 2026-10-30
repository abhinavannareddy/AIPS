// Temporal workflows (deterministic: no I/O here; side effects live in activities).
import { condition, defineSignal, proxyActivities, setHandler, sleep, workflowInfo } from '@temporalio/workflow';
import type * as activities from './activities';
import { escalationPlan, preflightEscalationAt, type CaseOpened, type PreflightConcern } from './plan';

const { escalateCase, caseStatus, escalatePreflight } = proxyActivities<typeof activities>({
  startToCloseTimeout: '30 seconds',
  retry: { initialInterval: '5 seconds', backoffCoefficient: 2, maximumAttempts: 20 },
});

export const caseClosedSignal = defineSignal('caseClosed');

/**
 * Durable SLA timer for an investigation case (WRK-05). Survives worker and core restarts:
 * Temporal persists the timers, so an escalation can never be lost to a crash.
 */
export async function caseSlaWorkflow(c: CaseOpened): Promise<string> {
  let closed = false;
  setHandler(caseClosedSignal, () => {
    closed = true;
  });
  for (const step of escalationPlan(c)) {
    const wait = Math.max(0, step.atMs - Date.now()); // workflow Date.now() is deterministic
    if (await condition(() => closed, wait)) return `closed before level ${step.level}`;
    // Re-check the system of record before acting; humans may have closed it in the UI.
    const status = await caseStatus(c.case_id);
    if (status === 'closed' || status === 'missing') return `case ${status}; no escalation`;
    await escalateCase(c.case_id, step.level, step.reason);
  }
  await sleep('1 minute');
  return `escalated through all levels (${workflowInfo().workflowId})`;
}

export const preflightReviewedSignal = defineSignal('preflightReviewed');

/**
 * PRV-04: durable pre-departure escalation. If the concern is still unreviewed at
 * STD - escalate_before_min, ask aips-core to escalate; aips-core re-checks the report and
 * decides (reviewed or resolved concerns are not escalated). Never a dispatch decision.
 */
export async function preflightConcernWorkflow(c: PreflightConcern): Promise<string> {
  let reviewed = false;
  setHandler(preflightReviewedSignal, () => {
    reviewed = true;
  });
  const wait = Math.max(0, preflightEscalationAt(c) - Date.now());
  if (await condition(() => reviewed, wait)) return 'reviewed before escalation point';
  const outcome = await escalatePreflight(c.flight_id);
  return `escalation point reached: ${outcome}`;
}
