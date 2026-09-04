# omoffice

Sample API. Rust backend on AWS Lambda behind an API Gateway HTTP API, with a
React frontend on S3 + CloudFront, an Aurora DSQL database, and all AWS
resources managed by Terraform.

```
.
├── backend/          Rust crate (axum). One binary: local HTTP server + Lambda handler.
│   ├── src/main.rs   routes, auth policy, local/Lambda entrypoint
│   ├── src/auth.rs   JWT issuing, key lookup, protected-route middleware
│   ├── src/chime.rs  Chime SDK meetings: create a meeting, join one by code
│   ├── src/db.rs     Aurora DSQL pool, IAM auth tokens, the queries
│   ├── src/migrate.rs  the migration runner (`omoffice-api migrate ...`)
│   ├── migrations/   ordered <version>_<name>.up.sql / .down.sql pairs
│   └── Makefile      init | run | build | deploy | test | fmt | lint | db-*
├── frontend/         React + Vite + Tailwind. A meeting panel over RouteTester cards.
│   ├── src/components/RouteTester.jsx   a form + a result box for one route
│   ├── src/components/MeetingPanel.jsx  create/join a meeting, run the media session
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

### Migrations

Ordered pairs of files in `backend/migrations/`, both halves required:

```
0001_users.up.sql      0001_users.down.sql
0002_meetings.up.sql   0002_meetings.down.sql
```

The runner is a subcommand of the API binary (`omoffice-api migrate up`), so it
shares the connection code with the server.

A normal migration tool writes its ledger row in the same transaction as the
schema change, so a crash can only roll back both. DSQL does not allow DDL and
DML in one transaction, so that is not available here, and ordering alone does
not replace it: ledger-first and a crash leaves a row claiming a migration that
never ran; ledger-last and a crash leaves a change no row admits to. So each
migration moves through three steps and two states:

1. insert the ledger row as `pending`
2. run the DDL
3. update the row to `applied`

Reverting is the mirror (`applied` -> `reverting` -> row deleted). A crash
anywhere leaves a row mid-state, and the next run refuses to move:

```
error: migration 0002 (meetings) was interrupted while pending.
The database may or may not carry its change, so nothing further will run.
```

### Writing DSQL, not postgres

DSQL supports a subset of postgres and the subset moves — check the current
compatibility docs before designing a schema. As of September 2026: one database
(`postgres`) per cluster, no views, triggers, extensions, PL/pgSQL or temporary
tables; foreign keys work, indexes are `CREATE INDEX ASYNC`, a primary key is
fixed at `CREATE TABLE`, DDL and DML need separate transactions (one DDL each),
and concurrency is optimistic — a write can fail at commit with SQLSTATE 40001,
so writes retry.

There are no indexes yet on purpose: there is no data to speak of, so they would
buy nothing.

### Why the queries are not compile-time checked

`sqlx::query!` checks SQL against the real schema while the crate compiles,
which is what this project wanted. It cannot be used against Aurora DSQL.

The macro opens its own connection to describe each query, and the first thing
`sqlx-macros-core` sends on it is a `DO $$ ... $$` block that sets
`plan_cache_mode`. DSQL has no PL/pgSQL:

```
$ psql ... -c 'DO $$ BEGIN END $$;'
ERROR:  DO with language plpgsql not supported
```

So every `query!` fails the build, and `cargo sqlx prepare` fails for the same
reason — which also rules out the `.sqlx` offline cache, since there is no way
to generate one. Nothing about it is configurable; the block is unconditional.

`backend/src/db.rs` therefore uses runtime `sqlx::query` / `query_as` with
`.bind(...)`. Nothing to prepare, nothing to commit, no offline mode — and no
compile-time check either: a column that a migration renamed becomes a 500 at
runtime instead of a build error.

### The schema

```sql
users     (email PRIMARY KEY, created_at)
meetings  (code PRIMARY KEY, created_by -> users.email, created_at)
```

A row lands in `users` when someone asks for a key, and in `meetings` when
`POST /private/meetings` creates one. `GET /private/meetings` reads back the
meetings the caller created:

```sh
curl "$API_URL/private/meetings" -H "Authorization: Bearer $TOKEN"
# {"meetings":[{"code":"a6ff...","created_by":"dev@omoffice.example",
#               "created_at":"2026-09-04T10:11:12Z"}]}
```

The meeting lobby on the page shows the same list, with a **rejoin** button per
row. Chime forgets a meeting as soon as it empties, so a code from the list may
answer `404 meeting_not_found` — the row is history, not state.

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
- The `dev` DSQL cluster is created with `force_destroy`, so `make destroy dev`
  takes it and its data with it. `prod` has deletion protection instead.
- The API connects as the built-in `admin` database role. A real deployment
  would create a database role per caller and grant `dsql:DbConnect` against it.
