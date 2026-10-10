#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FORKLAUNCH_CLI="${FORKLAUNCH_CLI:-${FORKLAUNCH_TEST_CLI:-}}"
TSC_BIN="${TSC_BIN:-$REPO_ROOT/framework/node_modules/.bin/tsc}"
run_cli() {
    if [ -n "$FORKLAUNCH_CLI" ]; then "$FORKLAUNCH_CLI" "$@";
    else cargo run --manifest-path "$REPO_ROOT/cli/Cargo.toml" --release -- "$@"; fi
}
mkdir -p "$SCRIPT_DIR/output/rust-cac-runtime"
RUN_ROOT="$(mktemp -d "$SCRIPT_DIR/output/rust-cac-runtime/run.XXXXXX")"
run_cli init application dry-app --path "$RUN_ROOT/dry-app" --language rust --modules cac-base -D Dry -A ForkLaunch -L MIT --dryrun
test ! -e "$RUN_ROOT/dry-app"
run_cli init application bundled-app --path "$RUN_ROOT/bundled-app" --language rust --modules cac-base -D Bundled -A ForkLaunch -L MIT
test -f "$RUN_ROOT/bundled-app/crates/cac/src/main.rs"
cargo metadata --manifest-path "$RUN_ROOT/bundled-app/Cargo.toml" --no-deps --format-version 1 >/dev/null
run_cli init application module-app --path "$RUN_ROOT/app" --language mixed -D CAC -A ForkLaunch -L MIT
run_cli init module dry-cac --path "$RUN_ROOT/app" --language rust -m cac-base --dryrun
test ! -d "$RUN_ROOT/app/crates/dry-cac"
# An existing IAM install exercises embedded workspace upgrades as well as coexistence.
run_cli init module iam --path "$RUN_ROOT/app" --language rust -m iam-base -d postgresql
python3 - "$RUN_ROOT/app/.forklaunch/blueprint-rs/Cargo.toml" <<'PYTHON'
from pathlib import Path
import sys
p=Path(sys.argv[1]); s=p.read_text(); s='\n'.join(line for line in s.splitlines() if '/cac/' not in line and 'forklaunch-cac-interface' not in line)
p.write_text(s+'\n# user-workspace-marker\n')
PYTHON
run_cli init module --path "$RUN_ROOT/app" --language rust -m cac-base
cp -R "$RUN_ROOT/app" "$RUN_ROOT/relocated-app"
cd "$RUN_ROOT/relocated-app"
grep -q user-workspace-marker .forklaunch/blueprint-rs/Cargo.toml
grep -q forklaunch-cac-axum .forklaunch/blueprint-rs/Cargo.toml
printf '\npub const USER_MODULE_MARKER: &str = "preserved";\n' >> crates/cac/src/lib.rs
printf '\n[package.metadata.user]\nmarker = "preserved"\n' >> crates/cac/Cargo.toml
run_cli sync service cac --path .
run_cli init module cac --path . --language rust -m cac-base
if run_cli init service cac --path . --language rust -d postgresql -D Invalid; then
    echo 'FAIL generic initialization must not convert a CAC module'; exit 1
fi
grep -q USER_MODULE_MARKER crates/cac/src/lib.rs
grep -q CAC_HMAC_SECRET_KEY docker-compose.yaml
cargo build --workspace
OTEL_EXPORTER_OTLP_ENDPOINT=invalid OTEL_EXPORTER_OTLP_PROTOCOL=invalid run_cli openapi export --path .
run_cli sdk generate --path . --target all --from contract
"$TSC_BIN" --strict --target ES2022 --module ESNext --moduleResolution Bundler --lib ES2022,DOM --outDir sdks/typescript/dist sdks/typescript/src/index.ts
cargo check --workspace
mkdir -p sdks/rust/cac-sdk/tests
cp "$SCRIPT_DIR/fixtures/rust_cac_sdk_runtime.rs" sdks/rust/cac-sdk/tests/runtime.rs
cat >> sdks/rust/cac-sdk/Cargo.toml <<'TOML'

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
TOML
python3 - <<'PYTHON'
import json
from pathlib import Path
routes=json.loads(Path('.forklaunch/contracts/cac/contract.json').read_text())['routes']
assert len(routes)==5
assert {route['path'] for route in routes}=={'/codeSet','/codeSet/:code','/compliance/export/:userId','/compliance/erase/:userId','/health'}
PYTHON
BINARY="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"]+"/debug/cac")')"
node "$SCRIPT_DIR/rust_cac_runtime.mjs" "$BINARY" "$PWD/sdks/typescript/dist/index.js"
run_cli change service --path crates/cac -N coding --confirm
grep -q USER_MODULE_MARKER crates/coding/src/lib.rs
grep -q 'marker = "preserved"' crates/coding/Cargo.toml
grep -q 'variant = "cac-base"' .forklaunch/manifest.toml
run_cli sync service coding --path .
run_cli sdk generate --path . --target all --from contract --check
cargo check --workspace
run_cli delete service coding --path . --continue
test ! -d crates/coding
test ! -d sdks/rust/coding-sdk
test ! -d .forklaunch/contracts/coding
echo 'PASS generated CAC mixed app, bundled init, upgrade, source preservation, offline artifacts, SDKs, HMAC/JWKS/RBAC runtime, rename, sync and delete'
