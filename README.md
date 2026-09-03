# omoffice

Sample API. Rust backend on AWS Lambda behind an API Gateway HTTP API, with a
React frontend on S3 + CloudFront, and all AWS resources managed by Terraform.

```
.
├── backend/          Rust crate (axum). One binary: local HTTP server + Lambda handler.
│   ├── src/main.rs   routes, auth policy, local/Lambda entrypoint
│   ├── src/auth.rs   JWT issuing, key lookup, protected-route middleware
│   ├── src/chime.rs  Chime SDK meetings: create a meeting, join one by code
│   └── Makefile      init | run | build | deploy | test | fmt | lint
├── frontend/         React + Vite + Tailwind. A meeting panel over RouteTester cards.
│   ├── src/components/RouteTester.jsx   a form + a result box for one route
│   ├── src/components/MeetingPanel.jsx  create/join a meeting, run the media session
│   └── Makefile      init | run | build | deploy | preview | lint
└── infra/            Terraform stack (Lambda, API Gateway, IAM, CloudWatch,
    │                 Secrets Manager, S3, CloudFront)
    ├── envs/         per-stage tfvars
    ├── rotator/      Python lambda that rotates the JWT key weekly
    └── Makefile      init | plan | deploy | output | destroy
```

Stages are `dev` and `prod`. Each stage lives in its own Terraform workspace, so
state is separate.

## Requirements

- Rust toolchain (`rustup`), `cargo-lambda`, `zig` (used to cross-compile for Lambda)
- Node 20+ (for the frontend)
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
JWT_SECRET_ID=omoffice/jwt-key-dev
```

Then ship the code:

```sh
cd ../backend
make deploy dev
curl "$API_URL"               # {"message":"hello world!"}
```

`make output dev` in `infra/` reprints the values at any time.

## Frontend

The same two-step split as the backend: terraform creates the bucket and the
CloudFront distribution, `make deploy` uploads the built assets.

```sh
cd frontend
make init
make run local                # vite on :5173, calls http://localhost:8000
make run dev                  # same UI, calls the deployed dev API
make deploy dev               # build + sync to S3 + invalidate CloudFront
```

Stages are `local`, `dev` and `prod`; a bare `make run` is `local`. Each stage
reads `frontend/.env.<stage>`, which the Makefile sources and turns into
`VITE_API_URL` — Vite's own `.env` loading is not used, because Vite reads
`.env.local` in *every* mode and would clobber `make run dev`. `local` ships in
the repo pointing at `localhost:8000`; `dev` and `prod` come from
`cd infra && make output <stage>`:

```
API_URL=https://xxxxxxxxxx.execute-api.us-east-1.amazonaws.com/
FRONTEND_BUCKET=omoffice-frontend-dev-123456789012
FRONTEND_DISTRIBUTION_ID=E1XXXXXXXXXXXX
FRONTEND_URL=https://dxxxxxxxxxxxxx.cloudfront.net
```

`make deploy local` is refused — only `dev` and `prod` have infrastructure.

### RouteTester

The page is one `<RouteTester>` per route. It works out its own form: `{...}`
placeholders in the route become path inputs, `fields` becomes the JSON body,
and `onResult` receives the parsed body of a successful response — which is how
the token reaches the protected routes.

```jsx
<RouteTester route="/health" />
<RouteTester route="/auth/request-key/{email}" onResult={r => setToken(r.token)} />
<RouteTester route="/private/me" token={token} />
<RouteTester
  method="POST"
  route="/private/sample-post/{var}"
  fields={{ name: "string", male: "boolean" }}
  token={token}
/>
```

Field types are `"string"`, `"boolean"` (a checkbox) and `"number"`; values are
coerced back to those types before the body is sent.

The API answers any origin with CORS, since it holds no cookies and the bearer
token is the only thing that grants access.

## Meetings

Amazon Chime SDK meetings, behind the same bearer token as everything else
under `/private`:

```sh
curl -X POST "$API_URL/private/meetings" -H "Authorization: Bearer $TOKEN"
# {"code":"a6ff7e2e-...","created_by":"dev@omoffice.example","Meeting":{...}}

curl -X POST "$API_URL/private/meetings/$CODE/attendees" \
  -H "Authorization: Bearer $TOKEN"
