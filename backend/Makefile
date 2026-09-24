SHELL := /bin/bash
.DEFAULT_GOAL := help

# Root env (AWS_PROFILE, AWS_REGION, PROJECT) -- sourced automatically, no
# manual export needed.
-include ../.env
export AWS_PROFILE AWS_REGION PROJECT

BIN  := omoffice-api
ARCH ?= arm64

# `make run` reloads on source edits via cargo-watch. WATCH=0 runs once.
WATCH       ?= 1
WATCH_PATHS := -w lib -w main.rs -w Cargo.toml -w Cargo.lock

# Allow `make run dev` / `make deploy prod`. Bare `make run` defaults to dev.
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
  echo "run 'cd ../infra && make output $(STAGE)' and copy the values into backend/$(ENV_FILE)"; \
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
	@echo "make init                install toolchain + fetch deps"
	@echo "make run [stage]         run locally on http://localhost:8000 (reloads on src edits; WATCH=0 disables)"
	@echo "make build [stage]       build the docker image ($(ARCH))"
	@echo "make deploy [stage]      build + push image to ECR, force a new ECS deployment"
	@echo "make db-generate         diff schema.rs against the last generated migration, write any new one (local only, no stage)"
	@echo "make db-status [stage]   which migrations are applied, and which are not"
	@echo "make db-migrate [stage]  apply every pending migration"
	@echo "make db-revert [stage]   revert the most recently applied one (destructive, asks first)"
	@echo "make db-shell [stage]    psql against the stage, with a fresh IAM token"
	@echo "make test | fmt | lint | clean"

.PHONY: init
init:
	$(REQUIRE_ENV)
	@command -v cargo >/dev/null || { echo "cargo not found: https://rustup.rs"; exit 1; }
	@command -v cargo-watch >/dev/null || cargo install cargo-watch --locked
	@command -v docker >/dev/null || { echo "docker not found (needed for 'make build'/'make deploy'): https://docs.docker.com/get-docker/"; exit 1; }
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

.PHONY: build
build:
	$(REQUIRE_ENV)
	docker buildx build --platform linux/$(ARCH) -t $(BIN):local -f Dockerfile .

.PHONY: deploy
deploy: build
	$(REQUIRE_ENV)
	$(REQUIRE_DB)
	@set -a; . ./$(ENV_FILE); set +a; \
	  test -n "$$ECR_REPOSITORY_URL" || { echo "error: ECR_REPOSITORY_URL unset in $(ENV_FILE)"; exit 1; }; \
	  test -n "$$ECS_CLUSTER" || { echo "error: ECS_CLUSTER unset in $(ENV_FILE)"; exit 1; }; \
	  test -n "$$ECS_SERVICE" || { echo "error: ECS_SERVICE unset in $(ENV_FILE)"; exit 1; }; \
	  aws ecr get-login-password --region "$${AWS_REGION:-us-east-1}" | docker login --username AWS --password-stdin "$$ECR_REPOSITORY_URL"; \
	  docker buildx build --platform linux/$(ARCH) -t "$$ECR_REPOSITORY_URL:$(STAGE)" -f Dockerfile --push .; \
	  echo "forcing new deployment on $$ECS_CLUSTER/$$ECS_SERVICE"; \
	  aws ecs update-service --cluster "$$ECS_CLUSTER" --service "$$ECS_SERVICE" --force-new-deployment \
	    --query 'service.deployments[0].id' --output text; \
	  echo "waiting for deployment to stabilize..."; \
	  aws ecs wait services-stable --cluster "$$ECS_CLUSTER" --services "$$ECS_SERVICE"; \
	  echo "deployed $(STAGE): $$API_URL"

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
# Migrations are ordered .up.sql/.down.sql pairs in migrations/, applied by
# the runner in core/data/migrate/. It writes a ledger row as `pending`, runs
# the DDL, then marks it `applied` -- DSQL cannot put the DDL and the ledger
# write in one transaction, so a crash has to be visible rather than
# silently half-done.
#
# `db-generate` is the odd one out: it diffs schema.rs against
# migrations/.snapshot.json (the shape as of the last time it ran) and writes
# whatever .up.sql/.down.sql the difference calls for, instead of you typing
# matching SQL into two files by hand -- see core/data/orm/mod.rs. It
# touches no database and needs no stage or .env.<stage>: it is local
# codegen, checked in the same way the files (and the snapshot) it writes are.
# ---------------------------------------------------------------------------

.PHONY: db-generate
db-generate:
	$(REQUIRE_ENV)
	cargo run --quiet -- migrate generate

.PHONY: db-status
db-status:
	$(REQUIRE_ENV)
	$(REQUIRE_DB)
	@set -a; . ./$(ENV_FILE); set +a; cargo run --quiet -- migrate status

.PHONY: db-migrate
db-migrate:
	$(REQUIRE_ENV)
	$(REQUIRE_DB)
	@set -a; . ./$(ENV_FILE); set +a; cargo run --quiet -- migrate up

.PHONY: db-revert
db-revert:
	$(REQUIRE_ENV)
	$(REQUIRE_DB)
	@echo "about to REVERT the most recently applied migration on '$(STAGE)'."
	@echo "a down migration drops what its up migration created -- dropping a column"
	@echo "drops the data that was in it, and there is no backup here."
	@read -r -p "type '$(STAGE)' to confirm: " ans; \
	  test "$$ans" = "$(STAGE)" || { echo "aborted."; exit 1; }
	@set -a; . ./$(ENV_FILE); set +a; cargo run --quiet -- migrate down

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
