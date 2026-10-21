# Project-local tasks, equivalent to service package scripts.
PACKAGE := fieldnotes-notes
REPO_ROOT := ../..
BINARY := fieldnotes-notes
include $(REPO_ROOT)/scripts/cargo-project.mk

.PHONY: migrate migrate-up migrate-status migrate-baseline migrate-create
migrate migrate-up:
	$(PYTHON) $(REPO_ROOT)/scripts/with-env.py --env-file "$(DOTENV_FILE_PATH)" -- $(CARGO) run --locked --manifest-path $(MANIFEST) --package $(PACKAGE) --bin $(BINARY) -- --migrate-only

migrate-status:
	$(PYTHON) $(REPO_ROOT)/scripts/with-env.py --env-file "$(DOTENV_FILE_PATH)" -- $(CARGO) run --locked --manifest-path $(MANIFEST) --package $(PACKAGE) --bin $(BINARY) -- --migrate-status

migrate-baseline:
	$(PYTHON) $(REPO_ROOT)/scripts/with-env.py --env-file "$(DOTENV_FILE_PATH)" -- $(CARGO) run --locked --manifest-path $(MANIFEST) --package $(PACKAGE) --bin $(BINARY) -- --migrate-baseline

migrate-create:
	$(PYTHON) migrations/create.py "$(NAME)"
