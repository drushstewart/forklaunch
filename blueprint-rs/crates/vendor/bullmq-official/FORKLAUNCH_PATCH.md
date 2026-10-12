# ForkLaunch BullMQ outcome-data patch

Source: `bullmq-official` 1.3.5 from crates.io; archive SHA-256 `80692af1923c98cb2fddb0111b88090d2e1395be5e748bca3a18dfb63326cad8`. The published VCS metadata identifies `taskforcesh/bullmq` commit `0e07ef6cc1597b5bc080458d89a7333b4a5e9331`, path `rust`, with a dirty release tree. Runtime sources and Lua scripts were copied from the published crate; root LICENSE was retrieved from that commit. Upstream README and MIT attribution are retained.

The optional `forklaunch-outcome-data` feature emits `WorkerEvent::FailedWithData` immediately before each existing `Failed` notification. It includes the same original payload snapshot already used by the worker's outcome path, so callbacks still receive their event when a preprocessor failure removes its job. Existing `Failed` notifications, Redis scripts, retry/retention policies and wire formats are preserved. The feature is enabled only by the ForkLaunch adapter. The worker module passes the original data to its private permanent-failure helper; the outcome hook does not otherwise change processing behavior.

The crate manifest retains the published dependency pins, uses the ForkLaunch package name `forklaunch-bullmq-protocol` and version `0.1.0` so future releases do not require modifying an existing upstream registry package, adds this feature and an isolated workspace, and includes this patch note and LICENSE in source packages. The Rust library name remains `bullmq`; the adapter re-exports it as `protocol`. Keep this patch small and replace the vendored dependency when an upstream outcome-data API can provide the same guarantee.

The optional `forklaunch-job-options` feature preserves `keepLogs` (wire key `kl`), `stackTraceLimit`, `timestamp` and `sizeLimit` in single/bulk job option maps, and accepts the Node wire alias `kl` when reading job options. These fields previously disappeared from the upstream encoder. The ForkLaunch producer carries queue defaults into explicit job options while overriding attempts/backoff and leaving `jobId` unset, matching the actual TypeScript wrapper. This is a separate compatibility patch in `queue.rs` and `options.rs`; retry/retention Lua scripts remain unchanged.

The same `forklaunch-job-options` feature persists `failedReason` and the JSON
`stacktrace` array on immediate retries, delayed retries and terminal failures.
It reads existing history and passes the updated fields to the existing
lock-checked transition scripts; there is no standalone write that can recreate
an automatically removed job. `stackTraceLimit: 0` clears history, positive
limits keep the newest entries, and an absent limit retains all entries. Rust
processor errors contribute their diagnostic strings, rather than synthetic
JavaScript call stacks. Existing TypeScript history entries are preserved.
The actual TypeScript/native worker fixture compares final history and reasons
for recovered and terminal jobs across both retry paths and all limit forms.
Lua scripts and behavior with this feature disabled remain unchanged.

With `forklaunch-job-options`, repeat schedulers advance immediately after a job
is fetched and before user processing or preprocessor rejection, matching the
actual Node worker. The former outcome-time calls remain only when the feature
is disabled. This preserves scheduling cadence for slow processors, exposes the
queued iteration during active work and lets scheduler removal cancel it without
resurrection at completion. Zero `maxStartedAttempts` also disables that guard,
matching the Node worker's truthiness check; positive limits still apply.
Twelve actual-library scenarios cover interval/cron schedules in both directions,
three-iteration limits, completed/failed runs, inspection/removal, cancellation
during active work, preprocessor rejection and the zero-limit boundary. The
fixtures reproduced both timing and zero-limit mismatches before their fixes.

A shared compatibility encoder now carries `keepLogs` (`kl`), `stackTraceLimit`
and `sizeLimit` through ordinary add-job maps, persisted scheduler templates,
the first scheduled job and subsequent iterations. The scheduler fixture checks
these options on every completed/failed job and verifies two retained log rows
and the configured failure-history bound through real workers in both directions.
Scheduler timestamp/date/other advanced-option conformance remains separate.
