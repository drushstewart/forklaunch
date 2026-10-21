# Project-local tasks, equivalent to service package scripts.
PACKAGE := fieldnotes-insights
REPO_ROOT := ../..
BINARY := fieldnotes-insights
include $(REPO_ROOT)/scripts/cargo-project.mk

.PHONY: migrate migrate-up migrate-init
migrate migrate-up migrate-init:
	$(PYTHON) $(REPO_ROOT)/scripts/with-env.py --env-file "$(DOTENV_FILE_PATH)" -- $(CARGO) run --locked --manifest-path $(MANIFEST) --package $(PACKAGE) --bin $(BINARY) -- --migrate-only
