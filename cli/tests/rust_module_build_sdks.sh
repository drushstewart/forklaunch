#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
if [ "${FORKLAUNCH_WORKER_RUNTIME_LOCKED:-}" != 1 ]; then
  exec python3 "$SCRIPT_DIR/fixtures/worker_runtime_lock.py" "${BASH_SOURCE[0]}" "$@"
fi
CLI="${FORKLAUNCH_CLI:-$REPO_ROOT/cli/target/debug/forklaunch}"
RUN_ROOT="${FORKLAUNCH_MODULE_SDK_ROOT:-$(mktemp -d "${TMPDIR:-/tmp}/module-build-sdks.XXXXXX")}"
echo "Module SDK workspace: $RUN_ROOT"
if [ ! -f "$RUN_ROOT/app/Cargo.toml" ]; then
  "$CLI" init application module-sdks --path "$RUN_ROOT/app" --language rust -D Modules -A ForkLaunch -L MIT
fi
for module in iam-base cac-base ecommerce-stripe billing-base billing-stripe; do
  "$CLI" init module "$module" --path "$RUN_ROOT/app" --language rust -m "$module"
done
cd "$RUN_ROOT/app"
"$CLI" init router custom --path src/cac-base --language rust
cargo check --workspace
node --input-type=module <<'JS'
import fs from 'node:fs';
for(const name of ['iam-base','cac-base','ecommerce-stripe','billing-base','billing-stripe']) {
 const root=`src/client-sdk/typescript/${name}`;
 const artifact=JSON.parse(fs.readFileSync(`${root}/contract.json`));
 if(!artifact.routes.length)throw Error(`Missing ${name} routes`);
 for(const path of [`${root}/index.ts`,`${root}/openapi.json`,`src/client-sdk/rust/${name}.rs`])if(!fs.statSync(path).size)throw Error(`Empty ${path}`);
}
const artifact=JSON.parse(fs.readFileSync('src/client-sdk/typescript/cac-base/contract.json'));
if(!artifact.routes.some(r=>r.path==='/custom'))throw Error('Missing shared extension');
JS
"$CLI" change router --path src/cac-base -e custom -N renamed --confirm
cargo check -p cac-base
node -e 'const a=require("./src/client-sdk/typescript/cac-base/contract.json");if(!a.routes.some(r=>r.path==="/renamed")||a.routes.some(r=>r.path==="/custom"))throw Error("Stale renamed contract")'
"$CLI" delete router renamed --path src/cac-base --continue
cargo check -p cac-base
node -e 'const a=require("./src/client-sdk/typescript/cac-base/contract.json");if(a.routes.some(r=>r.path==="/renamed"))throw Error("Stale removed contract")'
"${FORKLAUNCH_TEST_TSC:-$REPO_ROOT/framework/node_modules/.bin/tsc}" --noEmit --strict --skipLibCheck --target es2022 --module esnext --moduleResolution bundler src/client-sdk/typescript/*/index.ts
mkdir -p src/sdk-check
cat > src/sdk-check/Cargo.toml <<'TOML'
[package]
name = "sdk-check"
version = "0.0.0"
edition = "2024"
[workspace]
[lib]
path = "lib.rs"
[dependencies]
forklaunch-core = { path = "../../vendor/framework/crates/core", features = ["schema-validation"] }
forklaunch-universal-sdk = { path = "../../vendor/framework/crates/universal-sdk" }
reqwest = { version = "0.13.2", features = ["json", "query", "multipart"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
multer = "3.1"
futures-util = "0.3"
TOML
python3 - <<'PYTHON'
from pathlib import Path
files=sorted(Path('src/client-sdk/rust').glob('*.rs'))
assert len(files)==8
Path('src/sdk-check/lib.rs').write_text(''.join(f'#[path = "../client-sdk/rust/{p.name}"]\npub mod {p.stem.replace("-","_")};\n' for p in files))
PYTHON
cargo check --manifest-path src/sdk-check/Cargo.toml
echo 'PASS all eight modules emit contract/OpenAPI/TS/Rust on compilation with strict TS/Rust SDK compilation and shared extension rename/delete updates outputs'
rm -rf "$RUN_ROOT"