# {"code":"a6ff7e2e-...","Meeting":{...},"Attendee":{...}}
```

The meeting code *is* the Chime `MeetingId`; nothing is stored on our side, and
joining is a `GetMeeting` plus a `CreateAttendee`. A code stops working once the
meeting ends — Chime drops a meeting nobody is in — which comes back as
`404 meeting_not_found`.

`Meeting` and `Attendee` are keyed the way the browser SDK's
`MeetingSessionConfiguration` reads them, so the frontend hands the response
straight to it. The meeting panel on the page does exactly that: **create
meeting** shows the code, and the join form starts the real audio/video session
(mute, camera and leave included). The browser needs microphone permission and a
secure origin — `localhost` or the CloudFront URL, not a plain-HTTP host.

The media region is `CHIME_MEDIA_REGION`, else the region the AWS SDK resolved.
The lambda's IAM policy for these calls is `infra/chime.tf`.

## Auth

Routes under a protected prefix need a bearer token. Both the prefixes and the
email whitelist are declared next to the routes, in `app()` in `backend/src/main.rs`:

```rust
let auth = AuthConfig::new(keys)
    // Every route under these prefixes needs a valid bearer token.
    .protect(vec!["/private"])
    // Only these emails may ask for a token.
    .allow_emails(vec!["admin@omoffice.example", "dev@omoffice.example"])
    .build();
```

Adding `"/admin"` to `.protect(...)` protects `/admin` and everything under it.
Nothing else changes: the middleware is already layered over the whole router.

Get a token — only whitelisted emails are accepted:

```sh
curl "$API_URL/auth/request-key/dev@omoffice.example"
# {"token":"eyJ...","token_type":"Bearer","email":"dev@omoffice.example",
#  "expires_at":1756900000,"expires_in":172800}

curl -H "Authorization: Bearer $TOKEN" "$API_URL/private/me"
# {"email":"dev@omoffice.example","expires_at":1756900000}

curl -X POST "$API_URL/private/sample-post/hello" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"name":"ada","male":false}'
# {"received":{"var":"hello","name":"ada","male":false},"by":"dev@omoffice.example"}
```

Tokens are HS256 and last 48h. Failures come back as JSON with a stable slug:

| Case | Status | `error` |
|---|---|---|
| no/!Bearer `Authorization` header | 401 | `missing_token` |
| malformed or wrongly signed token | 401 | `invalid_token` |
| past its `exp` | 401 | `expired_token` |
| email not whitelisted | 403 | `email_not_allowed` |
| signing key unreadable | 500 | `key_unavailable` |

### The signing key

Terraform creates a Secrets Manager secret (`omoffice/jwt-key-<stage>`) and a
small Python lambda that rotates it every Sunday at 07:00 UTC
(`jwt_rotation_schedule` in `infra/variables.tf`). The API verifies against both
`AWSCURRENT` and `AWSPREVIOUS`, so a token issued the day before a rotation is
still good for its full 48h. Keys are cached in the lambda for 5 minutes.

The API picks its key in this order:

1. `JWT_KEY` — a fixed key, for tests and offline work.
2. `JWT_SECRET_ID` — the rotated secret. Terraform sets this on the deployed
   lambda; `make run` picks it up from `backend/.env.<stage>` and reads it with
   your AWS profile.
3. Neither: local runs fall back to a built-in **insecure** key and log a
   warning. In Lambda this is a startup error instead.

To force a rotation out of schedule:

```sh
aws secretsmanager rotate-secret --secret-id omoffice/jwt-key-dev
```

## Teardown

```sh
cd infra
make destroy dev              # prompts for confirmation
```

## Notes

- Lambda architecture is `arm64` on both sides. To switch, change
  `lambda_architecture` in `infra/envs/<stage>.tfvars` and `ARCH` in
  `backend/Makefile` together.
- `/auth/request-key/{email}` hands out a token to anyone who names a
  whitelisted address — there is no proof of ownership yet. Fine for a sample,
  not for real users; mail the token instead of returning it before this is
  exposed to anything that matters.
- Terraform state is local (`infra/terraform.tfstate.d/`). Move it to an S3
  backend before more than one person deploys.
- The frontend bucket is named with the account id appended, because bucket
  names are globally unique. It is private; only CloudFront can read it.
