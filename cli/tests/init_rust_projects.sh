#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
TSC_BIN="$REPO_ROOT/framework/node_modules/.bin/tsc"

compile_typescript_sdk() {
    local sdk_entry="$1"

    if [ ! -x "$TSC_BIN" ]; then
        echo "[ERROR] TypeScript compiler not found at $TSC_BIN"
        echo "[ERROR] Run pnpm install before running generated TypeScript SDK tests."
        exit 1
    fi

    "$TSC_BIN" \
        --noEmit \
        --target ES2022 \
        --module ESNext \
        --moduleResolution Bundler \
        --lib ES2022,DOM \
        "$sdk_entry"
}

echo "Processing Rust project generation..."

test -f "$REPO_ROOT/cli/src/templates/rust/application/Cargo.toml"
test -f "$REPO_ROOT/cli/src/templates/rust/application/Dockerfile"
test -f "$REPO_ROOT/cli/src/templates/rust/project/service/src/lib.rs"
test -f "$REPO_ROOT/cli/src/templates/rust/project/worker/src/lib.rs"
test -f "$REPO_ROOT/cli/src/templates/rust/project/library/src/lib.rs"
test -f "$REPO_ROOT/cli/src/templates/rust/router/router.rs"
test -f "$REPO_ROOT/cli/src/templates/sdk/typescript/index.ts"
test -f "$REPO_ROOT/cli/src/templates/sdk/rust/lib.rs"
test -L "$REPO_ROOT/cli/src/templates/rust/blueprint/iam-base"
test -L "$REPO_ROOT/cli/src/templates/rust/blueprint/billing-base"
test -L "$REPO_ROOT/cli/src/templates/rust/blueprint/billing-stripe"

if [ -d "output/init-rust-projects" ]; then
    rm -rf output/init-rust-projects
fi

mkdir -p output/init-rust-projects
cd output/init-rust-projects

RUST_BACKTRACE=1 cargo run --release init application rustproject \
    --path ./rustproject \
    --language rust \
    -D "Rust app" \
    -A "ForkLaunch" \
    -L MIT

test -f rustproject/Cargo.toml
test -f rustproject/Dockerfile
test -f rustproject/docker-compose.yaml
test -f rustproject/package.json
test -f rustproject/pnpm-workspace.yaml
test -f rustproject/.forklaunch/manifest.toml
test -f rustproject/rustfmt.toml
test -f rustproject/clippy.toml
test -f rustproject/sdks/typescript/package.json
test -f rustproject/sdks/typescript/src/index.ts
test -d rustproject/crates
grep -q 'language = "rust"' rustproject/.forklaunch/manifest.toml
grep -q 'sdkTargets = \["typescript", "rust"\]' rustproject/.forklaunch/manifest.toml
grep -q 'sdk:generate' rustproject/package.json
grep -q 'sdk:check' rustproject/package.json
grep -q 'sdk:typecheck' rustproject/package.json
grep -q 'sdk:watch' rustproject/package.json
grep -q 'rust:build' rustproject/package.json
grep -q 'rust:test' rustproject/package.json
grep -q 'rustproject-network' rustproject/docker-compose.yaml

RUST_BACKTRACE=1 cargo run --release init service accounts \
    --path ./rustproject \
    --language rust \
    -d postgresql \
    -D "Accounts service"

RUST_BACKTRACE=1 cargo run --release init service notifications \
    --path ./rustproject \
    --language rust \
    -d postgresql \
    -D "Notifications service"

RUST_BACKTRACE=1 cargo run --release init worker email-worker \
    --path ./rustproject \
    --language rust \
    -t database \
    -d postgresql \
    -D "Email worker"

RUST_BACKTRACE=1 cargo run --release init library shared-kernel \
    --path ./rustproject \
    --language rust \
    -D "Shared kernel"

RUST_BACKTRACE=1 cargo run --release init router users \
    --path ./rustproject/crates/accounts \
    --language rust

