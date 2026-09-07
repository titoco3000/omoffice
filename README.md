# omoffice

Sample API. Rust backend on AWS Lambda behind an API Gateway HTTP API, with a
React frontend on S3 + CloudFront, an Aurora DSQL database, and all AWS
resources managed by Terraform.

```
.
├── backend/          Rust crate (axum). One binary: local HTTP server + Lambda handler.
│   ├── src/main.rs   routes, auth policy, local/Lambda entrypoint
│   ├── src/migrate/  the migration runner (`omoffice-api migrate ...`)
│   ├── src/auth/     JWT issuing, key lookup, protected-route middleware
│   ├── src/rbac/     organizations, membership, roles, org invites
│   ├── src/chat/     per-org chat room (no history)
│   ├── src/messenger/ outbound delivery to a user: websocket push, email stub
│   ├── src/users/    the `users` table: who has asked for a key
│   ├── src/meetings/ the `meetings` table + routes: Chime plus a history row
│   ├── src/core/     low-level only, no routes: db.rs (DSQL pool, IAM tokens),
│   │                 chime.rs (Chime client), orm/ (schema-as-data + diffing)
│   ├── migrations/   ordered <version>_<name>.up.sql / .down.sql pairs
│   └── Makefile      init | run | build | deploy | test | fmt | lint | db-*
├── frontend/         React + Vite + Tailwind. Orgs, chat, meetings, invites.
│   ├── src/components/RouteTester.jsx   a form + a result box for one route
│   ├── src/components/MeetingPanel.jsx  create/join a meeting, run the media session
│   ├── src/components/Organizations.jsx / OrgDetail.jsx   orgs, roster, invites
│   ├── src/components/ChatPanel.jsx     the selected org's chat room
│   └── Makefile      init | run | build | deploy | preview | lint
└── infra/            Terraform stack (Lambda, API Gateway, IAM, CloudWatch,
    │                 Secrets Manager, S3, CloudFront)
    ├── envs/         per-stage tfvars
    ├── dsql.tf       the Aurora DSQL cluster and the lambda's connect rights
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
- `psql` 14+ (used by `make db-shell`)

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
make run dev                  # http://localhost:8000, reloads on src/ edits
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
DATABASE_ENDPOINT=xxxxxxxxxxxxxxxxxxxxxxxxxx.dsql.us-east-1.on.aws
```

Then create the schema — the cluster terraform made is empty:

```sh
cd ../backend
make db-migrate dev
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

Routes under a protected prefix need a bearer token:

```sh
curl "$API_URL/auth/request-key/dev@omoffice.example"
# {"token":"eyJ...","token_type":"Bearer","email":"dev@omoffice.example",
#  "expires_at":1757500000,"expires_in":604800}

curl -H "Authorization: Bearer $TOKEN" "$API_URL/private/me"
# {"email":"dev@omoffice.example","expires_at":1757500000}
```

Tokens are HS256 and last 7 days. There is no email whitelist and no proof of
ownership — fine for a sample, not for real users. Failures come back as JSON
with a stable slug (`missing_token`, `invalid_token`, `expired_token`,
`key_unavailable`). See CLAUDE.md for the auth policy/`AuthConfig` shape and
the signing-key rotation.

## Database

Aurora DSQL, one cluster per stage. It is serverless, scales to zero, has a
public endpoint (so the lambda stays out of a VPC and there is no NAT gateway)
and authenticates with short-lived IAM tokens instead of a password — there is
nothing to store and nothing to rotate.

**There is no local database.** No emulator, no container, no compose file.
Local runs talk to the real `dev` cluster, exactly like `make run dev` already
reads the real per-stage JWT secret with your AWS profile.

```sh
cd backend
make db-status dev            # what is applied, what is not, what was interrupted
make db-migrate dev           # apply everything pending
make db-revert dev            # undo the most recent one (prompts; can lose data)
make db-shell dev             # psql, with a token generated on the spot
```

Migrations are ordered `<version>_<name>.up.sql`/`.down.sql` pairs in
`backend/migrations/`, applied by `omoffice-api migrate` (a subcommand of the
API binary, so it shares the connection code). `backend/src/schema.rs` is the
single source of truth for the current shape of every table — edit it, run
`make db-generate` to write the migration files, review, then `make db-migrate
<stage>`. See CLAUDE.md for the migration ledger's crash-safety states, DSQL's
postgres-compatibility subset, and why the queries aren't compile-time checked.

## Teardown

```sh
cd infra
make destroy dev              # prompts for confirmation
```

## Notes

- Lambda architecture is `arm64` on both sides. To switch, change
  `lambda_architecture` in `infra/envs/<stage>.tfvars` and `ARCH` in
  `backend/Makefile` together.
- `/auth/request-key/{email}` hands out a token to any email named, with no
  proof of ownership. Fine for a sample, not for real users; mail the token
  instead of returning it before this is exposed to anything that matters.
- Terraform state is local (`infra/terraform.tfstate.d/`). Move it to an S3
  backend before more than one person deploys.
- The frontend bucket is named with the account id appended, because bucket
  names are globally unique. It is private; only CloudFront can read it.
- The `dev` DSQL cluster is created with `force_destroy`, so `make destroy dev`
  takes it and its data with it. `prod` has deletion protection instead.
- The API connects as the built-in `admin` database role. A real deployment
  would create a database role per caller and grant `dsql:DbConnect` against it.
