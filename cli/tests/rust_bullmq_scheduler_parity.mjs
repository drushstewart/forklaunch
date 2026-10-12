import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { readFile, realpath } from 'node:fs/promises';
import { spawn } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';
import { redisTransport } from './fixtures/redis_resp.mjs';

const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..'
);
const require = createRequire(import.meta.url);
const sdkPath =
  process.env.BULLMQ_SDK_PATH ??
  '/private/tmp/fl-rust-bullmq-fixture/node_modules/bullmq';
const bullmq = require(sdkPath);
assert.equal(require(path.join(sdkPath, 'package.json')).version, '6.3.10');
const tsc =
  process.env.TSC_BIN ?? path.join(root, 'framework/node_modules/.bin/tsc');
const ts = require(
  require.resolve('typescript', { paths: [path.dirname(await realpath(tsc))] })
);
const compile = (source) =>
  ts.transpileModule(source, {
    compilerOptions: {
      module: ts.ModuleKind.CommonJS,
      target: ts.ScriptTarget.ES2022
    }
  }).outputText;
const url =
  process.env.FORKLAUNCH_TEST_REDIS_URL ?? 'redis://127.0.0.1:32770/12';
const transport = redisTransport(url);
const prefix = `fl_scheduler_${randomUUID().replaceAll('-', '')}`;
const env = {
  ...process.env,
  REDIS_KEY_PREFIX: prefix,
  FORKLAUNCH_TEST_REDIS_URL: url
};
const isolation = {};
vm.runInNewContext(
  compile(
    await readFile(
      path.join(
        root,
        'blueprint/implementations/worker/bullmq/domain/tenantIsolation.ts'
      ),
      'utf8'
    )
  ),
  { exports: isolation, process: { env } }
);
async function load(relative) {
  const exports = {};
  vm.runInNewContext(
    compile(await readFile(path.join(root, relative), 'utf8')),
    {
      exports,
      console,
      require(name) {
        if (name === 'bullmq') return bullmq;
        if (name === '../domain/tenantIsolation') return isolation;
        throw Error(`Unexpected runtime import ${name}`);
      }
    }
  );
  return exports;
}
const { BullMqWorkerProducer } = await load(
  'blueprint/implementations/worker/bullmq/producers/bullMqWorker.producer.ts'
);
const { BullMqWorkerConsumer } = await load(
  'blueprint/implementations/worker/bullmq/consumers/bullMqWorker.consumer.ts'
);
const handles = [],
  children = new Set();
