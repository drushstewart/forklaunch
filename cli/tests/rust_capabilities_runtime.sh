#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/cli/target}"
export CARGO_INCREMENTAL=0
export FORKLAUNCH_SKIP_FORMAT=1
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-stable}"
if [ "${FORKLAUNCH_WORKER_RUNTIME_LOCKED:-}" != 1 ]; then
    exec python3 "$SCRIPT_DIR/fixtures/worker_runtime_lock.py" "${BASH_SOURCE[0]}" "$@"
fi
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FORKLAUNCH_CLI="${FORKLAUNCH_CLI:-$REPO_ROOT/cli/target/debug/forklaunch}"
mkdir -p "$SCRIPT_DIR/output/rust-capabilities-runtime"
RUN_ROOT="$(mktemp -d "$SCRIPT_DIR/output/rust-capabilities-runtime/run.XXXXXX")"
trap 'rm -rf "$RUN_ROOT"' EXIT
# Capability edits are stateless: besides `openapi export` output, the
# manifest is all .forklaunch/ holds.
manifest_only() { python3 -c 'import pathlib,sys;s=sorted(str(p) for p in pathlib.Path(".forklaunch").rglob("*") if p.is_file() and not p.is_relative_to(".forklaunch/openapi"));sys.exit(0 if s==[".forklaunch/manifest.toml"] else f"unexpected .forklaunch state: {s}")'; }
"$FORKLAUNCH_CLI" init application capability-app --path "$RUN_ROOT/app" --database postgresql --validator zod --formatter prettier --linter eslint --http-framework express --runtime node --test-framework vitest -D Capabilities -A ForkLaunch -L MIT
"$FORKLAUNCH_CLI" init service messages --path "$RUN_ROOT/app" --language rust -D Messages
cp -R "$RUN_ROOT/app" "$RUN_ROOT/relocated-app"
cd "$RUN_ROOT/relocated-app"
cp src/messages/Cargo.toml "$RUN_ROOT/original-cargo.toml"
cp src/messages/server.rs "$RUN_ROOT/original-main.rs"
cp src/messages/lib.rs "$RUN_ROOT/original-lib.rs"
cp src/messages/.forklaunch/contracts/lib.rs "$RUN_ROOT/original-registry.rs"
cp src/messages/api/routes/mod.rs "$RUN_ROOT/original-routes.rs"
"$FORKLAUNCH_CLI" infra add messages email --path . --dryrun
test ! -e src/messages/capabilities/mod.rs
manifest_only
for feature in email sms whatsapp voice payments; do
    "$FORKLAUNCH_CLI" infra add messages "$feature" --path .
done
manifest_only
mkdir -p src/messages/examples
cat > src/messages/examples/capabilities.rs <<'RUST'
use messages::capabilities::*;
fn main() {
    if std::env::args().any(|arg|arg=="--unmanaged") {
        assert!(!sms_client().unwrap().managed());
        assert!(email_client().is_err());
        assert!(whatsapp_client().is_err());
        assert!(voice_client().is_err());
        assert!(payments_client().is_err());
        assert!(stripe_client().is_err());
    } else {
        assert!(sms_client().unwrap().managed());
        email_client().unwrap();whatsapp_client().unwrap();voice_client().unwrap();
        payments_client().unwrap();stripe_client().unwrap();
    }
}
RUST
cargo build --workspace
env -u PLATFORM_GATEWAY_URL -u INSTANCE_ID -u INSTANCE_HMAC_KEY cargo run -p messages --example capabilities -- --unmanaged
PLATFORM_GATEWAY_URL=http://127.0.0.1:18088 INSTANCE_ID=local-instance INSTANCE_HMAC_KEY=local-dev-instance-key cargo run -p messages --example capabilities
"$FORKLAUNCH_CLI" openapi export --path .
"$FORKLAUNCH_CLI" sdk generate --path . --target all --from contract
"$FORKLAUNCH_CLI" sdk generate --path . --target all --from contract --check
cargo check --workspace
TARGET_DIR="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
node "$SCRIPT_DIR/rust_capabilities_runtime.mjs" "$TARGET_DIR/debug/messages"
rm src/messages/examples/capabilities.rs
python3 - <<'PY'
from pathlib import Path
p=Path('src/messages/capabilities/nonce_store.rs')
p.write_text(p.read_text().replace('MemoryPlatformEventNonces::default()', 'MemoryPlatformEventNonces::new(2).expect("fixture capacity")'))
PY
printf '\n// Human main customization\n' >> src/messages/server.rs
printf '\n// Human main customization\n' >> "$RUN_ROOT/original-main.rs"
printf '\npub const CONFIG_MARKER: &str = "preserved";\n' >> src/messages/config.rs
"$FORKLAUNCH_CLI" sync service messages --path .
"$FORKLAUNCH_CLI" change service --path src/messages -N renamed-messages --confirm
manifest_only
grep -q 'http://renamed-messages:8000' docker-compose.yaml
grep -q 'Human main customization' src/renamed-messages/server.rs
grep -q CONFIG_MARKER src/renamed-messages/config.rs
cargo test -p renamed-messages
cargo build -p renamed-messages
node "$SCRIPT_DIR/rust_capabilities_runtime.mjs" "$TARGET_DIR/debug/renamed-messages" --bounded-store
"$FORKLAUNCH_CLI" change service --path src/renamed-messages -N messages --confirm
# Removal must preserve edits made between stacked capability commands.
printf '\n[package.metadata.user]\nmarker = "preserved"\n' >> src/messages/Cargo.toml
printf '\npub const USER_MARKER: &str = "preserved";\n' >> src/messages/lib.rs
printf '\n// Human handler customization\n' >> src/messages/capabilities/email.rs
printf '\n// Human nonce store customization\n' >> src/messages/capabilities/nonce_store.rs
printf '\nCUSTOM_SETTING=preserved\n' >> src/messages/.env.local
"$FORKLAUNCH_CLI" infra remove messages voice --path .
"$FORKLAUNCH_CLI" infra add messages voice --path .
for feature in sms payments email voice whatsapp; do
    "$FORKLAUNCH_CLI" infra remove messages "$feature" --path .
