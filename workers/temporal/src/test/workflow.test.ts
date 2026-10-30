// Workflow tests on a real local Temporal server (the Temporal CLI dev server).
// Activities are stubbed here: this verifies workflow logic (timers, signals, idempotency),
// not the HTTP integration with aips-core, which is covered by tests/live/live-infra.mjs.
import { strict as assert } from 'node:assert';
import { after, before, test } from 'node:test';
import { TestWorkflowEnvironment } from '@temporalio/testing';
import { Worker } from '@temporalio/worker';

const cli = process.env.TEMPORAL_CLI;
let env: TestWorkflowEnvironment | undefined;
const calls: string[] = [];

before(async () => {
  if (!cli) return;
  env = await TestWorkflowEnvironment.createLocal({ server: { executable: { type: 'existing-path', path: cli } } });
});
after(async () => { await env?.teardown(); });

async function run<T>(fn: () => Promise<T>): Promise<T> {
  const worker = await Worker.create({
    connection: env!.nativeConnection,
    taskQueue: 'test',
    workflowsPath: require.resolve('../workflows'),
    activities: {
      escalatePreflight: async (id: string) => { calls.push(`pf:${id}`); return 'escalated'; },
      caseStatus: async () => 'open',
      escalateCase: async (id: string, level: number) => { calls.push(`case:${id}:${level}`); },
    },
  });
  return worker.runUntil(fn());
}

test('preflight concern past its escalation point escalates once', { skip: !cli && 'TEMPORAL_CLI not set' }, async () => {
  const c = { type: 'preflight.concern', flight_id: 'SE-RAA-T1', tail: 'SE-RAA', std: new Date(Date.now() + 40 * 60_000).toISOString(), escalate_before_min: 45 };
  const r = await run(() => env!.client.workflow.execute('preflightConcernWorkflow', { taskQueue: 'test', workflowId: 'pf-1', args: [c] }));
  assert.match(r as string, /escalation point reached: escalated/);
  assert.deepEqual(calls.filter((x) => x === 'pf:SE-RAA-T1'), ['pf:SE-RAA-T1']);
});

test('a review signal before the escalation point prevents escalation', { skip: !cli && 'TEMPORAL_CLI not set' }, async () => {
  const c = { type: 'preflight.concern', flight_id: 'SE-RAA-T2', tail: 'SE-RAA', std: new Date(Date.now() + 3 * 3_600_000).toISOString(), escalate_before_min: 45 };
  const r = await run(async () => {
    const h = await env!.client.workflow.start('preflightConcernWorkflow', { taskQueue: 'test', workflowId: 'pf-2', args: [c] });
    await h.signal('preflightReviewed');
    return h.result();
  });
  assert.equal(r, 'reviewed before escalation point');
  assert.ok(!calls.includes('pf:SE-RAA-T2'));
});

test('duplicate start for the same workflow id is rejected (idempotent Kafka handling)', { skip: !cli && 'TEMPORAL_CLI not set' }, async () => {
  const c = { type: 'preflight.concern', flight_id: 'SE-RAA-T3', tail: 'SE-RAA', std: new Date(Date.now() + 3 * 3_600_000).toISOString(), escalate_before_min: 45 };
  await run(async () => {
    const h = await env!.client.workflow.start('preflightConcernWorkflow', { taskQueue: 'test', workflowId: 'pf-3', args: [c] });
    await assert.rejects(env!.client.workflow.start('preflightConcernWorkflow', { taskQueue: 'test', workflowId: 'pf-3', args: [c] }), /already started/i);
    await h.signal('preflightReviewed');
    await h.result();
  });
});
