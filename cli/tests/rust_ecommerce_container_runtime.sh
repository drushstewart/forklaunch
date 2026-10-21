#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/cli/target}"
export CARGO_INCREMENTAL=0
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-stable}"
export FORKLAUNCH_SKIP_FORMAT=1
# Images build from crates.io: a framework checkout outside the app is not in the build context.
unset FORKLAUNCH_FRAMEWORK_PATH
if [ "${FORKLAUNCH_WORKER_RUNTIME_LOCKED:-0}" != 1 ]; then
    exec python3 "$SCRIPT_DIR/fixtures/worker_runtime_lock.py" "$SCRIPT_DIR/rust_ecommerce_container_runtime.sh"
fi
FORKLAUNCH_CLI="${FORKLAUNCH_CLI:-${FORKLAUNCH_TEST_CLI:-}}"
run_cli() {
    if [ -n "$FORKLAUNCH_CLI" ]; then "$FORKLAUNCH_CLI" "$@";
    else cargo run --manifest-path "$REPO_ROOT/cli/Cargo.toml" --release -- "$@"; fi
}
mkdir -p "$SCRIPT_DIR/output/rust-ecommerce-container-runtime"
RUN_ROOT="$(mktemp -d "$SCRIPT_DIR/output/rust-ecommerce-container-runtime/run.XXXXXX")"
if [ -n "$FORKLAUNCH_CLI" ]; then
    cp "$FORKLAUNCH_CLI" "$RUN_ROOT/forklaunch"
    FORKLAUNCH_CLI="$RUN_ROOT/forklaunch"
    shasum -a 256 "$FORKLAUNCH_CLI"
fi
PASSED=false
PROJECT="fl-ecommerce-container-$$"
IMAGE="forklaunch-rust-ecommerce:fixture-$$"
NETWORK="$PROJECT-network"
DATABASE="$PROJECT-postgres"
COMPOSE="$RUN_ROOT/runtime.json"
cleanup() {
    if [ -f "$COMPOSE" ]; then docker compose -p "$PROJECT" -f "$COMPOSE" down --timeout 10 >/dev/null 2>&1 || true; fi
    docker rm -f "$DATABASE" >/dev/null 2>&1 || true
    docker network rm "$NETWORK" >/dev/null 2>&1 || true
    docker image rm "$IMAGE" >/dev/null 2>&1 || true
    if [ "$PASSED" = true ] && [ "${FORKLAUNCH_KEEP_FIXTURE:-0}" != 1 ]; then rm -rf "$RUN_ROOT"; fi
}
trap cleanup EXIT
run_cli init application container-shop --path "$RUN_ROOT/app" --database postgresql --validator zod --formatter prettier --linter eslint --http-framework express --runtime node --test-framework vitest -D 'Ecommerce container' -A ForkLaunch -L MIT
run_cli init module ecommerce --path "$RUN_ROOT/app" --language rust -m ecommerce-stripe
cp -R "$RUN_ROOT/app" "$RUN_ROOT/relocated-app"
APP_ROOT="$RUN_ROOT/relocated-app"
cargo generate-lockfile --manifest-path "$APP_ROOT/Cargo.toml"
docker build --progress=plain -f "$APP_ROOT/Dockerfile.rust" -t "$IMAGE" "$APP_ROOT"
docker network create "$NETWORK"
docker run --detach --name "$DATABASE" --network "$NETWORK" --tmpfs /var/lib/postgresql/data:rw,size=256m --env POSTGRES_PASSWORD=parity-test-only --env POSTGRES_DB=parity postgres:16-alpine
for _ in $(seq 1 60); do
    if docker exec "$DATABASE" pg_isready -U postgres -d parity >/dev/null 2>&1; then break; fi
    sleep 1
