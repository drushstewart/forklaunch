#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CLI_MANIFEST="$(cd "$SCRIPT_DIR/.." && pwd)/Cargo.toml"

run_cli() {
    if [ -n "${FORKLAUNCH_CLI:-}" ]; then
        "$FORKLAUNCH_CLI" "$@"
        return
    fi
    RUST_BACKTRACE=1 cargo run --manifest-path "$CLI_MANIFEST" --release -- "$@"
}

echo "[TEST] Rust init commands"

rm -rf output/rust-init-commands
mkdir -p output/rust-init-commands
cd output/rust-init-commands

run_cli init application rust-app \
    --path rust-app \
    --language rust \
    -D "Rust app" \
    -A "ForkLaunch" \
    -L MIT

test -f rust-app/Cargo.toml
test -f rust-app/.forklaunch/manifest.toml
test -f rust-app/README.md
test -d rust-app/crates
grep -q 'app_name = "rust-app"' rust-app/.forklaunch/manifest.toml
grep -q 'language = "rust"' rust-app/.forklaunch/manifest.toml
grep -q 'runtime = "tokio"' rust-app/.forklaunch/manifest.toml

mkdir -p current-path
(
    cd current-path
    run_cli init application rust-current \
        --path . \
        --language rust \
        -D "Rust current path app" \
        -A "ForkLaunch" \
        -L MIT

    test -f Cargo.toml
    test -f .forklaunch/manifest.toml
    test -f README.md
    test -d crates
    grep -q 'app_name = "rust-current"' .forklaunch/manifest.toml
)

run_cli init application rust-custom \
    --path custom/path \
    --language rust \
    -D "Rust custom path app" \
    -A "ForkLaunch" \
    -L MIT

test -f custom/path/Cargo.toml
test -f custom/path/.forklaunch/manifest.toml
grep -q 'app_name = "rust-custom"' custom/path/.forklaunch/manifest.toml

run_cli init application rust-sdk-only \
    --path rust-sdk-only \
    --language rust \
    --sdk-targets rust \
    -D "Rust SDK only app" \
    -A "ForkLaunch" \
    -L MIT

test -f rust-sdk-only/Cargo.toml
test ! -f rust-sdk-only/package.json
grep -q 'sdkTargets = \["rust"\]' rust-sdk-only/.forklaunch/manifest.toml

run_cli init application ts-sdk-only \
    --path ts-sdk-only \
    --language rust \
    --sdk-targets ts \
    -D "TypeScript SDK only app" \
    -A "ForkLaunch" \
    -L MIT

test -f ts-sdk-only/Cargo.toml
test -f ts-sdk-only/package.json
test -f ts-sdk-only/sdks/typescript/src/index.ts
grep -q 'sdkTargets = \["typescript"\]' ts-sdk-only/.forklaunch/manifest.toml

run_cli init application mixed-app \
    --path mixed-app \
    --language mixed \
    -D "Mixed app" \
    -A "ForkLaunch" \
    -L MIT

test -f mixed-app/Cargo.toml
test -f mixed-app/package.json
test -f mixed-app/pnpm-workspace.yaml
test -f mixed-app/sdks/typescript/src/index.ts
grep -q 'language = "mixed"' mixed-app/.forklaunch/manifest.toml

run_cli init service accounts \
    --path rust-app \
    --language rust \
    -d postgresql \
    -D "Accounts service"

run_cli init service inventory \
    --path rust-app/crates \
    --language rust \
    -d sqlite \
    -D "Inventory service"

run_cli init worker email-worker \
    --path rust-app \
    --language rust \
    -t database \
    -d postgresql \
    -D "Email worker"

run_cli init worker cache-worker \
    --path rust-app/crates \
    --language rust \
    -t redis \
    -D "Cache worker"

run_cli init library shared-kernel \
    --path rust-app \
    --language rust \
    -D "Shared kernel"

run_cli init router users \
    --path rust-app/crates/accounts \
    --language rust

