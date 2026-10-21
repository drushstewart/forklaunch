#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
if [ "${FORKLAUNCH_WORKER_RUNTIME_LOCKED:-}" != 1 ]; then
  exec python3 "$SCRIPT_DIR/fixtures/worker_runtime_lock.py" "${BASH_SOURCE[0]}" "$@"
fi
CLI="${FORKLAUNCH_CLI:-$REPO_ROOT/cli/target/debug/forklaunch}"
RUN_ROOT="${FORKLAUNCH_APALIS_TEST_ROOT:-$(mktemp -d "${TMPDIR:-/tmp}/apalis-profile.XXXXXX")}"
CONTAINER="forklaunch-apalis-profile-$$"
trap 'docker rm -f "$CONTAINER" >/dev/null 2>&1 || true' EXIT
echo "Apalis profile workspace: $RUN_ROOT"
"$CLI" init application native-worker-test --path "$RUN_ROOT/app" --language rust --framework-path "$REPO_ROOT/framework-rs" -D 'Native worker test' -A ForkLaunch -L MIT
if "$CLI" init worker wrong-backend --path "$RUN_ROOT/app" --language rust --runtime apalis -t redis -D Invalid; then
  echo 'Expected incompatible Apalis backend rejection'; exit 1
fi
test ! -e "$RUN_ROOT/app/src/wrong-backend"
"$CLI" init worker profile-jobs --path "$RUN_ROOT/app" --language rust --runtime apalis -t database -d postgresql -D 'Native worker'
cd "$RUN_ROOT/app"
printf '\n// application-owned profile marker\n' >> src/profile-jobs/consumer/worker_hooks.rs
"$CLI" init worker profile-jobs --path . --language rust -t database -d postgresql -D 'Native worker'
grep -q 'application-owned profile marker' src/profile-jobs/consumer/worker_hooks.rs
if "$CLI" init worker profile-jobs --path . --language rust --runtime leased -t database -d postgresql -D Invalid; then
  echo 'Expected implicit native-to-leased conversion rejection'; exit 1
fi
cargo +stable build -p profile-jobs
cargo +stable test -p profile-jobs
cargo +stable clippy -p profile-jobs --all-targets --no-deps -- -D warnings
cp "${CARGO_TARGET_DIR:-target}/debug/profile-jobs-worker" "$RUN_ROOT/profile-jobs"
cp "${CARGO_TARGET_DIR:-target}/debug/profile-jobs" "$RUN_ROOT/profile-server"
node "$SCRIPT_DIR/rust_mcp_service_startup.mjs" "$RUN_ROOT/profile-server"
node "$SCRIPT_DIR/rust_worker_service_policy.mjs" "$RUN_ROOT/profile-server"
python3 - <<'PY'
from pathlib import Path
import json,tomllib
root=Path('src/profile-jobs')
manifest=tomllib.loads(Path('.forklaunch/manifest.toml').read_text())
project=next(p for p in manifest['projects'] if p['name']=='profile-jobs')
assert project['metadata']['workerRuntime']=='apalis'
assert project['metadata']['workerService'] is True
assert not project['metadata'].get('workerControlService',False)
assert 'DATABASE_ALLOW_PLAINTEXT_DEVELOPMENT' in Path('docker-compose.yaml').read_text()
assert 'serve_management' not in (root/'worker.rs').read_text()
assert 'native::run_application' in (root/'consumer/bootstrapper.rs').read_text()
assert 'PostgresStorage' in (root/'consumer/worker_hooks.rs').read_text()
assert json.loads((root/'events/event-contract.json').read_text())['channel']=='work-v1'
assert (root/'events/typescript/events.ts').is_file()
assert (root/'events/rust/events.rs').is_file()
assert (root/'.forklaunch/compliance.json').is_file()
assert (root/'domain/schemas/jobs.rs').is_file()
assert (root/'api/routes/enqueue.rs').is_file()
assert (root/'sdk/typescript/sdk.ts').is_file()
PY
# Compile-time output watches must recover deleted assets, without source edits.
rm src/profile-jobs/events/typescript/events.ts
cargo +stable check -p profile-jobs
test -f src/profile-jobs/events/typescript/events.ts
# Run against a separate ephemeral database, never a reviewed Notes container.
docker run -d --name "$CONTAINER" --tmpfs /var/lib/postgresql/data:rw,size=256m \
  -e POSTGRES_PASSWORD=fixture-admin -e POSTGRES_DB=profile -p 127.0.0.1::5432 postgres:16-alpine >/dev/null
for attempt in $(seq 1 60); do
  if docker exec "$CONTAINER" pg_isready -U postgres -d profile >/dev/null 2>&1; then break; fi
  sleep 1
