# Workspace task orchestration; runtime tasks remain service-specific.
.DEFAULT_GOAL := check
PROJECTS := src/core/security src/notes/domain src/notes src/insights src/client-sdk/rust
.PHONY: check lint lint-fix fmt format fmt-check test build docs clean up down
check lint lint-fix fmt format fmt-check test build docs clean:
	@set -e; for project in $(PROJECTS); do $(MAKE) -C "$$project" $@; done

up:
	docker compose up --build -d

down:
	docker compose down
