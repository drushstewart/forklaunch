#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
RUN_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/forklaunch-packaged-install.XXXXXX")"
trap 'rm -rf "$RUN_ROOT"' EXIT
INSTALL_TARGET="${FORKLAUNCH_PACKAGE_INSTALL_TARGET:-$REPO_ROOT/cli/target}"
GENERATED_TARGET="${FORKLAUNCH_PACKAGE_GENERATED_TARGET:-$INSTALL_TARGET}"
INSTALL_FLAGS=()
if [ "${FORKLAUNCH_PACKAGE_PROFILE:-release}" = debug ]; then INSTALL_FLAGS+=(--debug); fi

CARGO_TARGET_DIR="$INSTALL_TARGET" cargo package --manifest-path "$REPO_ROOT/cli/Cargo.toml" --allow-dirty --no-verify
python3 - "$INSTALL_TARGET" "$RUN_ROOT" <<'PYTHON'
from pathlib import Path
import sys,tarfile
archives=list((Path(sys.argv[1])/'package').glob('forklaunch-*.crate'))
assert archives, 'CLI archive missing'
archive=max(archives,key=lambda path:path.stat().st_mtime_ns)
with tarfile.open(archive) as package:
    package.extractall(Path(sys.argv[2])/'source',filter='data')
PYTHON
SOURCE_ROOT="$(find "$RUN_ROOT/source" -mindepth 1 -maxdepth 1 -type d)"
test -f "$SOURCE_ROOT/src/templates/sdk/typescript/safeStringify.ts"
test -f "$SOURCE_ROOT/src/templates/sdk/typescript/sse.ts"
test -f "$SOURCE_ROOT/src/templates/sdk/rust/sse_client.rs"
test -f "$SOURCE_ROOT/src/templates/sdk/rust/sse_decoder.rs"
cargo install --path "$SOURCE_ROOT" --locked "${INSTALL_FLAGS[@]}" --root "$RUN_ROOT/installed" --target-dir "$INSTALL_TARGET"
CLI="$RUN_ROOT/installed/bin/forklaunch"
"$CLI" init application archive-app --path "$RUN_ROOT/app" --language rust -D 'Packaged CLI' -A ForkLaunch -L MIT
"$CLI" init service accounts --path "$RUN_ROOT/app" --language rust -d postgresql -D Accounts
"$CLI" init worker postgres-jobs --path "$RUN_ROOT/app" --language rust -t database -d postgresql -D Jobs
"$CLI" init worker redis-jobs --path "$RUN_ROOT/app" --language rust -t redis -D Jobs
"$CLI" init worker bullmq-jobs --path "$RUN_ROOT/app" --language rust -t bullmq -D Jobs
"$CLI" init worker kafka-jobs --path "$RUN_ROOT/app" --language rust -t kafka -D Jobs
"$CLI" init module coding --path "$RUN_ROOT/app" --language rust -m cac-base
"$CLI" init module messaging --path "$RUN_ROOT/app" --language rust -m messaging-base
"$CLI" init module texting --path "$RUN_ROOT/app" --language rust -m messaging-twilio
"$CLI" init module ecommerce --path "$RUN_ROOT/app" --language rust -m ecommerce-stripe
"$CLI" init module billing --path "$RUN_ROOT/app" --language rust -m billing-base
"$CLI" init module payments --path "$RUN_ROOT/app" --language rust -m billing-stripe
mv "$RUN_ROOT/app" "$RUN_ROOT/relocated-app"
cd "$RUN_ROOT/relocated-app"
for feature in email sms whatsapp voice payments; do
    "$CLI" infra add accounts "$feature" --path .
done
for resource in redis s3; do
    "$CLI" infra add accounts "$resource" --path .
done
cp "$REPO_ROOT/cli/tests/fixtures/rust_recursive_service.rs" crates/accounts/src/recursive_fixture.rs
python3 - <<'PYTHON'
from pathlib import Path
p=Path('crates/accounts/Cargo.toml')
p.write_text(p.read_text().replace('[dependencies]\n','[dependencies]\nserde_json = "1"\n'))
p=Path('crates/accounts/src/lib.rs')
s='pub mod recursive_fixture;\n'+p.read_text()
s=s.replace('for contract in routes::contracts() {','for contract in routes::contracts().into_iter().chain(recursive_fixture::contracts()) {')
p.write_text(s)
p=Path('crates/accounts/src/main.rs')
s=p.read_text().replace('router(state)', 'accounts::recursive_fixture::register(router(state))')
p.write_text(s)
PYTHON
python3 - "$REPO_ROOT" <<'PYTHON'
from pathlib import Path
import sys
for workspace in ['framework-rs','blueprint-rs']:
    root=Path('.forklaunch')/workspace
    assert root.joinpath('Cargo.toml').is_file()
    if workspace == 'framework-rs':
        assert root.joinpath('crates/forklaunch-infrastructure-s3/src/aws.rs').is_file(), 'Packaged async S3 adapter missing'
        assert root.joinpath('crates/forklaunch-infrastructure-s3/src/aws_post.rs').is_file(), 'Packaged S3 POST signer missing'
        assert root.joinpath('crates/forklaunch-contract/src/schema.rs').is_file(), 'Packaged recursive schema rebasing missing'
        assert root.joinpath('crates/forklaunch-mcp/src/server.rs').is_file(), 'Packaged MCP runtime missing'
        assert root.joinpath('crates/forklaunch-mcp/src/server/manual.rs').is_file(), 'Packaged manual MCP runtime missing'
        assert root.joinpath('crates/forklaunch-mcp/src/server/multipart.rs').is_file(), 'Packaged multipart MCP runtime missing'
        assert root.joinpath('crates/forklaunch-axum/src/mcp_runtime.rs').is_file(), 'Packaged separate MCP listeners missing'
        assert root.joinpath('crates/forklaunch-ws/src/transport.rs').is_file(), 'Packaged typed WebSocket runtime missing'
        assert root.joinpath('crates/forklaunch-ws/tests/transport.rs').is_file(), 'Packaged WebSocket transport tests missing'
        assert root.joinpath('crates/forklaunch-ws/examples/websocket_runtime.rs').is_file(), 'Packaged WebSocket interoperability fixture missing'
    else:
        assert root.joinpath('crates/implementations/worker/forklaunch-worker-redis/src/bullmq.rs').is_file(), 'Packaged BullMQ adapter missing'
    for path in root.rglob('*'):
        assert not path.is_symlink(), f'Packaged source retained a symlink: {path}'
        if path.is_file():
            assert sys.argv[1].encode() not in path.read_bytes(), f'Checkout path leaked: {path}'
