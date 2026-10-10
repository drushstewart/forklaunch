#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CLI_MANIFEST="$(cd "$SCRIPT_DIR/.." && pwd)/Cargo.toml"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
TSC_BIN="${TSC_BIN:-$REPO_ROOT/framework/node_modules/.bin/tsc}"

compile_typescript_sdk() {
    local sdk_entry="$1"

    if [ ! -x "$TSC_BIN" ]; then
        echo "[ERROR] TypeScript compiler not found at $TSC_BIN"
        echo "[ERROR] Run pnpm install before running generated TypeScript SDK tests."
        exit 1
    fi

    case "$sdk_entry" in
        */sdks/typescript/src/index.ts)
            "$TSC_BIN" --project "$(dirname "$(dirname "$sdk_entry")")/tsconfig.json"
            ;;
        *)
            "$TSC_BIN" --noEmit --target ES2022 --module ESNext \
                --moduleResolution Bundler --lib ES2022,DOM "$sdk_entry"
            ;;
    esac
}

run_cli() {
    if [ -n "${FORKLAUNCH_CLI:-}" ]; then
        "$FORKLAUNCH_CLI" "$@"
        return
    fi
    RUST_BACKTRACE=1 cargo run --manifest-path "$CLI_MANIFEST" --release -- "$@"
}

echo "[TEST] Rust OpenAPI and SDK commands"

rm -rf output/rust-sdk-openapi-commands
mkdir -p output/rust-sdk-openapi-commands
cd output/rust-sdk-openapi-commands

run_cli init application rust-contracts \
    --path rust-contracts \
    --language rust \
    -D "Rust contracts app" \
    -A "ForkLaunch" \
    -L MIT

run_cli init service accounts \
    --path rust-contracts \
    --language rust \
    -d postgresql \
    -D "Accounts service"

run_cli init service billing \
    --path rust-contracts \
    --language rust \
    -d sqlite \
    -D "Billing service"

run_cli init worker email-worker \
    --path rust-contracts \
    --language rust \
    -t redis \
    -D "Email worker"

run_cli init router users \
    --path rust-contracts/crates/accounts \
    --language rust

run_cli init router invoices \
    --path rust-contracts/crates/billing \
    --language rust

run_cli openapi export --path rust-contracts
test -f rust-contracts/.forklaunch/openapi/accounts/openapi.json
test -f rust-contracts/.forklaunch/openapi/billing/openapi.json
test -f rust-contracts/.forklaunch/openapi/email-worker/openapi.json
test -f rust-contracts/.forklaunch/contracts/accounts/contract.json
test -f rust-contracts/.forklaunch/contracts/billing/contract.json
test -f rust-contracts/.forklaunch/contracts/email-worker/contract.json
grep -q '"openapi"[[:space:]]*:[[:space:]]*"3.1.0"' rust-contracts/.forklaunch/openapi/accounts/openapi.json
grep -q 'listUsers' rust-contracts/.forklaunch/openapi/accounts/openapi.json
grep -q 'listInvoices' rust-contracts/.forklaunch/openapi/billing/openapi.json
grep -q '"formatVersion"[[:space:]]*:[[:space:]]*"forklaunch.contract.v1"' rust-contracts/.forklaunch/contracts/accounts/contract.json
grep -q '"generator"[[:space:]]*:[[:space:]]*"framework-rs"' rust-contracts/.forklaunch/contracts/accounts/contract.json
grep -q '"routes"' rust-contracts/.forklaunch/contracts/accounts/contract.json
grep -q 'List Users' rust-contracts/.forklaunch/contracts/accounts/contract.json

run_cli openapi export --path rust-contracts --output generated/openapi
test -f rust-contracts/generated/openapi/accounts/openapi.json
test -f rust-contracts/generated/openapi/billing/openapi.json

