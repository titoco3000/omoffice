SHELL := /bin/bash
.DEFAULT_GOAL := help

BIN  := omoffice-api
ARCH ?= arm64

# Allow `make run dev` / `make deploy prod`. Bare `make run` defaults to dev.
STAGE := $(word 2,$(MAKECMDGOALS))
ifeq ($(STAGE),)
STAGE := dev
endif
ENV_FILE := .env.$(STAGE)
ZIP      := target/lambda/$(BIN)/bootstrap.zip

define REQUIRE_ENV
@test -n "$$PROJECT" || { \
  echo "error: PROJECT is not set -- the root env vars are not exported."; \
  echo "export them from the repo root first (see README):"; \
  echo "  set -a; . ./.env; set +a"; \
  exit 1; }
endef

# Swallow the stage word so make does not treat it as a target.
.PHONY: dev prod
dev prod:
	@:

.PHONY: help
help:
	@echo "make init            install toolchain + fetch deps"
	@echo "make run [stage]     run locally on http://localhost:8000"
	@echo "make build [stage]   cross-compile lambda zip ($(ARCH))"
	@echo "make deploy [stage]  build + push code to the lambda from infra"
	@echo "make test | fmt | lint"

.PHONY: init
init:
	$(REQUIRE_ENV)
	@command -v cargo >/dev/null || { echo "cargo not found: https://rustup.rs"; exit 1; }
	@command -v cargo-lambda >/dev/null || cargo install cargo-lambda --locked
	@command -v zig >/dev/null || { echo "zig not found (cargo-lambda cross-compiles with it): https://ziglang.org/download/"; exit 1; }
	rustup target add aarch64-unknown-linux-gnu x86_64-unknown-linux-gnu
	cargo fetch
	@echo "backend ready."

.PHONY: run
run:
	$(REQUIRE_ENV)
	PORT=$${PORT:-8000} cargo run

.PHONY: build
build:
	$(REQUIRE_ENV)
	cargo lambda build --release --$(ARCH) --output-format zip

.PHONY: deploy
deploy: build
	$(REQUIRE_ENV)
	@test -f $(ENV_FILE) || { \
	  echo "error: $(ENV_FILE) missing."; \
	  echo "run 'cd ../infra && make output $(STAGE)' and copy the values into backend/$(ENV_FILE)"; \
	  exit 1; }
	@set -a; . ./$(ENV_FILE); set +a; \
	  test -n "$$LAMBDA_FUNCTION_NAME" || { echo "error: LAMBDA_FUNCTION_NAME unset in $(ENV_FILE)"; exit 1; }; \
	  echo "uploading $(ZIP) -> $$LAMBDA_FUNCTION_NAME"; \
	  aws lambda update-function-code \
	    --function-name "$$LAMBDA_FUNCTION_NAME" \
	    --zip-file "fileb://$(ZIP)" \
	    --query 'LastUpdateStatus' --output text; \
	  aws lambda wait function-updated --function-name "$$LAMBDA_FUNCTION_NAME"; \
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

.PHONY: clean
clean:
	$(REQUIRE_ENV)
	cargo clean