test -f rust-app/crates/accounts/Cargo.toml
test -f rust-app/crates/accounts/src/lib.rs
test -f rust-app/crates/accounts/src/main.rs
test -f rust-app/crates/accounts/src/config/mod.rs
test -f rust-app/crates/accounts/src/domain/mod.rs
test -f rust-app/crates/accounts/src/persistence/mod.rs
test -f rust-app/crates/accounts/src/routes/users.rs
test -f rust-app/crates/inventory/src/routes/mod.rs
test -f rust-app/crates/inventory/src/lib.rs
test -f rust-app/crates/inventory/src/main.rs
test -f rust-app/crates/email-worker/src/lib.rs
test -f rust-app/crates/email-worker/src/main.rs
test -f rust-app/crates/email-worker/src/jobs/mod.rs
test -f rust-app/crates/cache-worker/src/lib.rs
test -f rust-app/crates/cache-worker/src/main.rs
test -f rust-app/crates/cache-worker/src/jobs/mod.rs
test -f rust-app/crates/shared-kernel/src/lib.rs
grep -q '"crates/accounts"' rust-app/Cargo.toml
grep -q '"crates/inventory"' rust-app/Cargo.toml
grep -q '"crates/email-worker"' rust-app/Cargo.toml
grep -q '"crates/cache-worker"' rust-app/Cargo.toml
grep -q '"crates/shared-kernel"' rust-app/Cargo.toml
grep -q 'shared-kernel' rust-app/crates/accounts/Cargo.toml
grep -q '../shared-kernel' rust-app/crates/accounts/Cargo.toml
grep -q 'shared-kernel' rust-app/crates/inventory/Cargo.toml
grep -q '../shared-kernel' rust-app/crates/inventory/Cargo.toml
grep -q 'shared-kernel' rust-app/crates/email-worker/Cargo.toml
grep -q '../shared-kernel' rust-app/crates/email-worker/Cargo.toml
grep -q 'shared-kernel' rust-app/crates/cache-worker/Cargo.toml
grep -q '../shared-kernel' rust-app/crates/cache-worker/Cargo.toml
grep -q 'database = "postgresql"' rust-app/.forklaunch/manifest.toml
grep -q 'database = "sqlite"' rust-app/.forklaunch/manifest.toml
grep -q 'workerType = "database"' rust-app/.forklaunch/manifest.toml
grep -q 'workerType = "redis"' rust-app/.forklaunch/manifest.toml
grep -q 'users' rust-app/.forklaunch/manifest.toml
grep -q 'pub struct AppState' rust-app/crates/accounts/src/lib.rs
grep -q 'TcpListener::bind' rust-app/crates/accounts/src/main.rs
grep -q 'pub struct InMemoryRepository' rust-app/crates/accounts/src/persistence/mod.rs
grep -q 'pub fn list' rust-app/crates/accounts/src/routes/users.rs
grep -q 'pub struct JobReport' rust-app/crates/email-worker/src/jobs/mod.rs
grep -q 'pub struct LibraryService' rust-app/crates/shared-kernel/src/service/mod.rs

printf '\n// user route marker\n' >> rust-app/crates/accounts/src/routes/users.rs
run_cli init router users \
    --path rust-app/crates/accounts \
    --language rust
grep -q 'user route marker' rust-app/crates/accounts/src/routes/users.rs
grep -q 'pub mod users;' rust-app/crates/accounts/src/routes/mod.rs
printf '\n// user domain marker\n' >> rust-app/crates/accounts/src/domain/mod.rs
printf '\n// user routes mod marker\n' >> rust-app/crates/accounts/src/routes/mod.rs
run_cli init service accounts \
    --path rust-app \
    --language rust \
    -d postgresql \
    -D "Accounts service"
grep -q 'user route marker' rust-app/crates/accounts/src/routes/users.rs
grep -q 'user domain marker' rust-app/crates/accounts/src/domain/mod.rs
grep -q 'user routes mod marker' rust-app/crates/accounts/src/routes/mod.rs
grep -q 'pub mod users;' rust-app/crates/accounts/src/routes/mod.rs
grep -q 'shared-kernel' rust-app/crates/accounts/Cargo.toml
grep -q '../shared-kernel' rust-app/crates/accounts/Cargo.toml

if (
    cd rust-app
    run_cli sdk mode -t live > live-mode.log 2>&1
); then
    echo "[FAIL] Rust apps should reject live SDK mode"
    exit 1
fi
grep -q 'Live SDK mode is only supported for TypeScript projects' rust-app/live-mode.log

echo "[INFO] Generated Rust crates:"
find rust-app/crates -mindepth 2 -maxdepth 2 -name Cargo.toml | sort

run_cli init application dryrun-app \
    --path dryrun-app \
    --language rust \
    --dryrun \
    -D "Dry run app" \
    -A "ForkLaunch" \
    -L MIT
test ! -d dryrun-app

run_cli init service dry-service \
    --path rust-app \
    --language rust \
    --dryrun \
    -D "Dry service"
test ! -d rust-app/crates/dry-service
! grep -q '"crates/dry-service"' rust-app/Cargo.toml

run_cli init worker dry-worker \
    --path rust-app \
    --language rust \
    --dryrun \
    -t redis \
    -D "Dry worker"
test ! -d rust-app/crates/dry-worker
! grep -q '"crates/dry-worker"' rust-app/Cargo.toml

run_cli init library dry-library \
    --path rust-app \
    --language rust \
    --dryrun \
    -D "Dry library"
test ! -d rust-app/crates/dry-library
! grep -q '"crates/dry-library"' rust-app/Cargo.toml

run_cli init router dry-router \
    --path rust-app/crates/accounts \
    --language rust \
    --dryrun
test ! -f rust-app/crates/accounts/src/routes/dry_router.rs
! grep -q 'dry-router' rust-app/.forklaunch/manifest.toml

(
    cd rust-app
    cargo test --workspace
)

echo "[INFO] Generated Rust source files:"
find rust-app -type f -name '*.rs' | sort

echo "[INFO] Generated Rust app manifest/config files:"
find rust-app -maxdepth 3 -type f \
    ! -path '*/target/*' \
    ! -name '.DS_Store' \
    ! -name '*.rs' \
    | sort \
    | sed -n '1,80p'

FORKLAUNCH_CLI="${FORKLAUNCH_CLI:-$SCRIPT_DIR/../target/release/forklaunch}" python3 "$SCRIPT_DIR/rust_name_prompt.py"

echo "[SUCCESS] Rust init command tests passed"
