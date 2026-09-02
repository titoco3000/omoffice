# omoffice

Sample API. Rust backend on AWS Lambda behind an API Gateway HTTP API, with all
AWS resources managed by Terraform.

```
.
├── backend/          Rust crate (axum). One binary: local HTTP server + Lambda handler.
│   ├── src/main.rs
│   └── Makefile      init | run | build | deploy | test | fmt | lint
└── infra/            Terraform stack (Lambda, API Gateway, IAM, CloudWatch)
    ├── envs/         per-stage tfvars
    └── Makefile      init | plan | deploy | output | destroy
```

Stages are `dev` and `prod`. Each stage lives in its own Terraform workspace, so
state is separate.

## Requirements

- Rust toolchain (`rustup`), `cargo-lambda`, `zig` (used to cross-compile for Lambda)
- Terraform >= 1.5
- AWS CLI with a configured profile

## Setup

```sh
export $(grep -v '^#' .env | xargs -d '\n')
```

Every make target (except `help`) checks that `PROJECT` is set and stops with
that reminder if the env was never exported.

## Local

```sh
cd backend
make init
make run dev                  # http://localhost:8000
curl localhost:8000           # {"message":"hello world!"}
```

## Deploy

Terraform creates the Lambda with a placeholder package; the backend deploy
pushes the real binary. Terraform ignores later code changes, so the two never
fight over the function code.

```sh
cd infra
make deploy dev               # error: uninitialized
make init
make deploy dev               # prints all outputs
```

Copy the printed block into `backend/.env.dev` (see `backend/.env.dev.example`):

```
LAMBDA_FUNCTION_NAME=omoffice-api-dev
API_URL=https://xxxxxxxxxx.execute-api.us-east-1.amazonaws.com
```

Then ship the code:

```sh
cd ../backend
make deploy dev
curl "$API_URL"               # {"message":"hello world!"}
```

`make output dev` in `infra/` reprints the values at any time.

## Teardown

```sh
cd infra
make destroy dev              # prompts for confirmation
```

## Notes

- Lambda architecture is `arm64` on both sides. To switch, change
  `lambda_architecture` in `infra/envs/<stage>.tfvars` and `ARCH` in
  `backend/Makefile` together.
- Terraform state is local (`infra/terraform.tfstate.d/`). Move it to an S3
  backend before more than one person deploys.
