#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FORKLAUNCH_CLI="${FORKLAUNCH_CLI:-$REPO_ROOT/cli/target/debug/forklaunch}"
TSC_BIN="${TSC_BIN:-$REPO_ROOT/framework/node_modules/.bin/tsc}"
mkdir -p "$SCRIPT_DIR/output/rust-http-methods-runtime"
RUN_ROOT="$(mktemp -d "$SCRIPT_DIR/output/rust-http-methods-runtime/run.XXXXXX")"
"$FORKLAUNCH_CLI" init application methods --path "$RUN_ROOT/app" --language mixed --modules iam-base cac-base messaging-base -D Methods -A ForkLaunch -L MIT
if "$FORKLAUNCH_CLI" init router invalid --path "$RUN_ROOT/app" --language rust; then
    echo 'FAIL app root must not be accepted as a service'; exit 1
fi
test ! -e "$RUN_ROOT/app/src/routes/invalid.rs"
for module in iam cac messaging; do
    "$FORKLAUNCH_CLI" init router custom --path "$RUN_ROOT/app/crates/$module" --language rust
done
cp -R "$RUN_ROOT/app" "$RUN_ROOT/relocated-app"
cd "$RUN_ROOT/relocated-app"
python3 - <<'PY'
from pathlib import Path
methods=['Get','Post','Put','Patch','Delete','Head','Options','Trace','Checkout','Copy','Link','Lock','MSearch','Merge','MkActivity','MkCol','Move','Notify','PropFind','PropPatch','Purge','Report','Search','Subscribe','Unlink','Unlock','Unsubscribe']
p=Path('crates/cac/src/routes/custom.rs')
p.write_text('''use forklaunch_axum::ForkLaunchRouter;
use forklaunch_contract::{RouteContract, HttpMethod};
pub const ROUTE_NAME: &str = "custom";
pub fn contract() -> RouteContract { RouteContract::get("custom", "custom", "/custom").with_response(204, None) }
pub fn all_contracts() -> Vec<RouteContract> { vec![
'''+''.join(f'RouteContract::new("Probe {method}", "HTTP method", HttpMethod::{method}, "/verbs/{method.lower()}").with_response(204, None),\n' for method in methods)+'''] }
pub fn contracts() -> Vec<RouteContract> { let mut contracts=all_contracts(); contracts.push(contract()); contracts }
pub fn register(mut router: ForkLaunchRouter) -> ForkLaunchRouter {
    router = router.get(contract(), || async { axum::http::StatusCode::NO_CONTENT });
    for contract in all_contracts() {
        let method = contract.method;
        router = router.route(contract, move |request: axum::extract::Request| async move {
            assert_eq!(request.method().as_str(), method.as_str());
            axum::http::StatusCode::NO_CONTENT
        });
    }
    router
}
''')
PY
cargo build --workspace
"$FORKLAUNCH_CLI" openapi export --path .
node "$SCRIPT_DIR/rust_http_methods_parity.mjs" "$PWD/.forklaunch/contracts/cac/contract.json"
for source in contract openapi; do
    "$FORKLAUNCH_CLI" sdk generate --path . --target all --from "$source"
    "$TSC_BIN" --strict --target ES2022 --module ESNext --moduleResolution Bundler --lib ES2022,DOM --outDir sdks/typescript/dist sdks/typescript/src/index.ts
    cargo check --workspace
    mkdir -p sdks/rust/cac-sdk/tests
    python3 - <<'PY'
from pathlib import Path
import re
p=Path('sdks/rust/cac-sdk/src/lib.rs');s=p.read_text()
operations=re.findall(r'pub async fn (verbs_\w+)\(\s*&self,\s*(?:input|request): (\w+)',s)
assert len(operations)==27,operations
Path('sdks/rust/cac-sdk/tests/methods.rs').write_text('use cac_sdk::*;\n#[tokio::test]\nasync fn all_methods() { let client = CacClient::new(std::env::var("METHODS_BASE_URL").unwrap());\n'+''.join(f'assert!(matches!(client.{method}({request}::default()).await.unwrap(), {request.removesuffix("Request")}Responses::Status204(())));\n' for method,request in operations)+'}\n')
p=Path('sdks/rust/cac-sdk/Cargo.toml');s=p.read_text()
if '[dev-dependencies]' not in s:p.write_text(s+'\n[dev-dependencies]\ntokio = {version="1",features=["macros","rt-multi-thread"]}\n')
PY
    TARGET_DIR="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys;print(json.load(sys.stdin)["target_directory"])')"
    node "$SCRIPT_DIR/rust_http_methods_runtime.mjs" "$TARGET_DIR/debug/cac" "$PWD/sdks/typescript/dist/index.js"
done
# The same extension hooks must survive all module lifecycle commands.
for module in iam cac messaging; do
    "$FORKLAUNCH_CLI" change router --path "crates/$module" -e custom -N extra --confirm
    "$FORKLAUNCH_CLI" sync service "$module" --path .
done
cargo check --workspace
"$FORKLAUNCH_CLI" openapi export --path .
python3 - <<'PY'
from pathlib import Path
import json
for module in ['iam','cac','messaging']:
    contract=json.loads(Path(f'.forklaunch/contracts/{module}/contract.json').read_text())
    assert any(route['path']=='/extra' for route in contract['routes']),module
    assert not any(route['path']=='/custom' for route in contract['routes']),module
PY
for module in iam cac messaging; do
    "$FORKLAUNCH_CLI" delete router extra --path "crates/$module" --continue
done
cargo check --workspace
"$FORKLAUNCH_CLI" openapi export --path .
python3 - <<'PY'
from pathlib import Path
import json
for module in ['iam','cac','messaging']:
    contract=json.loads(Path(f'.forklaunch/contracts/{module}/contract.json').read_text())
    assert not any(route['path']=='/extra' for route in contract['routes']),module
PY
# Older customized modules must fail before creating unusable route files.
python3 - <<'PY'
from pathlib import Path
p=Path('crates/messaging/src/main.rs');s=p.read_text().replace('messaging::routes::register_router(', '(');p.write_text(s)
PY
if "$FORKLAUNCH_CLI" init router unsupported --path crates/messaging --language rust; then
    echo 'FAIL old module without hooks must reject router generation'; exit 1
fi
test ! -e crates/messaging/src/routes/unsupported.rs
echo 'PASS all 27 HTTP methods, both SDK sources and builtin module router lifecycle'