done
manifest_only
test ! -e src/messages/capabilities/mod.rs
test -e src/messages/capabilities/email.rs
grep -q 'Human handler customization' src/messages/capabilities/email.rs
grep -q 'Human nonce store customization' src/messages/capabilities/nonce_store.rs
grep -q 'CUSTOM_SETTING=preserved' src/messages/.env.local
grep -q USER_MARKER src/messages/lib.rs
grep -q 'marker = "preserved"' src/messages/Cargo.toml
cmp "$RUN_ROOT/original-main.rs" src/messages/server.rs
cmp "$RUN_ROOT/original-registry.rs" src/messages/.forklaunch/contracts/lib.rs
cmp "$RUN_ROOT/original-routes.rs" src/messages/api/routes/mod.rs
test ! -e src/messages/api/controllers/platform_events.rs
if grep -q 'instance-gateway\|forklaunch-billing-stripe' src/messages/Cargo.toml; then
    echo 'FAIL capability-owned dependencies remained'; exit 1
fi
cargo check --workspace
# Unmodified sources restore byte for byte, including the original Cargo formatting.
"$FORKLAUNCH_CLI" init service pristine --path . --language rust -D Pristine
cp src/pristine/Cargo.toml "$RUN_ROOT/pristine-cargo.toml"
cp src/pristine/lib.rs "$RUN_ROOT/pristine-lib.rs"
cp src/pristine/server.rs "$RUN_ROOT/pristine-main.rs"
for feature in payments sms; do "$FORKLAUNCH_CLI" infra add pristine "$feature" --path .; done
for feature in payments sms; do "$FORKLAUNCH_CLI" infra remove pristine "$feature" --path .; done
cmp "$RUN_ROOT/pristine-cargo.toml" src/pristine/Cargo.toml
cmp "$RUN_ROOT/pristine-lib.rs" src/pristine/lib.rs
cmp "$RUN_ROOT/pristine-main.rs" src/pristine/server.rs
test ! -e src/pristine/capabilities/nonce_store.rs
"$FORKLAUNCH_CLI" infra add pristine email --path .
python3 - <<'PY'
from pathlib import Path
p=Path('src/pristine/Cargo.toml')
s=p.read_text();assert s.count('"instance-gateway"')==1
p.write_text(s.replace('"instance-gateway"','"instance-gateway", "stripe-webhook"'))
PY
"$FORKLAUNCH_CLI" infra add pristine sms --path .
"$FORKLAUNCH_CLI" infra remove pristine email --path .
"$FORKLAUNCH_CLI" infra remove pristine sms --path .
test ! -e src/pristine/capabilities/mod.rs
manifest_only
grep -q stripe-webhook src/pristine/Cargo.toml
cargo check -p pristine
mkdir -p services/team
mv src/pristine services/team/pristine
python3 - <<'PY'
from pathlib import Path
for filename in ['Cargo.toml','.forklaunch/manifest.toml']:
 p=Path(filename);p.write_text(p.read_text().replace('src/pristine','services/team/pristine'))
import re, os
for relative in ['Cargo.toml','.forklaunch/contracts/Cargo.toml','sdk/rust/Cargo.toml']:
 p=Path('services/team/pristine')/relative
 def relocate(match):
  previous=Path(os.path.normpath(Path('src/pristine')/Path(relative).parent/match[1]))
  if previous.is_relative_to('src/pristine'): return match[0]
  return 'path = "'+os.path.relpath(previous,p.parent)+'"'
 p.write_text(re.sub(r'path\s*=\s*"([^"]+)"', relocate, p.read_text()))
PY
"$FORKLAUNCH_CLI" infra add pristine sms --path .
"$FORKLAUNCH_CLI" sync service pristine --path .
"$FORKLAUNCH_CLI" change service --path services/team/pristine -N relocated-pristine --confirm
test -e services/team/relocated-pristine/capabilities/mod.rs
test ! -d src/relocated-pristine
cargo test -p relocated-pristine
"$FORKLAUNCH_CLI" delete service relocated-pristine --path . --continue
manifest_only
if grep -q 'gateway-mock:' docker-compose.yaml; then echo 'FAIL deleted service retained the gateway mock'; exit 1; fi
echo 'PASS native capability generation, relocation, HTTP/MCP startup, SDKs, dry run and source/configuration preservation'