test -f rustproject/crates/accounts/Cargo.toml
test -f rustproject/crates/accounts/src/lib.rs
test -f rustproject/crates/accounts/src/routes/users.rs
test -f rustproject/crates/notifications/src/routes/mod.rs
test -f rustproject/crates/email-worker/src/jobs/mod.rs
test -f rustproject/crates/shared-kernel/src/lib.rs
grep -q '"crates/accounts"' rustproject/Cargo.toml
grep -q '"crates/notifications"' rustproject/Cargo.toml
grep -q '"crates/email-worker"' rustproject/Cargo.toml
grep -q '"crates/shared-kernel"' rustproject/Cargo.toml
grep -q 'accounts:' rustproject/docker-compose.yaml
grep -q 'notifications:' rustproject/docker-compose.yaml
grep -q 'email-worker:' rustproject/docker-compose.yaml
grep -q './target/release/accounts' rustproject/docker-compose.yaml
grep -q './target/release/email-worker' rustproject/docker-compose.yaml
grep -q '8000:8000' rustproject/docker-compose.yaml
grep -q '8001:8001' rustproject/docker-compose.yaml
! grep -q 'shared-kernel:' rustproject/docker-compose.yaml
grep -q 'dev:accounts' rustproject/package.json
grep -q 'dev:email-worker' rustproject/package.json
grep -q 'build:shared-kernel' rustproject/package.json
grep -q 'shared-kernel' rustproject/crates/accounts/Cargo.toml
grep -q '../shared-kernel' rustproject/crates/accounts/Cargo.toml
grep -q 'shared-kernel' rustproject/crates/notifications/Cargo.toml
grep -q '../shared-kernel' rustproject/crates/notifications/Cargo.toml
grep -q 'shared-kernel' rustproject/crates/email-worker/Cargo.toml
grep -q '../shared-kernel' rustproject/crates/email-worker/Cargo.toml
grep -q 'crateName = "accounts"' rustproject/.forklaunch/manifest.toml
grep -q 'cratePath = "crates/accounts"' rustproject/.forklaunch/manifest.toml
grep -q 'users' rustproject/.forklaunch/manifest.toml

(
    cd rustproject
    cargo test --workspace
)

RUST_BACKTRACE=1 cargo run --release change service \
    --path ./rustproject/crates/accounts \
    -D "Accounts service v2" \
    -d sqlite \
    --confirm

RUST_BACKTRACE=1 cargo run --release change service \
    --path ./rustproject/crates/notifications \
    --to worker \
    -t redis \
    --confirm

test -f rustproject/crates/notifications/src/jobs/mod.rs
grep -q 'name = "notifications"' rustproject/.forklaunch/manifest.toml
grep -q 'type = "Worker"' rustproject/.forklaunch/manifest.toml
grep -q 'workerType = "redis"' rustproject/.forklaunch/manifest.toml

RUST_BACKTRACE=1 cargo run --release change worker \
    --path ./rustproject/crates/notifications \
    --to service \
    --confirm

test -f rustproject/crates/notifications/src/routes/mod.rs

RUST_BACKTRACE=1 cargo run --release change worker \
    --path ./rustproject/crates/email-worker \
    -D "Email worker v2" \
    -t redis \
    --confirm

RUST_BACKTRACE=1 cargo run --release change router \
    --path ./rustproject/crates/accounts \
    -e users \
    -N customers \
    --confirm

RUST_BACKTRACE=1 cargo run --release change library \
    --path ./rustproject/crates/shared-kernel \
    -N common-kernel \
    -D "Common kernel" \
    --confirm

grep -q 'description = "Accounts service v2"' rustproject/.forklaunch/manifest.toml
grep -q 'database = "sqlite"' rustproject/.forklaunch/manifest.toml
grep -q 'description = "Email worker v2"' rustproject/.forklaunch/manifest.toml
test ! -f rustproject/crates/accounts/src/routes/users.rs
test -f rustproject/crates/accounts/src/routes/customers.rs
test ! -d rustproject/crates/shared-kernel
test -d rustproject/crates/common-kernel
grep -q '"crates/common-kernel"' rustproject/Cargo.toml
grep -q 'accounts:' rustproject/docker-compose.yaml
grep -q 'notifications:' rustproject/docker-compose.yaml
grep -q 'email-worker:' rustproject/docker-compose.yaml
! grep -q 'common-kernel:' rustproject/docker-compose.yaml
! grep -q 'build:shared-kernel' rustproject/package.json
grep -q 'build:common-kernel' rustproject/package.json
! grep -q 'shared-kernel' rustproject/crates/accounts/Cargo.toml
grep -q 'common-kernel' rustproject/crates/accounts/Cargo.toml
grep -q '../common-kernel' rustproject/crates/accounts/Cargo.toml
! grep -q 'shared-kernel' rustproject/crates/email-worker/Cargo.toml
grep -q 'common-kernel' rustproject/crates/email-worker/Cargo.toml
grep -q '../common-kernel' rustproject/crates/email-worker/Cargo.toml

