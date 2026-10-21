# Shared implementation; each project declares its own package and optional binary.
CARGO ?= cargo
PYTHON ?= python3
MANIFEST := Cargo.toml
DOTENV_FILE_PATH ?= .env.local
DEV_MAKE := $(MAKE)
.DEFAULT_GOAL := check
.PHONY: fmt format fmt-check lint lint-fix check test build docs clean

fmt format:
	$(CARGO) fmt --manifest-path $(MANIFEST) --package $(PACKAGE)

fmt-check:
	$(CARGO) fmt --manifest-path $(MANIFEST) --package $(PACKAGE) -- --check

lint:
	$(CARGO) clippy --locked --manifest-path $(MANIFEST) --package $(PACKAGE) --all-targets --no-deps -- -D warnings

lint-fix:
	$(CARGO) clippy --fix --allow-no-vcs --locked --manifest-path $(MANIFEST) --package $(PACKAGE) --all-targets --no-deps -- -D warnings

check:
	$(CARGO) check --locked --manifest-path $(MANIFEST) --package $(PACKAGE) --all-targets

test:
	$(CARGO) test --locked --manifest-path $(MANIFEST) --package $(PACKAGE)

build:
	$(CARGO) build --locked --manifest-path $(MANIFEST) --package $(PACKAGE)

docs:
	$(CARGO) doc --locked --manifest-path $(MANIFEST) --package $(PACKAGE) --no-deps

# Scope cleanup to this package; preserve dependencies, lockfile, data and credentials.
clean:
	$(CARGO) clean --manifest-path $(MANIFEST) --package $(PACKAGE)

ifdef BINARY
.PHONY: run start dev
run start:
	$(PYTHON) $(REPO_ROOT)/scripts/with-env.py --env-file "$(DOTENV_FILE_PATH)" -- $(CARGO) run --locked --manifest-path $(MANIFEST) --package $(PACKAGE) --bin $(BINARY)

dev:
	$(CARGO) watch --version >/dev/null 2>&1 || { echo 'Install cargo-watch to use dev: cargo install cargo-watch --locked'; exit 1; }
	$(CARGO) watch --workdir . --watch $(REPO_ROOT)/src --watch . --ignore '**/client-sdk/typescript/**' --ignore '**/target/**' --ignore '**/node_modules/**' --ignore '**/dist/**' --shell '$(DEV_MAKE) --no-print-directory start'
endif
