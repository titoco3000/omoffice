# Omoffice

Meetings app for organizations.

```
.
├── backend/          Rust crate (axum). One binary: local HTTP server + Lambda handler.
├── frontend/         React + Vite + Tailwind. 
└── infra/            Terraform stack (Lambda, API Gateway, IAM, CloudWatch,
                      Secrets Manager, S3, CloudFront)
```

Stages are `dev` and `prod`. Each stage lives in its own Terraform workspace, so
state is separate.

## Requirements

- Rust toolchain (`rustup`), `cargo-lambda`, `zig` (used to cross-compile for Lambda)
- Node 20+ (for the frontend)
- Terraform >= 1.5
- AWS CLI with a configured profile
- `psql` 14+

## Local Quickstart

```sh
# in one terminal
cd backend
make init
make run dev    # http://localhost:8000, reloads on src edit

# in another
cd frontend
make init
make run local  # http://localhost:5173, refreshes on src edit
```

## Usage Guide

Each of `backend/`, `frontend/` and `infra/` is driven through its own Makefile.
The stage is a bare second word (`make deploy dev`); `backend/`/`infra/` default
to `dev`, `frontend/` to `local`. Run `make help` in any of them for the full
target list.

### /infra - Resources

Resources managed via Terraform. Stage isolation with `dev` and `prod` .
Per-stage variables live in `infra/envs/<stage>.tfvars`.

```sh
cd infra
make init             # terraform init/fmt -check/validate
make plan <stage>         # show planned changes
make deploy <stage>       # apply, then print outputs
make output <stage>       # reprint outputs without applying
make destroy <stage>      # tear the stage down (asks for confirmation)
```

`make deploy`/`make output` ends by printing a `backend_env_file` and a
`frontend_env_file` block -- copy those into `backend/.env.<stage>` and
`frontend/.env.<stage>` respectively. That handoff is manual on purpose: it's
the seam between what Terraform owns (infrastructure) and what each app's own
`make deploy` owns (code/assets), so applying infra never overwrites what's
running.

Terraform state is local (`infra/terraform.tfstate.d/`, gitignored) - needs upgrade 
to an S3 backend if more than one person is going to handle deploys.

### /backend - API

`backend/` is a Rust (axum) binary that runs both as a local server and as the
Lambda handler using the same router.

```sh
cd backend
make init                 # install dependencies
make run <stage>          # http://localhost:8000, reloads on src/ edits (WATCH=0 to disable)
make build <stage>        # cross-compile the lambda zip (cargo-lambda, ARCH default arm64)
make deploy <stage>       # build + push the zip to the lambda named in .env.<stage>
make test | fmt | lint
```

There's no local database or JWT secret -- `make run`/`make deploy` reads
`backend/.env.<stage>` and talks to the real per-stage Aurora DSQL cluster and 
Secrets Manager secret, same as when deployed. `make deploy` only pushes the binary; 
it never touches infrastructure, so a `terraform apply` can't revert deployed code.

### /backend - Database

Aurora DSQL, one cluster per stage, no local emulator. Migrations are ordered
SQL pairs in `backend/migrations/`, run by a subcommand of the API binary.

```sh
cd backend
make db-generate         # diff schema.rs against migrations/.snapshot.json, write new migration files (local, no stage)
make db-status <stage>   # which migrations are applied
make db-migrate <stage>  # apply every pending migration
make db-revert <stage>   # revert the most recent one (destructive, asks first)
make db-shell <stage>    # psql against the stage, with a fresh IAM token
```

#### Workflow:
1. edit `backend/src/core/schema.rs`
2. `make db-generate`
3. review the generated `.up.sql`/`.down.sql`
4. `make db-migrate <stage>`.

### Frontend

`frontend/` is React/Vite/Tailwind, deployed to S3 behind CloudFront.

```sh
cd frontend
make init            # npm install
make run <stage>     # runs on http://localhost:5173, talks to API from the stage (local/dev/prod)
make build <stage>   # production build into dist/
make deploy <stage>  # build + sync dist/ to S3 + invalidate CloudFront
make preview <stage> # serve the built dist/ locally
make lint
```

Stage config comes from `frontend/.env.<stage>`, sourced into Vite's process
env.
