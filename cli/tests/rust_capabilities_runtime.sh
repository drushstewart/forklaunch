#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FORKLAUNCH_CLI="${FORKLAUNCH_CLI:-$REPO_ROOT/cli/target/debug/forklaunch}"
mkdir -p "$SCRIPT_DIR/output/rust-capabilities-runtime"
RUN_ROOT="$(mktemp -d "$SCRIPT_DIR/output/rust-capabilities-runtime/run.XXXXXX")"
"$FORKLAUNCH_CLI" init application capability-app --path "$RUN_ROOT/app" --language mixed -D Capabilities -A ForkLaunch -L MIT
"$FORKLAUNCH_CLI" init service messages --path "$RUN_ROOT/app" --language rust -d postgresql -D Messages
cp -R "$RUN_ROOT/app" "$RUN_ROOT/relocated-app"
cd "$RUN_ROOT/relocated-app"
cp crates/messages/Cargo.toml "$RUN_ROOT/original-cargo.toml"
cp crates/messages/src/main.rs "$RUN_ROOT/original-main.rs"
cp crates/messages/src/lib.rs "$RUN_ROOT/original-lib.rs"
"$FORKLAUNCH_CLI" infra add messages email --path . --dryrun
test ! -e crates/messages/src/capabilities/mod.rs
test ! -e .forklaunch/native-capabilities/messages.json
for feature in email sms whatsapp voice payments; do
    "$FORKLAUNCH_CLI" infra add messages "$feature" --path .
done
python3 - <<'PY'
from pathlib import Path
import os,stat
state=Path('.forklaunch/native-capabilities/messages.json')
assert '/*.json' in state.with_name('.gitignore').read_text().splitlines()
if os.name=='posix': assert stat.S_IMODE(state.stat().st_mode)==0o600
PY
mkdir -p crates/messages/examples
cat > crates/messages/examples/capabilities.rs <<'RUST'
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
"${TSC_BIN:-$REPO_ROOT/framework/node_modules/.bin/tsc}" --strict --target ES2022 --module ESNext --moduleResolution Bundler --lib ES2022,DOM --outDir sdks/typescript/dist sdks/typescript/src/index.ts
cargo check --workspace
TARGET_DIR="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
node "$SCRIPT_DIR/rust_capabilities_runtime.mjs" "$TARGET_DIR/debug/messages"
rm crates/messages/examples/capabilities.rs
python3 - <<'PY'
from pathlib import Path
p=Path('crates/messages/src/capabilities/nonce_store.rs')
p.write_text(p.read_text().replace('MemoryPlatformEventNonces::default()', 'MemoryPlatformEventNonces::new(2).expect("fixture capacity")'))
PY
printf '\n// Human main customization\n' >> crates/messages/src/main.rs
printf '\n// Human main customization\n' >> "$RUN_ROOT/original-main.rs"
printf '\npub const CONFIG_MARKER: &str = "preserved";\n' >> crates/messages/src/config/mod.rs
"$FORKLAUNCH_CLI" sync service messages --path .
"$FORKLAUNCH_CLI" change service --path crates/messages -N renamed-messages --confirm
test ! -e .forklaunch/native-capabilities/messages.json
test -e .forklaunch/native-capabilities/renamed-messages.json
grep -q 'http://renamed-messages:8000' docker-compose.yaml
grep -q 'Human main customization' crates/renamed-messages/src/main.rs
grep -q CONFIG_MARKER crates/renamed-messages/src/config/mod.rs
cargo test -p renamed-messages
cargo build -p renamed-messages
node "$SCRIPT_DIR/rust_capabilities_runtime.mjs" "$TARGET_DIR/debug/renamed-messages" --bounded-store
"$FORKLAUNCH_CLI" change service --path crates/renamed-messages -N messages --confirm
# Removal must preserve edits made between stacked capability commands.
printf '\n[package.metadata.user]\nmarker = "preserved"\n' >> crates/messages/Cargo.toml
printf '\npub const USER_MARKER: &str = "preserved";\n' >> crates/messages/src/lib.rs
printf '\n// Human handler customization\n' >> crates/messages/src/capabilities/email.rs
printf '\n// Human nonce store customization\n' >> crates/messages/src/capabilities/nonce_store.rs
printf '\nCUSTOM_SETTING=preserved\n' >> crates/messages/.env.local
"$FORKLAUNCH_CLI" infra remove messages voice --path .
"$FORKLAUNCH_CLI" infra add messages voice --path .
for feature in sms payments email voice whatsapp; do
    "$FORKLAUNCH_CLI" infra remove messages "$feature" --path .