run_cli sdk generate all --path rust-contracts --from openapi
test -f rust-contracts/sdks/typescript/src/index.ts
test -f rust-contracts/sdks/rust/accounts-sdk/src/lib.rs
test -f rust-contracts/sdks/rust/billing-sdk/src/lib.rs
test -f rust-contracts/sdks/rust/email-worker-sdk/src/lib.rs
grep -q 'createForkLaunchSdk' rust-contracts/sdks/typescript/src/index.ts
grep -q 'accountsService' rust-contracts/sdks/typescript/src/index.ts
grep -q 'billingService' rust-contracts/sdks/typescript/src/index.ts
grep -q 'emailWorkerService' rust-contracts/sdks/typescript/src/index.ts
grep -q 'users' rust-contracts/sdks/typescript/src/index.ts
grep -q 'invoices' rust-contracts/sdks/typescript/src/index.ts
grep -q 'AccountsUsersListUsersRequest' rust-contracts/sdks/typescript/src/index.ts
grep -q 'AccountsUsersListUsersResponses' rust-contracts/sdks/typescript/src/index.ts
grep -q 'records: Array<{ id: string; name: string }>' rust-contracts/sdks/typescript/src/index.ts
grep -q 'listUsers: (input: AccountsUsersListUsersRequest' rust-contracts/sdks/typescript/src/index.ts
grep -q 'ForkLaunchSdkResponse<Responses' rust-contracts/sdks/typescript/src/index.ts
grep -q "Code extends keyof ForkLaunchErrorResponses ? 'text/plain' : 'application/json'" rust-contracts/sdks/typescript/src/index.ts
grep -q 'AccountsClient' rust-contracts/sdks/rust/accounts-sdk/src/lib.rs
grep -q 'BillingClient' rust-contracts/sdks/rust/billing-sdk/src/lib.rs
grep -q 'EmailWorkerClient' rust-contracts/sdks/rust/email-worker-sdk/src/lib.rs
! grep -q 'pub async fn' rust-contracts/sdks/rust/email-worker-sdk/src/lib.rs
grep -q 'reqwest = { version = "0.13.2", features = \["json", "query"\] }' rust-contracts/sdks/rust/accounts-sdk/Cargo.toml
grep -q 'serde = { version = "1.0", features = \["derive"\] }' rust-contracts/sdks/rust/accounts-sdk/Cargo.toml
grep -q 'AccountsUsersListUsersRequest' rust-contracts/sdks/rust/accounts-sdk/src/lib.rs
grep -q 'AccountsUsersListUsersResponses' rust-contracts/sdks/rust/accounts-sdk/src/lib.rs
grep -q 'pub struct AccountsUsersListUsersResponse' rust-contracts/sdks/rust/accounts-sdk/src/lib.rs
grep -q 'pub records: Vec<AccountsUsersListUsersResponseRecordsItem>' rust-contracts/sdks/rust/accounts-sdk/src/lib.rs
grep -q "fn encode_path_segment" rust-contracts/sdks/rust/accounts-sdk/src/lib.rs
grep -q 'users_list_users_request' rust-contracts/sdks/rust/accounts-sdk/src/lib.rs
grep -q 'pub async fn users_list_users' rust-contracts/sdks/rust/accounts-sdk/src/lib.rs
grep -q 'pub enum SdkAuth' rust-contracts/sdks/rust/accounts-sdk/src/lib.rs
grep -q 'pub fn with_jwt' rust-contracts/sdks/rust/accounts-sdk/src/lib.rs
grep -q 'pub fn with_basic_auth' rust-contracts/sdks/rust/accounts-sdk/src/lib.rs
grep -q 'pub fn with_hmac_authorization' rust-contracts/sdks/rust/accounts-sdk/src/lib.rs
grep -q 'pub fn with_auth_hook' rust-contracts/sdks/rust/accounts-sdk/src/lib.rs
grep -q 'response.json::<AccountsUsersListUsersResponse>()' rust-contracts/sdks/rust/accounts-sdk/src/lib.rs
grep -q '"sdks/rust/accounts-sdk"' rust-contracts/Cargo.toml
grep -q '"sdks/rust/billing-sdk"' rust-contracts/Cargo.toml
grep -q '"sdks/rust/email-worker-sdk"' rust-contracts/Cargo.toml
compile_typescript_sdk rust-contracts/sdks/typescript/src/index.ts
"$TSC_BIN" --project rust-contracts/sdks/typescript/tsconfig.json --noEmit false --outDir "$PWD/rust-contracts/sdks/typescript/dist"
node "$SCRIPT_DIR/fixtures/typescript_sdk_paths.mjs" "$PWD/rust-contracts/sdks/typescript/dist/index.js"
cat > rust-contracts/generated-sdk-usage.ts << 'EOF'
import { accountsService, createForkLaunchSdk, sdk, type ForkLaunchGeneratedSdk } from './sdks/typescript/src/index';