PYTHON
export CARGO_TARGET_DIR="$GENERATED_TARGET"
cargo check --workspace
cargo test --manifest-path .forklaunch/framework-rs/Cargo.toml -p forklaunch-ws --all-features
cargo check --manifest-path .forklaunch/framework-rs/Cargo.toml -p forklaunch-infrastructure-s3 --features aws-sdk
cargo check --manifest-path .forklaunch/blueprint-rs/Cargo.toml -p forklaunch-billing-base -p forklaunch-billing-stripe -p forklaunch-billing-axum --features forklaunch-billing-base/postgres
cargo check --manifest-path .forklaunch/blueprint-rs/Cargo.toml -p forklaunch-cac-axum
cargo check --manifest-path .forklaunch/blueprint-rs/Cargo.toml -p forklaunch-messaging-axum
cargo check --manifest-path .forklaunch/blueprint-rs/Cargo.toml -p forklaunch-ecommerce-base -p forklaunch-ecommerce-stripe -p forklaunch-ecommerce-paypal -p forklaunch-ecommerce-axum --all-features
cargo build -p accounts
node "$REPO_ROOT/cli/tests/rust_mcp_service_startup.mjs" "$CARGO_TARGET_DIR/debug/accounts"
TSC_BIN="${TSC_BIN:-$REPO_ROOT/framework/node_modules/.bin/tsc}" node "$REPO_ROOT/cli/tests/rust_capabilities_runtime.mjs" "$CARGO_TARGET_DIR/debug/accounts"
"$CLI" openapi export --path .
for artifact in contract openapi; do
    "$CLI" sdk generate --path . --target all --from "$artifact"
    "$CLI" sdk generate --path . --target all --from "$artifact" --check
    "${TSC_BIN:-$REPO_ROOT/framework/node_modules/.bin/tsc}" --project sdks/typescript/tsconfig.json
    cargo check --workspace
done
# Empty application contracts must also work in consumers that deny warnings.
for sdk in postgres-jobs-sdk redis-jobs-sdk kafka-jobs-sdk bullmq-jobs-sdk; do
    cargo rustc -p "$sdk" --lib -- -D warnings
done
python3 - <<'PYTHON'
from pathlib import Path
import json
for name in ['accounts','postgres-jobs','redis-jobs','kafka-jobs','bullmq-jobs','coding','messaging','texting','ecommerce']:
    contract=json.loads((Path('.forklaunch/contracts')/name/'contract.json').read_text())
    assert contract['generator']=='framework-rs'
    spec=json.loads((Path('.forklaunch/openapi')/name/'openapi.json').read_text())
    assert spec['openapi'].startswith('3.')
ts=Path('sdks/typescript/src/index.ts').read_text()
rust=Path('sdks/rust/accounts-sdk/src/lib.rs').read_text()
assert 'AccountsTreesReadTreeResponse' in ts and 'AccountsChoicesReadChoiceResponse' in ts
assert 'pub children: Vec<AccountsSchema' in rust
PYTHON
# Invoke the native generator from portable sources shipped by the installed CLI.
cargo run --manifest-path .forklaunch/framework-rs/Cargo.toml -p forklaunch-sdk-rust --example recursive_client -- \
    "$PWD/.forklaunch/contracts/accounts/contract.json" "$PWD/sdks/rust/accounts-sdk/src/lib.rs"
cargo clippy -p accounts-sdk --all-targets -- -D warnings
cargo run --manifest-path .forklaunch/framework-rs/Cargo.toml -p forklaunch-sdk-typescript --example recursive_client -- \
    "$PWD/.forklaunch/contracts/accounts/contract.json" "$PWD/sdks/typescript/src/index.ts"
"${TSC_BIN:-$REPO_ROOT/framework/node_modules/.bin/tsc}" --project sdks/typescript/tsconfig.json
echo 'PASS packaged CLI installation, portable services/workers, CLI/native SDK generation and HTTP/MCP surfaces'
