#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/cli/target}"
export CARGO_INCREMENTAL=0 RUSTUP_TOOLCHAIN=stable
export FORKLAUNCH_SKIP_FORMAT=1
if [ "${FORKLAUNCH_WORKER_RUNTIME_LOCKED:-}" != 1 ]; then
  exec python3 "$SCRIPT_DIR/fixtures/worker_runtime_lock.py" "${BASH_SOURCE[0]}" "$@"
fi
CLI="${FORKLAUNCH_CLI:-$REPO_ROOT/cli/target/debug/forklaunch}"
RUN_ROOT="${FORKLAUNCH_POSTGRES_SERVICE_ROOT:-$(mktemp -d "${TMPDIR:-/tmp}/postgres-service.XXXXXX")}"
CONTAINER="forklaunch-generic-postgres-$$"
cleanup() {
  docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
  if [ -n "${FORKLAUNCH_KEEP_FIXTURE:-}" ]; then
    echo "Fixture retained: $RUN_ROOT"
  elif [ -z "${FORKLAUNCH_POSTGRES_SERVICE_ROOT:-}" ]; then
    rm -rf "$RUN_ROOT"
  fi
}
trap cleanup EXIT
echo "PostgreSQL service workspace: $RUN_ROOT"
python3 - "$CLI" <<'PYTHON'
import hashlib, sys
from pathlib import Path
print('CLI SHA256:', hashlib.sha256(Path(sys.argv[1]).read_bytes()).hexdigest(), flush=True)
PYTHON
"$CLI" init application storage-test --path "$RUN_ROOT/app" --database postgresql \
  --validator zod --formatter prettier --linter eslint --http-framework express \
  --runtime node --test-framework vitest -D Storage -A ForkLaunch -L MIT
if "$CLI" init service unsupported --path "$RUN_ROOT/app" --language rust -d sqlite -D Unsupported; then
  echo 'Expected unsupported database rejection'; exit 1
fi
test ! -e "$RUN_ROOT/app/src/unsupported"
"$CLI" init service records-api --path "$RUN_ROOT/app" --language rust -d postgresql -D Records
"$CLI" init service memory-api --path "$RUN_ROOT/app" --language rust -D Memory
"$CLI" init router users --path "$RUN_ROOT/app/src/records-api" --language rust
"$CLI" init router users --path "$RUN_ROOT/app/src/memory-api" --language rust
cd "$RUN_ROOT/app"
python3 - <<'PYTHON'
import tomllib
from pathlib import Path
m=tomllib.loads(Path('.forklaunch/manifest.toml').read_text())
p={p['name']:p for p in m['projects']}
assert p['records-api']['resources']['database']=='postgresql'
assert p['records-api']['metadata']['persistence']=='postgresql'
assert 'database' not in p['memory-api'].get('resources',{})
assert p['memory-api']['metadata']['persistence']=='memory'
PYTHON
cat >> src/records-api/__test__/persistence.rs <<'RUST'

#[tokio::test]
#[ignore = "requires migrated isolated PostgreSQL and a restricted runtime role"]
async fn generated_router_counts_only_the_tenant_without_disclosing_protected_fields() {
    use axum::{body::{Body, to_bytes}, http::Request};
    use tower::ServiceExt;
    let state = records_api::register(records_api::config::ServiceConfig::default()).await.unwrap();
    let repository = state.repository.clone();
    let before = repository.count().await.unwrap();
    let id = Uuid::new_v4();
    let canary = "router-protected-plaintext-canary";
    repository.insert(id, canary).await.unwrap();
    let router = records_api::server::router(state.clone()).unwrap();
    let response = router.oneshot(Request::builder().uri("/users").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), 200);
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert!(!String::from_utf8_lossy(&body).contains(canary));
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value, serde_json::json!({"route": "users", "summary": {"service": state.config.name, "total": before + 1}}));

    let mut config = DatabaseConfig::from_env().unwrap();
    config.tenant = Uuid::new_v4();
    let other = PostgresRepository::connect(config).await.unwrap();
    let mut other_state = state.clone();
    other_state.repository = other.clone();
    let router = records_api::server::router(other_state).unwrap();
    let response = router.oneshot(Request::builder().uri("/users").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), 200);
    let value: serde_json::Value = serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap()).unwrap();
    assert_eq!(value["summary"]["total"], 0);
    assert!(value.get("records").is_none());
    assert!(other.protected_name(id).await.unwrap().is_none());

    let config = DatabaseConfig::from_env().unwrap();
    let context = TenantContext::new(config.tenant.to_string()).unwrap();
    let pool = connect_runtime_postgres(&config.url, config.policy).await.unwrap();
    let mut transaction = begin_tenant_transaction(&pool, &context).await.unwrap();
    sqlx::query("DELETE FROM records WHERE tenant_id=$1 AND id=$2")
        .bind(context.id()).bind(id).execute(&mut *transaction).await.unwrap();
    transaction.commit().await.unwrap();
    assert_eq!(repository.count().await.unwrap(), before);
    pool.close().await;
    other.close().await;
    repository.close().await;
    let router = records_api::server::router(state).unwrap();
    let response = router.oneshot(Request::builder().uri("/users").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), 503, "A closed repository fails as unavailable without exposing storage errors");
}
RUST
cat >> src/memory-api/__test__/http.rs <<'RUST'