const configured = createForkLaunchSdk({
    baseUrl: 'http://localhost:8000',
    headers: { 'x-test': 'true' }
});
const typedSdk: ForkLaunchGeneratedSdk = configured;
const abort = new AbortController();

configured.accounts.users.listUsers({ signal: abort.signal });
configured.accounts.users.listUsers({ credentials: 'include' });
typedSdk.billing.invoices.listInvoices({ cache: 'no-store' });
void sdk.emailWorker; // Durable workers expose no application HTTP methods.
accountsService.users.listUsers({ redirect: 'follow' });
EOF
compile_typescript_sdk rust-contracts/generated-sdk-usage.ts
cat > rust-contracts/generated-sdk-live-compat.ts << 'EOF'
import {
    createForkLaunchSdk,
    type AccountsUsersListUsersRequest,
    type AccountsUsersListUsersResponses,
    type ForkLaunchErrorResponses
} from './sdks/typescript/src/index';

type LiveSdkResponse<Responses extends Record<number, unknown>> = {
    [Code in keyof Responses & number]: {
        code: Code;
        contentType: Code extends keyof ForkLaunchErrorResponses ? 'text/plain' : 'application/json';
        response: Responses[Code];
    };
}[keyof Responses & number];

type LiveSdkFunction<Request, Responses extends Record<number, unknown>> =
    (input: Request) => Promise<LiveSdkResponse<Responses>>;

type LiveAccountsSdkShape = {
    accounts: {
        users: {
            listUsers: LiveSdkFunction<AccountsUsersListUsersRequest, AccountsUsersListUsersResponses>;
        };
    };
};

const generatedAsLiveShape: LiveAccountsSdkShape = createForkLaunchSdk();

async function acceptsLiveShape(client: LiveAccountsSdkShape) {
    const response = await client.accounts.users.listUsers({});
    if (response.code === 400) {
        const text: string = response.response;
        const contentType: 'text/plain' = response.contentType;
        return `${contentType}:${text}`;
    }
    return response.response;
}

acceptsLiveShape(generatedAsLiveShape);
EOF
compile_typescript_sdk rust-contracts/generated-sdk-live-compat.ts
cat > rust-contracts/generated-sdk-invalid-usage.ts << 'EOF'
import { sdk } from './sdks/typescript/src/index';

sdk.emailWorker.health.get({});
sdk.accounts.users.missing({});
sdk.accounts.users.listUsers({ body: { unexpected: true } });
EOF
if compile_typescript_sdk rust-contracts/generated-sdk-invalid-usage.ts; then
    echo "[ERROR] Generated TypeScript SDK should reject removed routes and invalid request bodies"
    exit 1
fi
run_cli sdk generate --path rust-contracts --target all --from openapi --check
# OpenAPI generation must read the artifact, independently of the manifest and
# framework contract. An edited operation/schema must invalidate --check.
cp rust-contracts/.forklaunch/openapi/accounts/openapi.json rust-contracts/accounts-openapi.backup.json
python3 - <<'PYTHON'
from pathlib import Path
import json
p = Path('rust-contracts/.forklaunch/openapi/accounts/openapi.json')
spec = json.loads(p.read_text())
spec['paths']['/users']['get']['operationId'] = 'scanUsers'
spec['components']['schemas']['UsersResponse']['properties']['displayName'] = {'type': 'string'}
p.write_text(json.dumps(spec))
PYTHON
if run_cli sdk generate --path rust-contracts --target all --from openapi --check; then
    echo '[ERROR] OpenAPI changes must invalidate generated SDKs' >&2
    exit 1
