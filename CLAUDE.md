# CLAUDE.md

Omoffice: meetings app for organizations. Three parts, each with its own Makefile:

```
backend/    Rust crate (axum). One binary: local HTTP server + Lambda handler.   -> backend/CLAUDE.md
frontend/   React + Vite + Tailwind.                                             -> frontend/CLAUDE.md
infra/      Terraform stack (Lambda, API Gateway, AppSync Events, IAM, CloudWatch, Secrets Manager, S3, CloudFront)
```

Stages are `dev` and `prod`, each its own Terraform workspace (separate state). Most `make` targets take the stage as a bare second word: `make run dev`, `make deploy prod`. `backend`/`infra` default to `dev`; `frontend` always runs as `local` (talks to `localhost:8000`) regardless of what stage the backend is on.

Root `.env` holds `AWS_PROFILE`, `AWS_REGION`, `PROJECT` and is sourced by every sub-Makefile automatically.

## Commands

```sh
make init             # init in backend, frontend, infra
make run [stage]      # backend (dev/prod) + frontend (local) together, ctrl+c stops both
make fmt              # fmt in backend, frontend, infra
```

### infra/

```sh
make plan|deploy|output|destroy [stage]
```
`deploy`/`output` print a `backend_env_file` and `frontend_env_file` block -- copy those into `backend/.env.<stage>` / `frontend/.env.<stage>` by hand. That handoff is manual on purpose: infra owns infrastructure, each app's own `make deploy` owns code/assets, so applying infra never overwrites what's running. Terraform state is local (`infra/terraform.tfstate.d/`, gitignored).

## Other

`dev-docs/phase/*.md` is a running log of each development phase's original prompt/intent -- check it for *why* something was built a certain way before assuming a design choice is accidental.
