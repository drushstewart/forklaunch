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

echo "[TEST] Rust delete commands"

rm -rf output/rust-delete-commands
mkdir -p output/rust-delete-commands
cd output/rust-delete-commands

run_cli init application rust-delete \
    --path rust-delete \
    --language rust \
    -D "Rust delete app" \
    -A "ForkLaunch" \
    -L MIT

run_cli init service accounts \
    --path rust-delete \
    --language rust \
    -d postgresql \
    -D "Accounts service"

run_cli init service notifications \
    --path rust-delete \
    --language rust \
    -d postgresql \
    -D "Notifications service"

run_cli init worker email-worker \
    --path rust-delete \
    --language rust \
    -t database \
    -d postgresql \
    -D "Email worker"

run_cli init library common-kernel \
    --path rust-delete \
    --language rust \
    -D "Common kernel"

grep -q 'common-kernel' rust-delete/crates/accounts/Cargo.toml
grep -q '../common-kernel' rust-delete/crates/accounts/Cargo.toml
grep -q 'common-kernel' rust-delete/crates/notifications/Cargo.toml
grep -q '../common-kernel' rust-delete/crates/notifications/Cargo.toml
grep -q 'common-kernel' rust-delete/crates/email-worker/Cargo.toml
grep -q '../common-kernel' rust-delete/crates/email-worker/Cargo.toml

run_cli init router users \
    --path rust-delete/crates/accounts \
    --language rust

run_cli init router internal-users \
    --path rust-delete/crates/accounts \
    --language rust

run_cli sdk generate \
    --path rust-delete \
    --target all

test -f rust-delete/sdks/typescript/src/index.ts
test -d rust-delete/sdks/rust/accounts-sdk
test -d rust-delete/sdks/rust/notifications-sdk
test -d rust-delete/sdks/rust/email-worker-sdk
grep -q 'accountsService' rust-delete/sdks/typescript/src/index.ts
grep -q 'notificationsService' rust-delete/sdks/typescript/src/index.ts
grep -q 'emailWorkerService' rust-delete/sdks/typescript/src/index.ts
grep -q '"sdks/rust/accounts-sdk"' rust-delete/Cargo.toml
grep -q '"sdks/rust/notifications-sdk"' rust-delete/Cargo.toml
grep -q '"sdks/rust/email-worker-sdk"' rust-delete/Cargo.toml

run_cli delete router users \
    --path rust-delete/crates/accounts \
    --continue

test ! -f rust-delete/crates/accounts/src/routes/users.rs
test -f rust-delete/crates/accounts/src/routes/internal_users.rs
! grep -q 'pub mod users;' rust-delete/crates/accounts/src/routes/mod.rs
! grep -q '"users"' rust-delete/.forklaunch/manifest.toml
grep -q 'internal-users' rust-delete/.forklaunch/manifest.toml
! grep -q 'AccountsUsersListRequest' rust-delete/sdks/typescript/src/index.ts
grep -q 'AccountsInternalUsersListRequest' rust-delete/sdks/typescript/src/index.ts

run_cli delete worker email-worker \
    --path rust-delete \
    --continue

test ! -d rust-delete/crates/email-worker
test ! -d rust-delete/sdks/rust/email-worker-sdk
! grep -q 'name = "email-worker"' rust-delete/.forklaunch/manifest.toml
! grep -q '"crates/email-worker"' rust-delete/Cargo.toml
! grep -q '"sdks/rust/email-worker-sdk"' rust-delete/Cargo.toml
! grep -q 'emailWorkerService' rust-delete/sdks/typescript/src/index.ts

run_cli delete service notifications \
    --path rust-delete \
    --continue

test ! -d rust-delete/crates/notifications
test ! -d rust-delete/sdks/rust/notifications-sdk
! grep -q 'name = "notifications"' rust-delete/.forklaunch/manifest.toml
! grep -q '"crates/notifications"' rust-delete/Cargo.toml
! grep -q '"sdks/rust/notifications-sdk"' rust-delete/Cargo.toml
! grep -q 'notificationsService' rust-delete/sdks/typescript/src/index.ts

run_cli delete library common-kernel \
    --path rust-delete \
    --continue

test ! -d rust-delete/crates/common-kernel
! grep -q 'name = "common-kernel"' rust-delete/.forklaunch/manifest.toml
! grep -q '"crates/common-kernel"' rust-delete/Cargo.toml
! grep -q 'common-kernel' rust-delete/crates/accounts/Cargo.toml

(
    cd rust-delete
    cargo test --workspace
)

run_cli delete service accounts \
    --path rust-delete \
    --continue

test ! -d rust-delete/crates/accounts
test ! -d rust-delete/sdks/rust/accounts-sdk
! grep -q 'name = "accounts"' rust-delete/.forklaunch/manifest.toml
! grep -q '"crates/accounts"' rust-delete/Cargo.toml
! grep -q '"sdks/rust/accounts-sdk"' rust-delete/Cargo.toml
! grep -q 'accountsService' rust-delete/sdks/typescript/src/index.ts
grep -q 'members = \[' rust-delete/Cargo.toml
! grep -q '"crates/' rust-delete/Cargo.toml
! grep -q '"sdks/rust/' rust-delete/Cargo.toml

echo "[SUCCESS] Rust delete command tests passed"