fi
run_cli sdk generate --path rust-contracts --target all --from openapi
grep -q 'scanUsers:' rust-contracts/sdks/typescript/src/index.ts
grep -q 'displayName?: string' rust-contracts/sdks/typescript/src/index.ts
grep -q 'pub display_name: Option<String>' rust-contracts/sdks/rust/accounts-sdk/src/lib.rs
grep -q 'serde(rename = "displayName")' rust-contracts/sdks/rust/accounts-sdk/src/lib.rs
mv rust-contracts/.forklaunch/openapi/accounts/openapi.json rust-contracts/accounts-openapi.missing.json
if run_cli sdk generate --path rust-contracts --target all --from openapi; then
    echo '[ERROR] Missing OpenAPI must fail rather than fabricate manifest routes' >&2
    exit 1
fi
mv rust-contracts/accounts-openapi.backup.json rust-contracts/.forklaunch/openapi/accounts/openapi.json
run_cli sdk generate --path rust-contracts --target all --from openapi
if run_cli sdk generate rust --path rust-contracts --target typescript --from openapi; then
    echo "[ERROR] SDK target shorthand should conflict with a different explicit --target"
    exit 1
fi

run_cli sdk generate ts --path rust-contracts --service accounts --from contract
grep -q 'accountsService' rust-contracts/sdks/typescript/src/index.ts
grep -q 'users:' rust-contracts/sdks/typescript/src/index.ts
grep -q 'listUsers: (input: AccountsUsersListUsersRequest' rust-contracts/sdks/typescript/src/index.ts
! grep -q 'billingService' rust-contracts/sdks/typescript/src/index.ts
! grep -q 'emailWorkerService' rust-contracts/sdks/typescript/src/index.ts
compile_typescript_sdk rust-contracts/sdks/typescript/src/index.ts
run_cli sdk generate --path rust-contracts --target typescript --service accounts --from contract --check
printf '\n// stale sdk marker\n' >> rust-contracts/sdks/typescript/src/index.ts
if run_cli sdk generate --path rust-contracts --target typescript --service accounts --from contract --check; then
    echo "[ERROR] SDK check should fail when generated TypeScript SDK is stale"
    exit 1
fi
run_cli sdk watch --path rust-contracts --target typescript --service accounts --from contract --once --interval-ms 100
run_cli sdk generate --path rust-contracts --target typescript --service accounts --from contract --check

run_cli sdk generate rust --path rust-contracts --service billing --from contract
test -f rust-contracts/sdks/rust/billing-sdk/src/lib.rs
grep -q 'BillingClient' rust-contracts/sdks/rust/billing-sdk/src/lib.rs
grep -q 'invoices_list_invoices_request' rust-contracts/sdks/rust/billing-sdk/src/lib.rs
grep -q 'pub async fn invoices_list_invoices' rust-contracts/sdks/rust/billing-sdk/src/lib.rs
run_cli sdk generate --path rust-contracts --target rust --service billing --from contract --check

run_cli change service --path rust-contracts/crates/billing -N invoicing --confirm
test ! -d rust-contracts/sdks/rust/billing-sdk
test ! -d rust-contracts/.forklaunch/contracts/billing
test -f rust-contracts/sdks/rust/invoicing-sdk/src/lib.rs
grep -q 'InvoicingClient' rust-contracts/sdks/rust/invoicing-sdk/src/lib.rs
grep -q 'invoicingService' rust-contracts/sdks/typescript/src/index.ts
! grep -q 'billingService' rust-contracts/sdks/typescript/src/index.ts
run_cli sdk generate --path rust-contracts --target rust --service invoicing --from contract --check
run_cli sdk generate --path rust-contracts --target typescript --from contract --check
compile_typescript_sdk rust-contracts/sdks/typescript/src/index.ts

cat "$SCRIPT_DIR/fixtures/rust_sdk_paths.rs" >> rust-contracts/sdks/rust/accounts-sdk/src/lib.rs
(
    cd rust-contracts
    cargo test --workspace
)

echo "[SUCCESS] Rust OpenAPI and SDK command tests passed"