function native(input) {
  const example =
    input.mode === 'consume' ? 'bullmq_parity' : 'bullmq_scheduler_parity';
  const child = spawn(
    'cargo',
    [
      'run',
      '--quiet',
      '--manifest-path',
      path.join(root, 'blueprint-rs/Cargo.toml'),
      '-p',
      'forklaunch-worker-redis',
      '--features',
      'bullmq',
      '--example',
      example
    ],
    { cwd: root, env, stdio: ['pipe', 'pipe', 'inherit'] }
  );
  children.add(child);
  let output = '';
  child.stdout.on('data', (chunk) => (output += chunk));
  child.stdin.end(JSON.stringify(input));
  const timer = setTimeout(() => child.kill('SIGTERM'), 300000);
  return new Promise((resolve, reject) =>
    child.once('close', (code) => {
      clearTimeout(timer);
      children.delete(child);
      try {
        assert.equal(code, 0);
        resolve(JSON.parse(output));
      } catch (error) {
        reject(error);
      }
    })
  );
}
async function waitFor(check) {
  const deadline = Date.now() + 15000;
  while (Date.now() < deadline) {
    if (await check()) return;
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  throw Error('Scheduler parity fixture timed out');
}
try {
  for (const nativeConsumes of [false, true]) {
    const queueName = `schedule-before-processing-${nativeConsumes}`,
      scheduler = 'active-schedule';
    const opts = {
      retries: 1,
      interval: 20,
      backoffType: 'fixed',
      queueOptions: { connection: { url } }
    };
    const producer = new BullMqWorkerProducer(queueName, opts);
    handles.push(() => producer.queue.close());
    await producer.queue.upsertJobScheduler(
      scheduler,
      { every: 100, limit: 3 },
      {
        name: 'scheduled-active',
        data: { id: 'scheduled-active', sleepMs: 3000 },
        opts: { attempts: 1 }
      }
    );
    let completion, consumer, release;
    if (nativeConsumes) {
      completion = native({
        mode: 'consume',
        queue: queueName,
        expected: 1,
        retries: 1
      });
      completion.catch(() => {});
    } else {
      const blocked = new Promise((resolve) => (release = resolve));
      consumer = new BullMqWorkerConsumer(
        queueName,
        opts,
        async () => blocked,
        () => {}
      );
      handles.push(async () => {
        release();
        await consumer.close();
      });
      await consumer.start();
    }
    await waitFor(async () => (await producer.queue.getActiveCount()) === 1);
    // TypeScript schedules the next iteration on fetch, before user processing.
    assert.equal(
      (await producer.queue.getWaitingCount()) +
        (await producer.queue.getDelayedCount()),
      1,
      `next iteration missing during active ${nativeConsumes ? 'native' : 'TS'} processing`
    );
    assert.equal(
      (await native({ mode: 'remove', queue: queueName, scheduler })).removed,
      true
    );
    assert.equal(
      (await producer.queue.getWaitingCount()) +
        (await producer.queue.getDelayedCount()),
      0
    );
    release?.();
    if (nativeConsumes) assert.equal((await completion).completed, 1);
    else {
      await waitFor(
        async () => (await producer.queue.getCompletedCount()) === 1
      );
      await consumer.close();
      handles.pop();
    }
    assert.equal(
      (await producer.queue.getWaitingCount()) +
        (await producer.queue.getDelayedCount()),
      0
    );
    assert.equal(await producer.queue.getJobScheduler(scheduler), undefined);
    await producer.queue.close();
    handles.pop();
    console.log(
      `PASS ${nativeConsumes ? 'native' : 'TS'} schedules before processing and removal cancels the queued iteration`
    );
  }
  for (const nativeConsumes of [false, true]) {
    const queueName = `preprocessor-schedule-${nativeConsumes}`,
      scheduler = 'rejected-start';
    const opts = {
      retries: 1,
      interval: 20,
      backoffType: 'fixed',
      queueOptions: { connection: { url }, maxStartedAttempts: 1 }
    };
    const producer = new BullMqWorkerProducer(queueName, opts);
    handles.push(() => producer.queue.close());
    const event = { id: 'scheduled-preprocessor' };
    const first = await producer.queue.upsertJobScheduler(
      scheduler,
      { every: 100, limit: 3 },
      { name: event.id, data: event, opts: { attempts: 1 } }
    );
    // Model a prior pickup of the fixture-owned initial iteration.
    await transport.command(
      'HSET',
      `${prefix}:${queueName}:${first.id}`,
      'ats',
      1
    );
    if (nativeConsumes) {
      const completion = native({
        mode: 'consume',
        queue: queueName,
        expected: 3,
        retries: 1,
        maxStartedAttempts: 1
      });
      completion.catch(() => {});
      await waitFor(
        async () =>
          (await producer.queue.getCompletedCount()) +
            (await producer.queue.getFailedCount()) ===
          3
      );
      const result = await completion;
      assert.equal(result.completed, 2);
      assert.equal(result.failed, 1);
      assert.equal(result.attempts.length, 2);
      assert.equal(result.failures.length, 1);
    } else {
      let processed = 0,
        failures = 0;
      const consumer = new BullMqWorkerConsumer(
        queueName,
        opts,
        async () => {
          processed++;
        },
        (values) => {
          failures += values.length;
        }
      );
      handles.push(() => consumer.close());
      await consumer.start();
      await waitFor(
        async () =>
          (await producer.queue.getCompletedCount()) +
            (await producer.queue.getFailedCount()) ===
          3
      );
      await consumer.close();
      handles.pop();
      assert.equal(processed, 2);
      assert.equal(failures, 1);
    }
    assert.equal(
      (await producer.queue.getJob(first.id)).failedReason,
      'job started more than allowable limit'
    );
    assert.equal(
      (await producer.queue.getWaitingCount()) +
        (await producer.queue.getDelayedCount()),
      0
    );
    assert.equal(
      (await native({ mode: 'remove', queue: queueName, scheduler })).removed,
      true
    );
    await producer.queue.close();
    handles.pop();
    console.log(
      `PASS ${nativeConsumes ? 'native' : 'TS'} preprocessor rejection preserves the scheduled sequence`
    );
  }
  for (const nativeProduces of [false, true])
    for (const kind of ['every', 'cron'])
      for (const fails of [false, true]) {
        const queueName = `scheduler-${nativeProduces}-${kind}-${fails}`,
          scheduler = 'configured-schedule';
        const repeat = {
          ...(kind === 'every'
            ? { every: 100 }
            : { pattern: '* * * * * *', tz: 'America/Los_Angeles' }),
          limit: 3
        };
        const event = {
          id: 'scheduled-event',
          failAttempts: fails ? 10 : 0,
          logRows: ['one', 'two', 'three']
        };
        const jobOptions = {
          attempts: 1,
          backoff: { type: 'fixed', delay: 20 },
          keepLogs: 2,
          stackTraceLimit: 1,
          sizeLimit: 2048
        };
        const opts = {
          retries: 1,
          interval: 20,
          backoffType: 'fixed',
          queueOptions: {
            connection: { url },
            defaultJobOptions: jobOptions,
            maxStartedAttempts: 0
          }
        };
        const producer = new BullMqWorkerProducer(queueName, opts);
        handles.push(() => producer.queue.close());
        let initial;
        if (nativeProduces)
          initial = (
            await native({
              mode: 'upsert',
              queue: queueName,
              scheduler,
              repeat,
              event,
              jobOptions
            })
          ).id;
        else
          initial = (
            await producer.queue.upsertJobScheduler(scheduler, repeat, {
              name: event.id,
              data: event,
              opts: jobOptions
            })
          ).id;
        assert.ok(initial.startsWith(`repeat:${scheduler}:`));
        const stored = await producer.queue.getJob(initial);
        assert.deepEqual(stored.data, event);
        assert.equal(stored.name, event.id);
        assert.equal(stored.opts.repeat.limit, 3);
        assert.equal(stored.repeatJobKey, scheduler);
        if (nativeProduces) {
          const observed = [];
          const consumer = new BullMqWorkerConsumer(
            queueName,
            opts,
            async (values) => {
              observed.push(...values);
              const [active] = await producer.queue.getJobs(['active']);
              for (const row of values[0].logRows) await active.log(row);
              if (fails) throw Error('scheduled_failure');
            },
            () => {}
          );
          handles.push(() => consumer.close());
          await consumer.start();
          await waitFor(
            async () =>
              (await producer.queue.getCompletedCount()) +
                (await producer.queue.getFailedCount()) ===
              3
          );
          await consumer.close();
          handles.pop();
          assert.deepEqual(JSON.parse(JSON.stringify(observed)), [
            event,
            event,
            event
          ]);
        } else {
          const completion = native({
            mode: 'consume',
            queue: queueName,
            expected: 3,
            retries: 1,
            maxStartedAttempts: 0
          });
          // Keep rejection handled while Redis independently bounds the runtime wait.
          completion.catch(() => {});
          await waitFor(
            async () =>
              (await producer.queue.getCompletedCount()) +
                (await producer.queue.getFailedCount()) ===
              3
          );
          const result = await completion;
          assert.deepEqual(
            result.attempts.map((attempt) => attempt.event),
            [event, event, event]
          );
          assert.equal(result.failed, fails ? 3 : 0);
          assert.equal(result.completed, fails ? 0 : 3);
        }
        const jobs = await producer.queue.getJobs(['completed', 'failed']);
        assert.equal(jobs.length, 3);
        assert.deepEqual(
          jobs.map((job) => job.opts.repeat.count).sort((a, b) => a - b),
          [1, 2, 3]
        );
        for (const job of jobs) {
          assert.equal(job.repeatJobKey, scheduler);
          assert.equal(job.opts.attempts, 1);
          assert.deepEqual(job.opts.backoff, jobOptions.backoff);
          assert.equal(job.opts.repeat.limit, 3);
          assert.equal(typeof job.opts.timestamp, 'number');
          assert.ok(Math.abs(job.opts.timestamp - job.timestamp) < 100);
          if (kind === 'cron')
            assert.equal(job.opts.prevMillis, Number(job.id.split(':').at(-1)));
          else assert.equal(job.opts.prevMillis, undefined);
          assert.equal(job.opts.keepLogs, 2);
          assert.equal(job.opts.stackTraceLimit, 1);
          assert.equal(job.opts.sizeLimit, 2048);
          const logs = await producer.queue.getJobLogs(job.id);
          assert.deepEqual(logs.logs, ['two', 'three']);
          assert.equal(logs.count, 2);
          assert.equal(job.stacktrace.length, fails ? 1 : 0);
        }
        assert.equal(await producer.queue.getWaitingCount(), 0);
        assert.equal(await producer.queue.getDelayedCount(), 0);
        assert.equal(
          (await native({ mode: 'inspect', queue: queueName, scheduler }))
            .scheduler.name,
          event.id
        );
        assert.equal(
          (await native({ mode: 'remove', queue: queueName, scheduler }))
            .removed,
          true
        );
        assert.equal(
          await producer.queue.getJobScheduler(scheduler),
          undefined
        );
        assert.equal(
          (await native({ mode: 'remove', queue: queueName, scheduler }))
            .removed,
          false
        );
        await producer.queue.close();
        handles.pop();
        console.log(
          `PASS scheduler ${nativeProduces ? 'native-to-TS' : 'TS-to-native'} ${kind} ${fails ? 'failed' : 'completed'}`
        );
      }
  console.log(
    'PASS 12 actual TypeScript/native BullMQ scheduler lifecycle scenarios, including fetch timing, active removal, preprocessor rejection and disabled zero start limits'
  );
} finally {
  await Promise.all(
    [...children].map(
      (child) =>
        new Promise((resolve) => {
          child.once('close', resolve);
          child.kill('SIGTERM');
        })
    )
  );
  for (const close of handles.reverse()) await close();
  let cursor = '0';
  do {
    const [next, keys] = await transport.command(
      'SCAN',
      cursor,
      'MATCH',
      `${prefix}:*`,
      'COUNT',
      1000
    );
    if (keys.length) await transport.command('DEL', ...keys);
    cursor = String(next);
  } while (cursor !== '0');
}
