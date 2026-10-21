#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
export CARGO_INCREMENTAL=0
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/cli/target}"
if [[ "${FORKLAUNCH_WORKER_RUNTIME_LOCKED:-}" != 1 ]]; then
    exec python3 "$SCRIPT_DIR/fixtures/worker_runtime_lock.py" "$0"
fi
fixture_container="forklaunch-postgres-admission-$$"
trap 'docker rm -f "$fixture_container" >/dev/null 2>&1 || true' EXIT
# No production data or credentials; bind the disposable trust fixture only to loopback.
docker run -d --name "$fixture_container" -p 127.0.0.1::5432 \
    -e POSTGRES_HOST_AUTH_METHOD=trust -e POSTGRES_DB=forklaunch_admission_proof \
    postgres:17-alpine >/dev/null
for attempt in $(seq 1 60); do
    if docker exec "$fixture_container" pg_isready -U postgres >/dev/null 2>&1; then break; fi
    sleep 1
done
fixture_port="$(docker port "$fixture_container" 5432/tcp | sed 's/.*://')"
export FORKLAUNCH_ADMISSION_DATABASE_URL="postgres://postgres@127.0.0.1:$fixture_port/forklaunch_admission_proof"
for driver_feature in postgres postgres,postgres09; do
    cargo +stable run --manifest-path "$REPO_ROOT/framework-rs/Cargo.toml" \
        -p forklaunch-core --features "$driver_feature" --example postgres_admission
    if [[ "$driver_feature" == postgres ]]; then
        docker exec "$fixture_container" dropdb -U postgres forklaunch_admission_proof
        docker exec "$fixture_container" psql -U postgres -v ON_ERROR_STOP=1 -c 'DROP ROLE unsafe_app, safe_app, bypass_group' >/dev/null
        docker exec "$fixture_container" createdb -U postgres forklaunch_admission_proof
    fi
done
