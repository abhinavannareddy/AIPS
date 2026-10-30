import { strict as assert } from 'node:assert';
import { test } from 'node:test';
import { escalationPlan, isCaseOpened } from '../plan';

test('level 1 at SLA due, level 2 after half the window (bounded)', () => {
  const p = escalationPlan({ opened_at: '2026-10-30T00:00:00Z', sla_due: '2026-10-31T00:00:00Z', priority: 'caution' });
  assert.equal(p.length, 2);
  assert.equal(p[0].atMs, Date.parse('2026-10-31T00:00:00Z'));
  assert.equal(p[1].atMs - p[0].atMs, 12 * 3_600_000);
  const short = escalationPlan({ opened_at: '2026-10-30T00:00:00Z', sla_due: '2026-10-30T01:00:00Z', priority: 'warning' });
  assert.equal(short[1].atMs - short[0].atMs, 3_600_000);
});

test('rejects invalid timestamps', () => {
  assert.throws(() => escalationPlan({ opened_at: 'x', sla_due: '2026-10-30T00:00:00Z', priority: 'caution' }));
  assert.throws(() => escalationPlan({ opened_at: '2026-10-31T00:00:00Z', sla_due: '2026-10-30T00:00:00Z', priority: 'caution' }));
});

test('validates Kafka payloads before starting workflows', () => {
  const t = { opened_at: '2026-10-30T00:00:00Z', sla_due: '2026-10-30T04:00:00Z' };
  assert.ok(isCaseOpened({ type: 'case.opened', case_id: 'CASE-1A2B3C4D', ...t }));
  assert.ok(!isCaseOpened({ type: 'case.opened', case_id: '../../etc', ...t }));
  // Regression: unparseable or reversed dates were accepted and the workflow retried forever.
  assert.ok(!isCaseOpened({ type: 'case.opened', case_id: 'CASE-1A2B3C4D', sla_due: 'a', opened_at: 'b' }));
  assert.ok(!isCaseOpened({ type: 'case.opened', case_id: 'CASE-1A2B3C4D', opened_at: t.sla_due, sla_due: t.opened_at }));
  assert.ok(!isCaseOpened(null));
});

import { isPreflightConcern, preflightEscalationAt } from '../plan';

test('pre-departure escalation point is STD minus the configured lead time', () => {
  const at = preflightEscalationAt({ std: '2026-10-30T12:00:00Z', escalate_before_min: 45 });
  assert.equal(at, Date.parse('2026-10-30T11:15:00Z'));
  assert.throws(() => preflightEscalationAt({ std: 'nope', escalate_before_min: 45 }));
});

test('validates preflight.concern payloads', () => {
  const ok = { type: 'preflight.concern', flight_id: 'SE-RAA-NA1028-20261030', tail: 'SE-RAA', std: '2026-10-30T12:00:00Z', escalate_before_min: 45 };
  assert.ok(isPreflightConcern(ok));
  assert.ok(!isPreflightConcern({ ...ok, flight_id: '../../etc' }));
  assert.ok(!isPreflightConcern({ ...ok, std: 'x' }));
  assert.ok(!isPreflightConcern({ ...ok, escalate_before_min: -1 }));
  assert.ok(!isPreflightConcern({ ...ok, type: 'case.opened' }));
});
