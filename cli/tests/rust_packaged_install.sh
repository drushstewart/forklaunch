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
cargo install --path "$SOURCE_ROOT" --locked "${INSTALL_FLAGS[@]}" --root "$RUN_ROOT/installed" --target-dir "$INSTALL_TARGET"
CLI="$RUN_ROOT/installed/bin/forklaunch"
"$CLI" init application archive-app --path "$RUN_ROOT/app" --language rust -D 'Packaged CLI' -A ForkLaunch -L MIT
"$CLI" init service accounts --path "$RUN_ROOT/app" --language rust -d postgresql -D Accounts
"$CLI" init worker postgres-jobs --path "$RUN_ROOT/app" --language rust -t database -d postgresql -D Jobs
"$CLI" init worker redis-jobs --path "$RUN_ROOT/app" --language rust -t redis -D Jobs
"$CLI" init worker kafka-jobs --path "$RUN_ROOT/app" --language rust -t kafka -D Jobs
"$CLI" init module coding --path "$RUN_ROOT/app" --language rust -m cac-base
"$CLI" init module messaging --path "$RUN_ROOT/app" --language rust -m messaging-base
"$CLI" init module texting --path "$RUN_ROOT/app" --language rust -m messaging-twilio
mv "$RUN_ROOT/app" "$RUN_ROOT/relocated-app"
cd "$RUN_ROOT/relocated-app"
python3 - "$REPO_ROOT" <<'PYTHON'
from pathlib import Path
import sys
for workspace in ['framework-rs','blueprint-rs']:
    root=Path('.forklaunch')/workspace
    assert root.joinpath('Cargo.toml').is_file()
    if workspace == 'framework-rs':
        assert root.joinpath('crates/forklaunch-infrastructure-s3/src/aws.rs').is_file(), 'Packaged async S3 adapter missing'
        assert root.joinpath('crates/forklaunch-infrastructure-s3/src/aws_post.rs').is_file(), 'Packaged S3 POST signer missing'
    for path in root.rglob('*'):
        assert not path.is_symlink(), f'Packaged source retained a symlink: {path}'
        if path.is_file():
            assert sys.argv[1].encode() not in path.read_bytes(), f'Checkout path leaked: {path}'
PYTHON
export CARGO_TARGET_DIR="$GENERATED_TARGET"
cargo check --workspace
cargo check --manifest-path .forklaunch/framework-rs/Cargo.toml -p forklaunch-infrastructure-s3 --features aws-sdk
cargo check --manifest-path .forklaunch/blueprint-rs/Cargo.toml -p forklaunch-cac-axum
cargo check --manifest-path .forklaunch/blueprint-rs/Cargo.toml -p forklaunch-messaging-axum
"$CLI" openapi export --path .
"$CLI" sdk generate --path . --target all --from contract
"$CLI" sdk generate --path . --target all --from contract --check
cargo check --workspace
# Empty application contracts must also work in consumers that deny warnings.
for sdk in postgres-jobs-sdk redis-jobs-sdk kafka-jobs-sdk; do
    cargo rustc -p "$sdk" --lib -- -D warnings
done
python3 - <<'PYTHON'
from pathlib import Path
import json
for name in ['accounts','postgres-jobs','redis-jobs','kafka-jobs','coding','messaging','texting']:
    contract=json.loads((Path('.forklaunch/contracts')/name/'contract.json').read_text())
    assert contract['generator']=='framework-rs'
    spec=json.loads((Path('.forklaunch/openapi')/name/'openapi.json').read_text())
    assert spec['openapi'].startswith('3.')
PYTHON
echo 'PASS packaged CLI installation, portable service/worker generation, real compiler checks, and contract/SDK exports'