done
test ! -e .forklaunch/native-capabilities/messages.json
test ! -e crates/messages/src/capabilities/mod.rs
test -e crates/messages/src/capabilities/email.rs
grep -q 'Human handler customization' crates/messages/src/capabilities/email.rs
grep -q 'Human nonce store customization' crates/messages/src/capabilities/nonce_store.rs
grep -q 'CUSTOM_SETTING=preserved' crates/messages/.env.local
grep -q USER_MARKER crates/messages/src/lib.rs
grep -q 'marker = "preserved"' crates/messages/Cargo.toml
cmp "$RUN_ROOT/original-main.rs" crates/messages/src/main.rs
if grep -q 'forklaunch-blueprint-core\|forklaunch-billing-stripe' crates/messages/Cargo.toml; then
    echo 'FAIL capability-owned dependencies remained'; exit 1
fi
cargo check --workspace
# Unmodified sources restore byte for byte, including the original Cargo formatting.
"$FORKLAUNCH_CLI" init service pristine --path . --language rust -d postgresql -D Pristine
cp crates/pristine/Cargo.toml "$RUN_ROOT/pristine-cargo.toml"
cp crates/pristine/src/lib.rs "$RUN_ROOT/pristine-lib.rs"
cp crates/pristine/src/main.rs "$RUN_ROOT/pristine-main.rs"
for feature in payments sms; do "$FORKLAUNCH_CLI" infra add pristine "$feature" --path .; done
for feature in payments sms; do "$FORKLAUNCH_CLI" infra remove pristine "$feature" --path .; done
cmp "$RUN_ROOT/pristine-cargo.toml" crates/pristine/Cargo.toml
cmp "$RUN_ROOT/pristine-lib.rs" crates/pristine/src/lib.rs
cmp "$RUN_ROOT/pristine-main.rs" crates/pristine/src/main.rs
test ! -e crates/pristine/src/capabilities/nonce_store.rs
"$FORKLAUNCH_CLI" infra add pristine email --path .
rm .forklaunch/native-capabilities/pristine.json
python3 - <<'PY'
from pathlib import Path
p=Path('crates/pristine/Cargo.toml')
s=p.read_text();assert 'features = ["instance-gateway"]' in s
p.write_text(s.replace('features = ["instance-gateway"]','features = ["instance-gateway", "stripe-webhook"]'))
PY
"$FORKLAUNCH_CLI" infra add pristine sms --path .
"$FORKLAUNCH_CLI" infra remove pristine email --path .
"$FORKLAUNCH_CLI" infra remove pristine sms --path .
test ! -e crates/pristine/src/capabilities/mod.rs
test ! -e .forklaunch/native-capabilities/pristine.json
grep -q stripe-webhook crates/pristine/Cargo.toml
cargo check -p pristine
mkdir -p services/team
mv crates/pristine services/team/pristine
python3 - <<'PY'
from pathlib import Path
for filename in ['Cargo.toml','.forklaunch/manifest.toml']:
 p=Path(filename);p.write_text(p.read_text().replace('crates/pristine','services/team/pristine'))
p=Path('services/team/pristine/Cargo.toml');p.write_text(p.read_text().replace('../../.forklaunch/','../../../.forklaunch/'))
PY
"$FORKLAUNCH_CLI" infra add pristine sms --path .
"$FORKLAUNCH_CLI" sync service pristine --path .
"$FORKLAUNCH_CLI" change service --path services/team/pristine -N relocated-pristine --confirm
test -e services/team/relocated-pristine/src/capabilities/mod.rs
test ! -d crates/relocated-pristine
cargo test -p relocated-pristine
"$FORKLAUNCH_CLI" delete service relocated-pristine --path . --continue
test ! -e .forklaunch/native-capabilities/relocated-pristine.json
if grep -q 'gateway-mock:' docker-compose.yaml; then echo 'FAIL deleted service retained the gateway mock'; exit 1; fi
echo 'PASS native capability generation, relocation, HTTP/MCP startup, SDKs, dry run and source/configuration preservation'
