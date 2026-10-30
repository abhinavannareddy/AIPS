// AIPS workflow worker: runs Temporal workflows and starts one per case from Kafka (aips.cases).
import { Client, Connection, WorkflowExecutionAlreadyStartedError } from '@temporalio/client';
import { NativeConnection, Worker } from '@temporalio/worker';
import { Kafka, logLevel } from 'kafkajs';
import * as activities from './activities';
import { isCaseOpened, isPreflightConcern } from './plan';

const TASK_QUEUE = 'aips-workflow';

async function main(): Promise<void> {
  const address = process.env.TEMPORAL_ADDRESS ?? 'temporal:7233';
  const namespace = process.env.TEMPORAL_NAMESPACE ?? 'aips';

  const worker = await Worker.create({
    connection: await NativeConnection.connect({ address }),
    namespace,
    taskQueue: TASK_QUEUE,
    workflowsPath: require.resolve('./workflows'),
    activities,
  });
  const client = new Client({ connection: await Connection.connect({ address }), namespace });

  const kafka = new Kafka({
    clientId: 'aips-workflow',
    brokers: (process.env.KAFKA_BROKERS ?? 'kafka:9092').split(','),
    logLevel: logLevel.WARN,
  });
  const consumer = kafka.consumer({ groupId: 'aips-workflow' });
  await consumer.connect();
  await consumer.subscribe({ topic: 'aips.cases', fromBeginning: true });
  await consumer.run({
    eachMessage: async ({ message }) => {
      let payload: unknown;
      try {
        payload = JSON.parse(message.value?.toString() ?? '');
      } catch {
        console.warn('rejected malformed aips.cases message');
        return;
      }
      if (isPreflightConcern(payload)) {
        try {
          // One workflow per flight: duplicate deliveries are idempotent.
          await client.workflow.start('preflightConcernWorkflow', {
            taskQueue: TASK_QUEUE,
            workflowId: `preflight-${payload.flight_id}`,
            args: [payload],
          });
          console.log(`pre-departure escalation workflow started for ${payload.flight_id}`);
        } catch (e) {
          if (!(e instanceof WorkflowExecutionAlreadyStartedError)) throw e;
        }
        return;
      }
      if (!isCaseOpened(payload)) return;
      try {
        // Workflow id = case id: duplicate Kafka deliveries cannot start a second timer.
        await client.workflow.start('caseSlaWorkflow', {
          taskQueue: TASK_QUEUE,
          workflowId: `case-sla-${payload.case_id}`,
          args: [payload],
        });
        console.log(`SLA workflow started for ${payload.case_id}`);
      } catch (e) {
        if (!(e instanceof WorkflowExecutionAlreadyStartedError)) throw e;
      }
    },
  });

  // The Temporal SDK already stops the worker on SIGINT/SIGTERM (graceful drain). We only close
  // the Kafka consumer; calling worker.shutdown() a second time threw IllegalStateError and made
  // every graceful stop exit with an error (bug B13).
  const shutdown = async (): Promise<void> => {
    await consumer.disconnect().catch(() => undefined);
    if (worker.getState() === 'RUNNING') worker.shutdown();
  };
  process.on('SIGTERM', shutdown);
  process.on('SIGINT', shutdown);
  await worker.run();
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
