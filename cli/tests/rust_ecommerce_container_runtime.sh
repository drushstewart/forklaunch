#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FORKLAUNCH_CLI="${FORKLAUNCH_CLI:-${FORKLAUNCH_TEST_CLI:-}}"
run_cli() {
    if [ -n "$FORKLAUNCH_CLI" ]; then "$FORKLAUNCH_CLI" "$@";
    else cargo run --manifest-path "$REPO_ROOT/cli/Cargo.toml" --release -- "$@"; fi
}
mkdir -p "$SCRIPT_DIR/output/rust-ecommerce-container-runtime"
RUN_ROOT="$(mktemp -d "$SCRIPT_DIR/output/rust-ecommerce-container-runtime/run.XXXXXX")"
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
}
trap cleanup EXIT
run_cli init application container-shop --path "$RUN_ROOT/app" --language rust --modules ecommerce-stripe -D 'Ecommerce container' -A ForkLaunch -L MIT
cp -R "$RUN_ROOT/app" "$RUN_ROOT/relocated-app"
APP_ROOT="$RUN_ROOT/relocated-app"
docker build --progress=plain -t "$IMAGE" "$APP_ROOT"
docker network create "$NETWORK"
docker run --detach --name "$DATABASE" --network "$NETWORK" --tmpfs /var/lib/postgresql/data:rw,size=256m --env POSTGRES_PASSWORD=parity-test-only --env POSTGRES_DB=parity postgres:16-alpine
for _ in $(seq 1 60); do
    if docker exec "$DATABASE" pg_isready -U postgres -d parity >/dev/null 2>&1; then break; fi
    sleep 1
done
docker run --rm --network "$NETWORK" --env "DATABASE_URL=postgres://postgres:parity-test-only@$DATABASE:5432/parity" "$IMAGE" ./target/release/ecommerce --migrate
docker exec -i "$DATABASE" psql -U postgres -d parity -v ON_ERROR_STOP=1 <<'SQL'
CREATE ROLE ecommerce_app LOGIN NOSUPERUSER NOBYPASSRLS PASSWORD 'fixture-restricted-password';
GRANT USAGE ON SCHEMA fl_ecommerce,fl_worker TO ecommerce_app;
GRANT SELECT,INSERT,UPDATE,DELETE ON fl_ecommerce.records,fl_worker.jobs TO ecommerce_app;
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
compose['networks']={'fixture':{'external':True,'name':network}}
for name,service in compose['services'].items():
    service.pop('build',None)
    service.pop('container_name',None)
    service['image']=image
    service['networks']=['fixture']
    env=service['environment']
    env.update(DATABASE_URL=f'postgres://ecommerce_app:fixture-restricted-password@{database}:5432/parity',TENANT_ID='container-ecommerce-fixture',ENCRYPTION_KEY='container-encryption-fixture')
    if name=='ecommerce':
        env.update(HMAC_SECRET_KEY='container-ecommerce-hmac-secret',JWKS_PUBLIC_KEY_URL='http://127.0.0.1:9/jwks',STRIPE_API_KEY='sk_fixture',STRIPE_WEBHOOK_SECRET='whsec_fixture',HOST='0.0.0.0',PORT='8000')
        service['ports']=['127.0.0.1::8000']
    else:
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
done
echo 'PASS generated ecommerce Linux Compose HTTP/MCP, restricted PostgreSQL, separate inventory worker, paid/cancelled delivery and graceful shutdown'
