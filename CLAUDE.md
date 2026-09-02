# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`omoffice` — a sample API: a single Rust (axum) binary deployed to AWS Lambda behind an API Gateway v2 HTTP API, with all AWS resources managed by Terraform. Two stages: `dev` and `prod`.

## Environment

Every command below assumes the root `.env` is exported first (it carries `AWS_PROFILE`, `AWS_REGION`, `PROJECT`):

```sh
export $(grep -v '^#' .env | xargs -d '\n')
```

Every make target except `help` starts with `$(REQUIRE_ENV)`, which fails if
`PROJECT` is unset — that is the signal the root env was never exported.

## Commands

Both `backend/` and `infra/` drive everything through a Makefile that takes the stage as a bare second word (`make deploy dev`); with no stage word it defaults to `dev`. The Makefiles declare `dev`/`prod` as no-op phony targets so make does not try to build them.

Backend (`cd backend`):

```sh
make init            # verify cargo/zig, install cargo-lambda, add linux targets, cargo fetch
make run dev         # local server on http://localhost:8000 (PORT overrides)
make build dev       # cross-compile the lambda zip via cargo-lambda (ARCH, default arm64)
make deploy dev      # build, then aws lambda update-function-code from .env.<stage>
make test            # cargo test
make fmt             # cargo fmt
make lint            # cargo clippy --all-targets -- -D warnings
```

Run a single test: `cd backend && cargo test <test_name>` (`-- --nocapture` to see output).

Infra (`cd infra`):

```sh
make init            # terraform init + fmt -check + validate
make plan dev
make deploy dev      # apply, then print outputs
make output dev      # reprint outputs, including the block for backend/.env.<stage>
make destroy dev     # prompts: type the stage name to confirm
make fmt             # terraform fmt -recursive
```

Every infra target except `init` requires `.terraform/` to exist and selects the stage's terraform workspace (`terraform workspace select -or-create <stage>`) before running.

## Architecture

**One binary, two runtimes.** `backend/src/main.rs` builds a single `axum::Router` in `app()`. `main` checks `AWS_LAMBDA_RUNTIME_API` (set only by the Lambda execution environment): if present it hands the router to `lambda_http::run`, otherwise it binds a local `TcpListener`. Add routes in `app()` and both paths get them.

**Terraform owns infrastructure, the backend owns code.** `infra/main.tf` creates the Lambda from a placeholder zip (`infra/placeholder/bootstrap`) and then `ignore_changes = [filename, source_code_hash]`. The real binary is pushed only by `backend/make deploy`, so a `terraform apply` never reverts deployed code. This is why the two deploy steps exist and must not be merged.

**The handoff between the two halves is `backend/.env.<stage>`.** `infra/make output <stage>` prints a `backend_env_file` output (`LAMBDA_FUNCTION_NAME`, `API_URL`, `JWT_SECRET_ID`) that must be pasted into `backend/.env.dev` / `.env.prod`; `backend/make deploy` fails with a pointer to that command if the file or `LAMBDA_FUNCTION_NAME` is missing. `make run` sources the same file, so local runs read the real per-stage JWT secret with your AWS profile.

**Auth policy is declared in `app()`, implemented in `backend/src/auth.rs`.** `app()` builds an `AuthConfig` naming the protected prefixes (`.protect(vec!["/private"])`) and the email whitelist (`.allow_emails(...)`), so both lists sit beside the routes they govern; `auth.rs` holds no policy of its own. `require_bearer` is layered once over the whole router and checks the prefixes per request, so adding one to `.protect(...)` is the entire cost of protecting a route tree. Verified `Claims` are inserted as a request extension. `/auth/request-key/{email}` signs an HS256 token (48h) and returns it directly — there is no proof of email ownership, which is fine for a sample and not for real users.

**The signing key is a Secrets Manager secret rotated weekly** (`infra/secrets.tf`, `omoffice/jwt-key-<stage>`, `cron(0 7 ? * SUN *)` from `var.jwt_rotation_schedule`). `infra/rotator/index.py` is a standard four-step rotation lambda that terraform owns end to end — unlike the API lambda, its code is not deployed separately. The API verifies against `AWSCURRENT` then `AWSPREVIOUS`, so a 48h token survives a rotation; keys are cached in-process for 5 minutes. `KeySource::from_env()` picks the key once at startup: `JWT_KEY` env var, then `JWT_SECRET_ID`, then a built-in insecure key that is a hard startup error inside Lambda.

**Stage isolation is by terraform workspace**, not by directory: one root module in `infra/`, per-stage `envs/<stage>.tfvars` (currently only `stage = "..."`), separate state per workspace. Resource names are derived in `locals` as `<project>-api-<stage>` etc.

**State is local** (`infra/terraform.tfstate.d/`, gitignored). Move to an S3 backend before a second person deploys.

## Constraints

- Lambda architecture must match on both sides: `lambda_architecture` in `infra/envs/<stage>.tfvars` (default `arm64` from `variables.tf`) and `ARCH` in `backend/Makefile`. Change them together.
- `var.stage` is validated against `["dev", "prod"]`; adding a stage means touching `variables.tf`, a new `envs/*.tfvars`, and the Makefiles' phony stage targets.
- The API Gateway route is `$default` on a `$default` auto-deploy stage — all paths reach the Lambda, so routing is entirely in the axum router.
- `jsonwebtoken` has no default crypto provider: the `rust_crypto` feature is what supplies one. The AWS SDK deps are pinned to the rustls/ring TLS backend (`legacy-client` / `rustls`) because the default `aws-lc-rs` needs cmake and a C toolchain that the zig cross-build does not have. Changing either feature set breaks `make build`.
- The dev secret has `recovery_window_in_days = 0` so `make destroy` frees the name immediately instead of holding it for 30 days.
- The release profile is size-tuned (`opt-level = "z"`, `lto`, `strip`, `panic = "abort"`); `panic = "abort"` means no unwinding, so avoid code that relies on catching panics.

## dev-docs

`dev-docs/phase/<N>/prompt.md` holds the original feature prompts per phase, in order. Phase 1 (and point releases like 1.1) is the current implementation; later phases are the planned direction. Phases 1, 1.1 and 2 (JWT auth) are implemented.