RUST_BACKTRACE=1 cargo run --release sync service accounts --path ./rustproject
RUST_BACKTRACE=1 cargo run --release sync service notifications --path ./rustproject
RUST_BACKTRACE=1 cargo run --release sync library common-kernel --path ./rustproject
RUST_BACKTRACE=1 cargo run --release sync worker email-worker --path ./rustproject
RUST_BACKTRACE=1 cargo run --release sync all --path ./rustproject --confirm

(
    cd rustproject
    cargo test --workspace
)

RUST_BACKTRACE=1 cargo run --release openapi export --path ./rustproject

test -f rustproject/.forklaunch/openapi/accounts/openapi.json
test -f rustproject/.forklaunch/contracts/accounts/contract.json
grep -q '"openapi"[[:space:]]*:[[:space:]]*"3.1.0"' rustproject/.forklaunch/openapi/accounts/openapi.json
grep -q 'listCustomers' rustproject/.forklaunch/openapi/accounts/openapi.json
grep -q '"formatVersion"[[:space:]]*:[[:space:]]*"forklaunch.contract.v1"' rustproject/.forklaunch/contracts/accounts/contract.json
grep -q '"generator"[[:space:]]*:[[:space:]]*"framework-rs"' rustproject/.forklaunch/contracts/accounts/contract.json
grep -q 'List Customers' rustproject/.forklaunch/contracts/accounts/contract.json

RUST_BACKTRACE=1 cargo run --release sdk generate --path ./rustproject --target all

test -f rustproject/sdks/typescript/src/index.ts
test -f rustproject/sdks/rust/accounts-sdk/src/lib.rs
grep -q 'accountsService' rustproject/sdks/typescript/src/index.ts
grep -q 'customers' rustproject/sdks/typescript/src/index.ts
grep -q 'AccountsCustomersListRequest' rustproject/sdks/typescript/src/index.ts
grep -q 'list: (input: AccountsCustomersListRequest' rustproject/sdks/typescript/src/index.ts
grep -q 'AccountsClient' rustproject/sdks/rust/accounts-sdk/src/lib.rs
grep -q 'AccountsCustomersListRequest' rustproject/sdks/rust/accounts-sdk/src/lib.rs
grep -q 'customers_list_request' rustproject/sdks/rust/accounts-sdk/src/lib.rs
grep -q '"sdks/rust/accounts-sdk"' rustproject/Cargo.toml
compile_typescript_sdk rustproject/sdks/typescript/src/index.ts

RUST_BACKTRACE=1 cargo run --release sdk generate --path ./rustproject --target typescript --service accounts
grep -q 'accountsService' rustproject/sdks/typescript/src/index.ts
grep -q 'customers:' rustproject/sdks/typescript/src/index.ts
grep -q 'list: (input: AccountsCustomersListRequest' rustproject/sdks/typescript/src/index.ts
! grep -q 'emailWorkerService' rustproject/sdks/typescript/src/index.ts
compile_typescript_sdk rustproject/sdks/typescript/src/index.ts

RUST_BACKTRACE=1 cargo run --release sdk generate --path ./rustproject --target rust --service email-worker
test -f rustproject/sdks/rust/email-worker-sdk/src/lib.rs
grep -q 'EmailWorkerClient' rustproject/sdks/rust/email-worker-sdk/src/lib.rs
grep -q '"sdks/rust/email-worker-sdk"' rustproject/Cargo.toml

