SHELL := /bin/bash
.DEFAULT_GOAL := help

# Root env (AWS_PROFILE, AWS_REGION, PROJECT) -- sourced automatically, no
# manual export needed.
-include ../.env
export AWS_PROFILE AWS_REGION PROJECT

# `make run` reloads on source edits via cargo-watch. WATCH=0 runs once.
WATCH       ?= 1
WATCH_PATHS := -w lib -w main.rs -w Cargo.toml -w Cargo.lock

# Allow `make run dev` / `make run prod`. Bare `make run` defaults to dev.
STAGE := $(word 2,$(MAKECMDGOALS))
ifeq ($(STAGE),)
STAGE := dev
endif
ENV_FILE := .env.$(STAGE)

define REQUIRE_ENV
@test -n "$$PROJECT" || { \
  echo "error: PROJECT is not set -- ../.env is missing or malformed."; \
  echo "expected AWS_PROFILE, AWS_REGION, PROJECT in the repo root .env."; \
  exit 1; }
endef

# Everything that touches the database needs the stage's endpoint. There is
# no local DSQL -- no emulator, no container -- so this is the same handoff
# as `infra/make output <stage>` was for the old backend.
define REQUIRE_DB
@test -f $(ENV_FILE) || { \
  echo "error: $(ENV_FILE) missing."; \
  echo "run 'cd ../infra && make output $(STAGE)' and copy the values into backend/$(ENV_FILE)"; \
  exit 1; }
@set -a; . ./$(ENV_FILE); set +a; test -n "$$DATABASE_ENDPOINT" || { \
  echo "error: DATABASE_ENDPOINT unset in $(ENV_FILE)"; \
  exit 1; }
endef

# A fresh IAM auth token, used as the password. Nothing is stored, and there
# is no password to look up.
DSQL_TOKEN = aws dsql generate-db-connect-admin-auth-token \
  --region "$${AWS_REGION:-us-east-1}" --expires-in 43200 --hostname "$$DATABASE_ENDPOINT"

# Swallow the stage word so make does not treat it as a target.
.PHONY: dev prod
dev prod:
	@:

.PHONY: help
help:
	@echo "make init            install toolchain + fetch deps"
	@echo "make run [stage]     run locally on http://localhost:8000 (reloads on src edits; WATCH=0 disables)"
	@echo "make db-generate     diff schema.rs against the current shape, write any migration files (local only, no stage)"
	@echo "make db-shell [stage] psql against the stage, with a fresh IAM token"
	@echo "make test | fmt | lint | clean"
	@echo
	@echo "note: this backend is mid-overhaul -- no 'migrate up/down/status' yet"
	@echo "(core::data::orm::generate is still a stub), and no lambda build/deploy"
	@echo "until the Lambda wiring comes back."

.PHONY: init
init:
	$(REQUIRE_ENV)
	@command -v cargo >/dev/null || { echo "cargo not found: https://rustup.rs"; exit 1; }
	@command -v cargo-watch >/dev/null || cargo install cargo-watch --locked
	@command -v psql >/dev/null || echo "note: psql not found -- 'make db-shell' needs it: https://www.postgresql.org/download/"
	cargo fetch
	@echo "backend ready."

.PHONY: run
run:
	$(REQUIRE_ENV)
	@set -a; if [ -f $(ENV_FILE) ]; then . ./$(ENV_FILE); fi; set +a; \
	  export PORT=$${PORT:-8000}; \
	  if [ "$(WATCH)" = "1" ]; then \
	    command -v cargo-watch >/dev/null || { \
	      echo "error: cargo-watch not found (needed for hot reload)."; \
	      echo "run 'make init', or 'make run WATCH=0' to run once without it."; \
	      exit 1; }; \
	    echo "watching -- edits rebuild and restart the server"; \
	    exec cargo watch $(WATCH_PATHS) --why -x run; \
	  else \
	    exec cargo run; \
	  fi

.PHONY: test
test:
	$(REQUIRE_ENV)
	cargo test

.PHONY: fmt
fmt:
	$(REQUIRE_ENV)
	cargo fmt

.PHONY: lint
lint:
	$(REQUIRE_ENV)
	cargo clippy --all-targets -- -D warnings

# ---------------------------------------------------------------------------
# Database
#
# `db-generate` diffs schema.rs against the database's last known shape and
# writes whatever migration files the difference calls for -- see
# core/data/orm/mod.rs. It touches no database and needs no stage or
# .env.<stage>, same as the old backend's `db-generate`; the diffing itself
# is still a stub, so this currently fails loudly with a "not yet
# implemented" rather than writing anything.
# ---------------------------------------------------------------------------

.PHONY: db-generate
db-generate:
	$(REQUIRE_ENV)
	cargo run --quiet -- migrate generate

.PHONY: db-shell
db-shell:
	$(REQUIRE_ENV)
	$(REQUIRE_DB)
	@command -v psql >/dev/null || { echo "psql not found: https://www.postgresql.org/download/"; exit 1; }
	@set -a; . ./$(ENV_FILE); set +a; \
	  echo "connecting to $(STAGE): $$DATABASE_ENDPOINT"; \
	  export PGPASSWORD=$$($(DSQL_TOKEN)); \
	  export PGSSLMODE=require; \
	  exec psql --quiet \
	    --username "$${DATABASE_USER:-admin}" \
	    --dbname "$${DATABASE_NAME:-postgres}" \
	    --host "$$DATABASE_ENDPOINT"

.PHONY: clean
clean:
	$(REQUIRE_ENV)
	cargo clean