done
docker run --rm --network "$NETWORK" --env "DATABASE_URL=postgres://postgres:parity-test-only@$DATABASE:5432/parity" "$IMAGE" /app/bin/ecommerce --migrate
docker exec -i "$DATABASE" psql -U postgres -d parity -v ON_ERROR_STOP=1 <<'SQL'
CREATE ROLE ecommerce_app LOGIN NOSUPERUSER NOBYPASSRLS PASSWORD 'fixture-restricted-password';
GRANT USAGE ON SCHEMA fl_ecommerce,fl_worker TO ecommerce_app;
GRANT SELECT,INSERT,UPDATE,DELETE ON fl_ecommerce.order_retention,fl_ecommerce.records,fl_worker.jobs TO ecommerce_app;
GRANT SELECT,INSERT ON fl_ecommerce.inventory_event_receipts TO ecommerce_app;
GRANT SELECT,INSERT,UPDATE ON fl_ecommerce.webhook_events TO ecommerce_app;
GRANT INSERT ON fl_ecommerce.pii_read_audit TO ecommerce_app;
GRANT SELECT,INSERT ON fl_ecommerce.payment_reconciliation_audit,fl_ecommerce.payment_cancellation_audit TO ecommerce_app;
GRANT USAGE ON ALL SEQUENCES IN SCHEMA fl_worker TO ecommerce_app;
SQL
python3 - "$APP_ROOT" "$COMPOSE" "$IMAGE" "$NETWORK" "$DATABASE" <<'PY'
import json,subprocess,sys
from pathlib import Path
app,out,image,network,database=sys.argv[1:]
compose=json.loads(subprocess.check_output(['docker','compose','-f',str(Path(app)/'docker-compose.yaml'),'config','--no-interpolate','--format','json']))
compose.pop('name',None)
assert set(['ecommerce','ecommerce-inventory-worker']) <= set(compose['services'])
compose['services']={name:compose['services'][name] for name in ['ecommerce','ecommerce-inventory-worker']}
compose['networks']={'fixture':{'external':True,'name':network}}
for name,service in compose['services'].items():
    service.pop('build',None)
    service.pop('container_name',None)
    service.pop('depends_on',None)
    service.pop('volumes',None)
    service.pop('env_file',None)
    service['image']=image
    service['networks']=['fixture']
    env=service['environment']
    env.update(DATABASE_URL=f'postgres://ecommerce_app:fixture-restricted-password@{database}:5432/parity',TENANT_ID='container-ecommerce-fixture',ENCRYPTION_KEY='container-encryption-fixture')
    if name=='ecommerce':
        assert service['command']==['/app/bin/ecommerce']
        env.update(HMAC_SECRET_KEY='container-ecommerce-hmac-secret',JWKS_PUBLIC_KEY_URL='http://127.0.0.1:9/jwks',STRIPE_API_KEY='sk_fixture',STRIPE_WEBHOOK_SECRET='whsec_fixture',HOST='0.0.0.0',PORT='8000')
        service['ports']=['127.0.0.1::8000']
    else:
        assert service['command']==['/app/bin/ecommerce','--worker']
        env['WORKER_POLL_MS']='20'
    service['healthcheck'].update(interval='1s',timeout='3s',retries=30,start_period='1s')
Path(out).write_text(json.dumps(compose))
PY
docker compose -p "$PROJECT" -f "$COMPOSE" up --detach --no-build --wait --wait-timeout 90
ADDRESS="$(docker compose -p "$PROJECT" -f "$COMPOSE" port ecommerce 8000)"
node "$SCRIPT_DIR/fixtures/rust_ecommerce_container_runtime.mjs" "http://$ADDRESS"
docker compose -p "$PROJECT" -f "$COMPOSE" stop --timeout 15
for id in $(docker compose -p "$PROJECT" -f "$COMPOSE" ps --all --quiet); do
    test "$(docker inspect --format '{{.State.ExitCode}}' "$id")" = 0
    test "$(docker inspect --format '{{.Config.User}}' "$id")" = 65532:65532
done
PASSED=true
echo 'PASS generated ecommerce Linux Compose HTTP/MCP, restricted PostgreSQL, separate inventory worker, paid/cancelled delivery and graceful shutdown'