(
    cd rustproject
    cargo test --workspace
)

RUST_BACKTRACE=1 cargo run --release delete router customers \
    --path ./rustproject/crates/accounts \
    --continue
! grep -q 'AccountsCustomersListRequest' rustproject/sdks/typescript/src/index.ts

RUST_BACKTRACE=1 cargo run --release delete worker email-worker \
    --path ./rustproject \
    --continue

RUST_BACKTRACE=1 cargo run --release delete service notifications \
    --path ./rustproject \
    --continue

RUST_BACKTRACE=1 cargo run --release delete library common-kernel \
    --path ./rustproject \
    --continue

test ! -f rustproject/crates/accounts/src/routes/customers.rs
! grep -q 'customers' rustproject/.forklaunch/manifest.toml
test ! -d rustproject/crates/email-worker
test ! -d rustproject/sdks/rust/email-worker-sdk
test ! -d rustproject/crates/notifications
test ! -d rustproject/sdks/rust/notifications-sdk
test ! -d rustproject/crates/common-kernel
! grep -q 'email-worker:' rustproject/docker-compose.yaml
! grep -q 'notifications:' rustproject/docker-compose.yaml
! grep -q 'common-kernel:' rustproject/docker-compose.yaml
! grep -q 'dev:email-worker' rustproject/package.json
! grep -q 'dev:notifications' rustproject/package.json
! grep -q 'build:common-kernel' rustproject/package.json
! grep -q 'common-kernel' rustproject/crates/accounts/Cargo.toml
! grep -q '"crates/email-worker"' rustproject/Cargo.toml
! grep -q '"sdks/rust/email-worker-sdk"' rustproject/Cargo.toml
! grep -q '"crates/notifications"' rustproject/Cargo.toml
! grep -q '"sdks/rust/notifications-sdk"' rustproject/Cargo.toml
! grep -q '"crates/common-kernel"' rustproject/Cargo.toml
! grep -q 'name = "email-worker"' rustproject/.forklaunch/manifest.toml
! grep -q 'name = "notifications"' rustproject/.forklaunch/manifest.toml
! grep -q 'name = "common-kernel"' rustproject/.forklaunch/manifest.toml
! grep -q 'emailWorkerService' rustproject/sdks/typescript/src/index.ts
! grep -q 'notificationsService' rustproject/sdks/typescript/src/index.ts

(
    cd rustproject
    cargo test --workspace
)

RUST_BACKTRACE=1 cargo run --release delete service accounts \
    --path ./rustproject \
    --continue

test ! -d rustproject/crates/accounts
test ! -d rustproject/sdks/rust/accounts-sdk
! grep -q 'accounts:' rustproject/docker-compose.yaml
! grep -q 'dev:accounts' rustproject/package.json
! grep -q '"crates/accounts"' rustproject/Cargo.toml
! grep -q '"sdks/rust/accounts-sdk"' rustproject/Cargo.toml
! grep -q 'name = "accounts"' rustproject/.forklaunch/manifest.toml
! grep -q 'accountsService' rustproject/sdks/typescript/src/index.ts

if grep -q '"crates/' rustproject/Cargo.toml || grep -q '"sdks/rust/' rustproject/Cargo.toml; then
    (
        cd rustproject
        cargo test --workspace
    )
else
    grep -q 'members = \[' rustproject/Cargo.toml
fi

RUST_BACKTRACE=1 cargo run --release init application mixedproject \
    --path ./mixedproject \
    --language mixed \
    -D "Mixed app" \
    -A "ForkLaunch" \
    -L MIT

test -f mixedproject/Cargo.toml
test -f mixedproject/package.json
test -f mixedproject/pnpm-workspace.yaml
test -f mixedproject/sdks/typescript/src/index.ts
grep -q 'language = "mixed"' mixedproject/.forklaunch/manifest.toml

RUST_BACKTRACE=1 cargo run --release init application dryrunrust \
    --path ./dryrunrust \
    --language rust \
    --dryrun \
    -D "Dry run app" \
    -A "ForkLaunch" \
    -L MIT

test ! -d dryrunrust

echo "Rust project generation tests passed"
