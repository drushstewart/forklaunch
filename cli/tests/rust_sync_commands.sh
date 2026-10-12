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

replace_in_tree() {
    local root="$1"
    local from="$2"
    local to="$3"

    while IFS= read -r file; do
        sed -i.bak "s/${from}/${to}/g" "$file"
        rm -f "$file.bak"
    done < <(find "$root" -type f)
}

echo "[TEST] Rust sync commands"

rm -rf output/rust-sync-commands
mkdir -p output/rust-sync-commands
cd output/rust-sync-commands

run_cli init application rust-sync \
    --path rust-sync \
    --language rust \
    -D "Rust sync app" \
    -A "ForkLaunch" \
    -L MIT

run_cli init service users \
    --path rust-sync \
    --language rust \
    -d postgresql \
    -D "Users service"

run_cli init worker email-worker \
    --path rust-sync \
    --language rust \
    -t database \
    -d postgresql \
    -D "Email worker"

run_cli init library shared-types \
    --path rust-sync \
    --language rust \
    -D "Shared types"

run_cli sync service users --path rust-sync
run_cli sync worker email-worker --path rust-sync
run_cli sync library shared-types --path rust-sync
run_cli sync all --path rust-sync --confirm

grep -q 'shared-types' rust-sync/crates/users/Cargo.toml
grep -q '../shared-types' rust-sync/crates/users/Cargo.toml
grep -q 'shared-types' rust-sync/crates/email-worker/Cargo.toml
grep -q '../shared-types' rust-sync/crates/email-worker/Cargo.toml

cp -R rust-sync/crates/users rust-sync/crates/products
replace_in_tree rust-sync/crates/products users products
! grep -q 'name = "products"' rust-sync/.forklaunch/manifest.toml

run_cli sync service products --path rust-sync
grep -q 'name = "products"' rust-sync/.forklaunch/manifest.toml
grep -q 'cratePath = "crates/products"' rust-sync/.forklaunch/manifest.toml
grep -q '"crates/products"' rust-sync/Cargo.toml

run_cli sync service products --path rust-sync
grep -q 'name = "products"' rust-sync/.forklaunch/manifest.toml

cp -R rust-sync/crates/email-worker rust-sync/crates/video-worker
replace_in_tree rust-sync/crates/video-worker email-worker video-worker
replace_in_tree rust-sync/crates/video-worker email_worker video_worker
! grep -q 'name = "video-worker"' rust-sync/.forklaunch/manifest.toml

run_cli sync worker video-worker --path rust-sync
grep -q 'name = "video-worker"' rust-sync/.forklaunch/manifest.toml
grep -q 'cratePath = "crates/video-worker"' rust-sync/.forklaunch/manifest.toml
grep -q '"crates/video-worker"' rust-sync/Cargo.toml

cp -R rust-sync/crates/shared-types rust-sync/crates/common-utils
replace_in_tree rust-sync/crates/common-utils shared-types common-utils
replace_in_tree rust-sync/crates/common-utils shared_types common_utils
! grep -q 'name = "common-utils"' rust-sync/.forklaunch/manifest.toml

run_cli sync library common-utils --path rust-sync
grep -q 'name = "common-utils"' rust-sync/.forklaunch/manifest.toml
grep -q 'cratePath = "crates/common-utils"' rust-sync/.forklaunch/manifest.toml
grep -q '"crates/common-utils"' rust-sync/Cargo.toml
grep -q 'common-utils' rust-sync/crates/users/Cargo.toml
grep -q '../common-utils' rust-sync/crates/users/Cargo.toml
grep -q 'common-utils' rust-sync/crates/email-worker/Cargo.toml
grep -q '../common-utils' rust-sync/crates/email-worker/Cargo.toml

if run_cli sync service missing-service --path rust-sync > missing-service.log 2>&1; then
    echo "[FAIL] Missing Rust service sync should fail"
    exit 1
fi
grep -q 'Service directory not found: .*missing-service' missing-service.log

rm -rf rust-sync/crates/products
run_cli sync all --path rust-sync --confirm
! grep -q 'name = "products"' rust-sync/.forklaunch/manifest.toml
! grep -q '"crates/products"' rust-sync/Cargo.toml

(
    cd rust-sync
    cargo test --workspace
)

echo "[SUCCESS] Rust sync command tests passed"