done
PORT="$(docker port "$CONTAINER" 5432/tcp | sed 's/.*://')"
docker exec -i "$CONTAINER" psql -v ON_ERROR_STOP=1 -U postgres -d profile <<'SQL'
CREATE ROLE profile_owner LOGIN PASSWORD 'fixture-owner';
CREATE ROLE profile_runtime LOGIN PASSWORD 'fixture-runtime';
GRANT CREATE ON DATABASE profile TO profile_owner;
GRANT CREATE ON SCHEMA public TO profile_owner;
SQL
export DATABASE_ALLOW_PLAINTEXT_DEVELOPMENT=true
export DATABASE_URL="postgresql://profile_owner:fixture-owner@127.0.0.1:$PORT/profile"
"$RUN_ROOT/profile-jobs" --migrate
"$RUN_ROOT/profile-jobs" --migrate-only
docker exec -i "$CONTAINER" psql -v ON_ERROR_STOP=1 -U postgres -d profile <<'SQL'
GRANT USAGE ON SCHEMA apalis TO profile_runtime;
GRANT SELECT,INSERT,UPDATE,DELETE ON ALL TABLES IN SCHEMA apalis TO profile_runtime;
GRANT USAGE,SELECT ON ALL SEQUENCES IN SCHEMA apalis TO profile_runtime;
GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA apalis TO profile_runtime;
SQL
export DATABASE_URL="postgresql://profile_runtime:fixture-runtime@127.0.0.1:$PORT/profile"
# The authenticated domain producer accepts trusted context, seals before queue I/O,
# and uses the same Apalis storage as the independent consumer.
mkdir -p src/profile-jobs/examples
cat > src/profile-jobs/examples/producer_probe.rs <<'RS'
#[path = "../events/rust/events.rs"]
mod event_sdk;
#[tokio::main]
async fn main() {
    let input=profile_jobs::domain::WorkInput { resource_id:uuid::Uuid::new_v4(), revision:1, message:"must-not-be-stored-in-plaintext".into() };
    let accepted=profile_jobs::domain::services::producer::enqueue(uuid::Uuid::from_u128(1),input).await.expect("restricted encrypted enqueue");
    assert!(!accepted.operation_id.is_empty());
    let key=forklaunch_core::encryption::EnvelopeKey::new(forklaunch_core::encryption::EnvelopeProfile::new("profile-jobs").unwrap(),[7;32]);
    let event=event_sdk::seal_work(&key,uuid::Uuid::from_u128(1),"profile-jobs",uuid::Uuid::new_v4(),event_sdk::WorkInput{resource_id:uuid::Uuid::new_v4(),revision:1,message:"sdk-secret".into()}).unwrap();
    assert!(!serde_json::to_string(&event).unwrap().contains("sdk-secret"));
    assert!(serde_json::from_value::<event_sdk::WorkInput>(serde_json::json!({"resourceId":uuid::Uuid::new_v4(),"revision":0,"message":"invalid"})).is_err());
}
RS
ENCRYPTION_KEY_HEX=0707070707070707070707070707070707070707070707070707070707070707 cargo +stable run -p profile-jobs --example producer_probe
docker exec "$CONTAINER" psql -U postgres -d profile -Atc "SELECT job::text FROM apalis.jobs" > "$RUN_ROOT/queued-envelope.json"
if grep -q must-not-be-stored-in-plaintext "$RUN_ROOT/queued-envelope.json"; then echo 'Queue stored plaintext'; exit 1; fi
test -s "$RUN_ROOT/queued-envelope.json"
export FORKLAUNCH_APALIS_BINARY="$RUN_ROOT/profile-jobs"
export FORKLAUNCH_APALIS_CONTAINER="$CONTAINER"
python3 - <<'PY'
import os,subprocess,time,signal
from pathlib import Path
binary=os.environ['FORKLAUNCH_APALIS_BINARY']
with Path(binary+'.runtime.log').open('w') as log:
 p=subprocess.Popen([binary],stdout=log,stderr=log)
 try:
  for _ in range(100):
   assert p.poll() is None,'native runtime exited before shutdown'
   row=subprocess.check_output(['docker','exec',os.environ['FORKLAUNCH_APALIS_CONTAINER'],'psql','-U','postgres','-d','profile','-Atc','SELECT count(*) FROM apalis.workers'],text=True).strip()
   if row!='0':break
   time.sleep(.1)
  else:raise AssertionError('native worker never registered')
  p.send_signal(signal.SIGTERM)
  assert p.wait(timeout=25)==0,'native worker did not drain successfully'
 finally:
  if p.poll() is None:p.kill();p.wait()
PY
if DATABASE_URL="postgresql://postgres:fixture-admin@127.0.0.1:$PORT/profile" "$RUN_ROOT/profile-jobs" > "$RUN_ROOT/privileged.log" 2>&1; then
  echo 'Expected privileged runtime rejection'; exit 1
fi
if grep -q fixture-admin "$RUN_ROOT/privileged.log"; then echo 'Credential leak'; exit 1; fi
if "$RUN_ROOT/profile-jobs" --control-service > "$RUN_ROOT/unsupported.log" 2>&1; then
  echo 'Native profile must not pretend it supports leased HTTP control'; exit 1
fi
echo 'PASS ordinary Apalis profile generation, reinit, build assets, restricted PostgreSQL lifecycle and graceful shutdown. Independent producer HTTP/MCP startup matrix passed; domain authorization and handler remain fail closed.'
if [ "${FORKLAUNCH_KEEP_APALIS_FIXTURE:-0}" != 1 ]; then rm -rf "$RUN_ROOT"; fi