#[tokio::test]
async fn memory_router_retains_its_typed_record_listing() {
    let state = memory_api::register(memory_api::config::ServiceConfig::default()).unwrap();
    let expected = state.repository.list().unwrap();
    assert!(!expected.is_empty());
    let router = memory_api::server::router(state).unwrap();
    let response = router.oneshot(Request::builder().uri("/users").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), 200);
    let value: serde_json::Value = serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap()).unwrap();
    assert_eq!(value["records"], serde_json::to_value(&expected).unwrap());
    assert_eq!(value["summary"]["total"], expected.len());
}
RUST
cargo check --workspace --all-targets
cargo build -p records-api
cargo test -p records-api
cargo test -p memory-api --test http
cargo clippy -p records-api --all-targets --no-deps -- -D warnings
python3 - <<'PYTHON'
import json, re
from pathlib import Path
for service, operation, description, fields in [
    ('records-api', 'summarizeUsers', 'Summarize tenant-scoped users records', {'route', 'summary'}),
    ('memory-api', 'listUsers', 'List users records', {'route', 'summary', 'records'}),
]:
    sdk = Path('src') / service / 'sdk/typescript'
    openapi = json.loads((sdk / 'openapi.json').read_text())
    endpoint = openapi['paths']['/users']['get']
    assert endpoint['operationId'] == operation, endpoint
    assert endpoint['summary'] == description, endpoint
    schema = endpoint['responses']['200']['content']['application/json']['schema']
    while '$ref' in schema:
        reference = schema['$ref']
        assert reference.startswith('#/'), reference
        schema = openapi
        for component in reference[2:].split('/'):
            schema = schema[component]
    assert set(schema['properties']) == fields, schema
    typescript = (sdk / 'sdk.ts').read_text()
    has_records = re.search(r'(?:\brecords|[\'"]records[\'"])\??\s*:', typescript) is not None
    assert has_records == ('records' in fields), typescript
PYTHON
"$CLI" sdk generate --target all --from contract --check --path .
cp "${CARGO_TARGET_DIR:-target}/debug/records-api" "$RUN_ROOT/records-api"
docker run -d --name "$CONTAINER" --tmpfs /var/lib/postgresql/data:rw,size=256m \
  -e POSTGRES_PASSWORD=fixture-admin -e POSTGRES_DB=service -p 127.0.0.1::5432 postgres:16-alpine >/dev/null
for attempt in $(seq 1 60); do
  if docker exec "$CONTAINER" pg_isready -h 127.0.0.1 -U postgres -d service >/dev/null 2>&1; then break; fi
  sleep 1
done
PORT="$(docker port "$CONTAINER" 5432/tcp | sed 's/.*://')"
docker exec -i "$CONTAINER" psql -v ON_ERROR_STOP=1 -U postgres -d service <<'SQL'
CREATE ROLE service_owner LOGIN PASSWORD 'fixture-owner';
CREATE ROLE service_runtime LOGIN PASSWORD 'fixture-runtime';
GRANT CREATE ON SCHEMA public TO service_owner;
SQL
export MIGRATION_DATABASE_URL="postgresql://service_owner:fixture-owner@127.0.0.1:$PORT/service"
export DATABASE_URL="postgresql://service_runtime:fixture-runtime@127.0.0.1:$PORT/service"
export TENANT_ID=33333333-3333-4333-8333-333333333333
export ENCRYPTION_KEY_HEX=1111111111111111111111111111111111111111111111111111111111111111
export ENCRYPTION_KEY_ID=fixture
export STORAGE_ENCRYPTION_NAMESPACE=fixture-records
export ENCRYPTION_LEGACY_KEY_HEX=disabled
"$RUN_ROOT/records-api" --migrate
"$RUN_ROOT/records-api" --migrate
docker exec -i "$CONTAINER" psql -v ON_ERROR_STOP=1 -U postgres -d service <<'SQL'
GRANT SELECT,INSERT,UPDATE,DELETE ON records TO service_runtime;
SQL
cargo test -p records-api -- --ignored --test-threads=1
node "$SCRIPT_DIR/rust_mcp_service_startup.mjs" "$RUN_ROOT/records-api"
if DATABASE_URL="postgresql://postgres:fixture-admin@127.0.0.1:$PORT/service" "$RUN_ROOT/records-api" > "$RUN_ROOT/rejected.log" 2>&1; then
  echo 'Expected privileged runtime rejection'; exit 1
fi
if grep -q fixture-admin "$RUN_ROOT/rejected.log"; then echo 'Connection error leaked credentials'; exit 1; fi
# Re-initialization preserves custom storage and its explicit dependency mode.
printf '\n// user storage marker\n' >> src/records-api/persistence/repository.rs
"$CLI" init service records-api --path . --language rust -D Records
grep -q 'user storage marker' src/records-api/persistence/repository.rs
"$CLI" change service --path src/records-api -N renamed-records --confirm
cargo check -p renamed-records --all-targets
"$CLI" delete service renamed-records --path . --continue
test ! -e src/renamed-records
node -e 'const m=require("./src/memory-api/.forklaunch/compliance.json"); if(!m)throw Error("missing no-storage metadata")' 2>/dev/null || cargo check -p memory-api
echo 'PASS explicit PostgreSQL service generation, summary-only router and SDKs, memory listing, migrations, restricted role, encrypted tenant-bound repository, audited disclosure, HTTP/MCP and lifecycle'
