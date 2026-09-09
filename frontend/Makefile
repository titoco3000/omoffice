SHELL := /bin/bash
.DEFAULT_GOAL := help

# Root env (AWS_PROFILE, AWS_REGION, PROJECT) -- sourced automatically, no
# manual export needed.
-include ../.env
export AWS_PROFILE AWS_REGION PROJECT

NPM  ?= npm
DIST := dist

# Allow `make run dev` / `make deploy prod`. Bare `make run` defaults to local.
STAGE := $(word 2,$(MAKECMDGOALS))
ifeq ($(STAGE),)
STAGE := local
endif
ENV_FILE := .env.$(STAGE)

define REQUIRE_ENV
@test -n "$$PROJECT" || { \
  echo "error: PROJECT is not set -- ../.env is missing or malformed."; \
  echo "expected AWS_PROFILE, AWS_REGION, PROJECT in the repo root .env."; \
  exit 1; }
endef

define REQUIRE_MODULES
@test -d node_modules || { echo "error: dependencies missing -- run 'make init' first"; exit 1; }
endef

# `local` points at localhost; only the deployed stages have infrastructure.
define REQUIRE_DEPLOYABLE_STAGE
@case "$(STAGE)" in dev|prod) ;; *) \
  echo "error: cannot deploy stage '$(STAGE)' -- deployable stages are dev and prod."; \
  exit 1;; esac
endef

# Every stage needs an env file naming the API it talks to.
define REQUIRE_ENV_FILE
@test -f $(ENV_FILE) || { \
  echo "error: $(ENV_FILE) missing."; \
  echo "run 'cd ../infra && make output $(STAGE)' and copy the values into frontend/$(ENV_FILE)"; \
  exit 1; }
endef

# Swallow the stage word so make does not treat it as a target.
.PHONY: local dev prod
local dev prod:
	@:

.PHONY: help
help:
	@echo "make init            install node dependencies"
	@echo "make run [stage]     vite dev server on http://localhost:5173 (PORT overrides)"
	@echo "make build [stage]   production build into $(DIST)/"
	@echo "make deploy [stage]  build + sync to S3 + invalidate CloudFront"
	@echo "make preview [stage] serve the built $(DIST)/ locally"
	@echo "make lint | clean"
	@echo "stages: local (backend on localhost:8000), dev, prod"

.PHONY: init
init:
	$(REQUIRE_ENV)
	@command -v node >/dev/null || { echo "node not found: https://nodejs.org/en/download"; exit 1; }
	@command -v $(NPM) >/dev/null || { echo "$(NPM) not found (ships with node)"; exit 1; }
	@command -v aws >/dev/null || { \
	  echo "aws cli not found -- needed by 'make deploy': https://docs.aws.amazon.com/cli/latest/userguide/getting-started-install.html"; exit 1; }
	$(NPM) install
	@echo "frontend ready."

.PHONY: run
run:
	$(REQUIRE_ENV)
	$(REQUIRE_MODULES)
	$(REQUIRE_ENV_FILE)
	@set -a; . ./$(ENV_FILE); set +a; \
	  test -n "$$API_URL" || { echo "error: API_URL unset in $(ENV_FILE)"; exit 1; }; \
	  echo "stage $(STAGE) -> $$API_URL"; \
	  VITE_API_URL="$$API_URL" VITE_STAGE=$(STAGE) VITE_CHAT_WS_URL="$$CHAT_WS_URL" $(NPM) run dev

.PHONY: build
build:
	$(REQUIRE_ENV)
	$(REQUIRE_MODULES)
	$(REQUIRE_ENV_FILE)
	@set -a; . ./$(ENV_FILE); set +a; \
	  test -n "$$API_URL" || { echo "error: API_URL unset in $(ENV_FILE)"; exit 1; }; \
	  echo "building $(STAGE) against $$API_URL"; \
	  VITE_API_URL="$$API_URL" VITE_STAGE=$(STAGE) VITE_CHAT_WS_URL="$$CHAT_WS_URL" $(NPM) run build

.PHONY: preview
preview: build
	$(REQUIRE_ENV)
	$(NPM) run preview

.PHONY: deploy
deploy:
	$(REQUIRE_DEPLOYABLE_STAGE)
	@$(MAKE) --no-print-directory build $(STAGE)
	$(REQUIRE_ENV)
	@set -a; . ./$(ENV_FILE); set +a; \
	  test -n "$$FRONTEND_BUCKET" || { echo "error: FRONTEND_BUCKET unset in $(ENV_FILE)"; exit 1; }; \
	  test -n "$$FRONTEND_DISTRIBUTION_ID" || { echo "error: FRONTEND_DISTRIBUTION_ID unset in $(ENV_FILE)"; exit 1; }; \
	  echo "uploading $(DIST)/ -> s3://$$FRONTEND_BUCKET"; \
	  aws s3 sync $(DIST)/ "s3://$$FRONTEND_BUCKET" --delete \
	    --exclude index.html --cache-control "public,max-age=31536000,immutable"; \
	  aws s3 cp $(DIST)/index.html "s3://$$FRONTEND_BUCKET/index.html" \
	    --cache-control "no-cache"; \
	  aws cloudfront create-invalidation \
	    --distribution-id "$$FRONTEND_DISTRIBUTION_ID" --paths '/*' \
	    --query 'Invalidation.Status' --output text; \
	  echo "deployed $(STAGE): $$FRONTEND_URL"

.PHONY: lint
lint:
	$(REQUIRE_ENV)
	$(REQUIRE_MODULES)
	$(NPM) run lint

.PHONY: clean
clean:
	$(REQUIRE_ENV)
	rm -rf $(DIST) node_modules
