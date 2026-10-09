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

echo "[TEST] Rust change commands"

rm -rf output/rust-change-commands
mkdir -p output/rust-change-commands
cd output/rust-change-commands

run_cli init application rust-change \
    --path rust-change \
    --language rust \
    -D "Rust change app" \
    -A "ForkLaunch" \
    -L MIT

run_cli change application \
    --path rust-change \
    -N changed-rust-app \
    -D "Changed Rust app" \
    -A "ForkLaunch Team" \
    -L "AGPL-3.0" \
    --confirm

grep -q 'app_name = "changed-rust-app"' rust-change/.forklaunch/manifest.toml
grep -q 'app_description = "Changed Rust app"' rust-change/.forklaunch/manifest.toml
grep -q 'author = "ForkLaunch Team"' rust-change/.forklaunch/manifest.toml
grep -q 'license = "AGPL-3.0"' rust-change/.forklaunch/manifest.toml
grep -q 'license = "AGPL-3.0"' rust-change/Cargo.toml
grep -q '"ForkLaunch Team"' rust-change/Cargo.toml
grep -q '# changed-rust-app' rust-change/README.md

run_cli init service accounts \
    --path rust-change \
    --language rust \
    -d postgresql \
    -D "Accounts service"

run_cli init worker jobs \
    --path rust-change \
    --language rust \
    -t database \
    -d postgresql \
    -D "Jobs worker"

run_cli init library shared \
    --path rust-change \
    --language rust \
    -D "Shared library"

grep -q 'shared' rust-change/crates/accounts/Cargo.toml
grep -q '../shared' rust-change/crates/accounts/Cargo.toml
grep -q 'shared' rust-change/crates/jobs/Cargo.toml
grep -q '../shared' rust-change/crates/jobs/Cargo.toml

run_cli init router users \
    --path rust-change/crates/accounts \
    --language rust

run_cli change service \
    --path rust-change/crates/accounts \
    -N customer-accounts \
    -d sqlite \
    -D "Customer accounts service" \
    --confirm

test ! -d rust-change/crates/accounts
test -d rust-change/crates/customer-accounts
grep -q 'name = "customer-accounts"' rust-change/crates/customer-accounts/Cargo.toml
grep -q '"crates/customer-accounts"' rust-change/Cargo.toml
! grep -q '"crates/accounts"' rust-change/Cargo.toml
grep -q 'name = "customer-accounts"' rust-change/.forklaunch/manifest.toml
grep -q 'description = "Customer accounts service"' rust-change/.forklaunch/manifest.toml
grep -q 'database = "sqlite"' rust-change/.forklaunch/manifest.toml

run_cli change service \
    --path rust-change/crates/customer-accounts \
    --to worker \
    -t redis \
    --confirm

test -f rust-change/crates/customer-accounts/src/jobs/mod.rs
grep -q 'type = "Worker"' rust-change/.forklaunch/manifest.toml
grep -q 'workerType = "redis"' rust-change/.forklaunch/manifest.toml

run_cli change worker \
    --path rust-change/crates/customer-accounts \
    --to service \
    --confirm

test -f rust-change/crates/customer-accounts/src/routes/mod.rs

run_cli change worker \
    --path rust-change/crates/jobs \
    -N email-jobs \
    -t redis \
    -D "Email jobs worker" \
    --confirm

test ! -d rust-change/crates/jobs
test -d rust-change/crates/email-jobs
grep -q 'name = "email-jobs"' rust-change/crates/email-jobs/Cargo.toml
grep -q '"crates/email-jobs"' rust-change/Cargo.toml
grep -q 'description = "Email jobs worker"' rust-change/.forklaunch/manifest.toml

run_cli change library \
    --path rust-change/crates/shared \
    -N common-core \
    -D "Common core" \
    --confirm

test ! -d rust-change/crates/shared
test -d rust-change/crates/common-core
grep -q 'name = "common-core"' rust-change/crates/common-core/Cargo.toml
grep -q '"crates/common-core"' rust-change/Cargo.toml
grep -q 'description = "Common core"' rust-change/.forklaunch/manifest.toml
! grep -q 'shared' rust-change/crates/customer-accounts/Cargo.toml
grep -q 'common-core' rust-change/crates/customer-accounts/Cargo.toml
grep -q '../common-core' rust-change/crates/customer-accounts/Cargo.toml
! grep -q 'shared' rust-change/crates/email-jobs/Cargo.toml
grep -q 'common-core' rust-change/crates/email-jobs/Cargo.toml
grep -q '../common-core' rust-change/crates/email-jobs/Cargo.toml

run_cli change router \
    --path rust-change/crates/customer-accounts \
    -e users \
    -N customers \
    --confirm

test ! -f rust-change/crates/customer-accounts/src/routes/users.rs
test -f rust-change/crates/customer-accounts/src/routes/customers.rs
grep -q 'customers' rust-change/.forklaunch/manifest.toml
! grep -q 'users' rust-change/crates/customer-accounts/src/routes/mod.rs

run_cli change library \
    --path rust-change/crates/common-core \
    -N dry-core \
    --dryrun

test ! -d rust-change/crates/dry-core
test -d rust-change/crates/common-core
! grep -q 'dry-core' rust-change/.forklaunch/manifest.toml

(
    cd rust-change
    cargo test --workspace
)

echo "[SUCCESS] Rust change command tests passed"
