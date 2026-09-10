SHELL := /bin/bash
.DEFAULT_GOAL := help

# Allow `make run dev` / `make run prod` -- picks the backend stage.
# Frontend always runs as `local` (talks to localhost:8000) regardless.
STAGE := $(word 2,$(MAKECMDGOALS))
ifeq ($(STAGE),)
STAGE := dev
endif

# Swallow the stage word so make does not treat it as a target.
.PHONY: dev prod
dev prod:
	@:

.PHONY: help
help:
	@echo "make init          run 'make init' in backend, frontend, infra"
	@echo "make run [stage]   run backend (dev/prod, default dev) and frontend (local) together, ctrl+c stops both"

.PHONY: init
init:
	$(MAKE) -C backend init
	$(MAKE) -C frontend init
	$(MAKE) -C infra init

.PHONY: run
run:
	@trap 'kill $$backend_pid $$frontend_pid 2>/dev/null; fuser -k 8000/tcp 5173/tcp 2>/dev/null; true' EXIT INT TERM; \
	(cd backend && exec make run $(STAGE)) & backend_pid=$$!; \
	(cd frontend && exec make run local) & frontend_pid=$$!; \
	wait
