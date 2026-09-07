# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`omoffice` — a sample API: a single Rust (axum) binary deployed to AWS Lambda behind an API Gateway v2 HTTP API, plus a React/Vite/Tailwind frontend on S3 + CloudFront, an Aurora DSQL database, with all AWS resources managed by Terraform. Two deployed stages: `dev` and `prod`.

## Environment

Every command below assumes the root `.env` is exported first (it carries `AWS_PROFILE`, `AWS_REGION`, `PROJECT`):

```sh
export $(grep -v '^#' .env | xargs -d '\n')
```

Every make target except `help` starts with `$(REQUIRE_ENV)`, which fails if
`PROJECT` is unset — that is the signal the root env was never exported.

## Commands

`backend/`, `frontend/` and `infra/` each drive everything through a Makefile that takes the stage as a bare second word (`make deploy dev`); with no stage word it defaults to `dev` (`local` in `frontend/`). The Makefiles declare the stage names as no-op phony targets so make does not try to build them.

Backend (`cd backend`):

```sh
make init            # verify cargo/zig, install cargo-lambda + cargo-watch, add linux targets, cargo fetch
make run dev         # local server on http://localhost:8000, rebuilds+restarts on src edits (PORT overrides, WATCH=0 disables)
make build dev       # cross-compile the lambda zip via cargo-lambda (ARCH, default arm64)
make deploy dev      # build, then aws lambda update-function-code from .env.<stage>
make db-status dev   # which migrations are applied, which are not, which were interrupted
make db-migrate dev  # apply every pending migration
make db-revert dev   # revert the most recent one (destructive; prompts for the stage name)
make db-shell dev    # psql against the stage, with a freshly generated IAM token
make test            # cargo test
make fmt             # cargo fmt
make lint            # cargo clippy --all-targets -- -D warnings
```

Run a single test: `cd backend && cargo test <test_name>` (`-- --nocapture` to see output). Nothing in the build touches a database. Every `db-*` target does, and needs `DATABASE_ENDPOINT` in `backend/.env.<stage>` plus AWS credentials, because there is no local database to fall back to.

Frontend (`cd frontend`):

```sh
make init            # verify node/npm/aws cli, npm install
make run local       # vite on http://localhost:5173 against localhost:8000 (PORT overrides)
make run dev         # same UI against the deployed dev API
make build dev       # production build into dist/
make deploy dev      # build, then s3 sync + cloudfront invalidation from .env.<stage>
make preview dev     # build, then serve dist/ locally
make lint            # eslint
```

Stages here are `local`, `dev`, `prod`; `make deploy local` is refused because only `dev`/`prod` have infrastructure.

Infra (`cd infra`):

```sh
make init            # terraform init + fmt -check + validate
make plan dev
make deploy dev      # apply, then print outputs
make output dev      # reprint outputs, including the blocks for backend/.env.<stage> and frontend/.env.<stage>
make destroy dev     # prompts: type the stage name to confirm
make fmt             # terraform fmt -recursive
```

Every infra target except `init` requires `.terraform/` to exist and selects the stage's terraform workspace (`terraform workspace select -or-create <stage>`) before running.

## Architecture

**One binary, two runtimes.** `backend/src/main.rs` builds a single `axum::Router` in `app()`. `main` checks `AWS_LAMBDA_RUNTIME_API` (set only by the Lambda execution environment): if present it hands the router to `lambda_http::run`, otherwise it binds a local `TcpListener`. Add routes in `app()` and both paths get them.

**Terraform owns infrastructure, the backend owns code.** `infra/main.tf` creates the Lambda from a placeholder zip (`infra/placeholder/bootstrap`) and then `ignore_changes = [filename, source_code_hash]`. The real binary is pushed only by `backend/make deploy`, so a `terraform apply` never reverts deployed code. This is why the two deploy steps exist and must not be merged.

**The frontend is the same two-step split.** `infra/frontend.tf` creates a private S3 bucket (`<project>-frontend-<stage>-<account-id>`, globally-unique names) and a CloudFront distribution reading it through an Origin Access Control; `frontend/make deploy <stage>` uploads `dist/` and invalidates the distribution. Terraform never touches the objects. 403/404 both rewrite to `/index.html` so the SPA owns routing.

**Frontend stage config does not use Vite's `.env` loading.** Vite reads `.env.local` in *every* mode, which would clobber `make run dev`. Instead `frontend/Makefile` sources `frontend/.env.<stage>` and exports `VITE_API_URL`/`VITE_STAGE` into vite's process env — the same shape as `backend/Makefile`. `.env.local` (checked in) points at `localhost:8000`; `.env.dev`/`.env.prod` come from `infra/make output <stage>`, which prints a `frontend_env_file` block (`API_URL`, `FRONTEND_BUCKET`, `FRONTEND_DISTRIBUTION_ID`, `FRONTEND_URL`).

**The database is Aurora DSQL, and there is no local one** (`backend/src/core/db.rs`, `infra/dsql.tf`). One cluster per stage; it is serverless, scales to zero and has a public endpoint, so the lambda stays out of a VPC and the stack needs no NAT gateway. There is no DSQL emulator or container image, so local runs talk to the real `dev` cluster — the same pattern as `make run dev` already reading the real per-stage JWT secret with your AWS profile. Terraform owns the cluster; the schema inside it is owned by `make db-migrate`, the same split as the lambda and its code.

**Connecting is an IAM token, not a password.** Nothing is stored and nothing rotates: `db.rs` SigV4-presigns a `DbConnectAdmin` call locally and uses the result as the postgres password (`infra/dsql.tf` grants `dsql:DbConnectAdmin` on the cluster). Those tokens expire, so a pool cannot keep the one it got at cold start: `Db::pool()` re-signs before the current token ages out and hands the new one to the pool with sqlx's `set_connect_options`. That check is pull-based rather than a background refresh task on purpose — a lambda is frozen between invocations, and a timer that only ticks while the sandbox is awake would hand out an expired token after an idle gap. The pool is built lazily, so a cold start pays for neither the token nor a connection.

**`db.rs` is deliberately dumb.** It hands out a connected pool, retries a write that lost an optimistic-concurrency race, and words a database failure as HTTP — nothing in it knows a table or a column name. Same split on the Chime side: `chime.rs` only knows how to call Chime (`create_meeting`/`get_meeting`/`create_attendee`) and the JSON shapes the browser SDK reads; it does not know meetings get recorded anywhere. `users.rs` and `meetings.rs` are the domain modules that own a table each and, for meetings, use *both* `db.rs` and `chime.rs` to do the thing a meeting actually is — create it in Chime, then record it. Route handlers live in the domain module, not in `chime.rs` or `db.rs`: a base module gaining a route, a query, or a dependency on the other base module is the thing to catch in review.

**Migrations are ordered SQL pairs with a three-step ledger** (`backend/migrations/`, `backend/src/migrate.rs`). Every migration is `<version>_<name>.up.sql` plus a `.down.sql`; the runner is a subcommand of the API binary (`omoffice-api migrate <status|up|down>`), so it shares the connection code and there is still one binary to cross-compile. Normally a migration tool writes its ledger row inside the same transaction as the schema change, so a crash rolls back both. DSQL forbids DDL and DML in one transaction, so that guarantee is unavailable and ordering alone does not replace it — ledger-first leaves a row claiming a migration that never ran, ledger-last leaves a change no row admits to. So the ledger has states: insert `pending`, run the DDL, update to `applied` (and `applied` -> `reverting` -> row deleted on the way back). A crash leaves a row in an in-between state and the next run stops with a named error saying which migration was interrupted. `sqlx`'s own migrator was not used: its `success` flag is written by the same transaction the DDL is in, which is exactly the thing DSQL will not do.

**The queries are built at runtime, and that is forced.** `sqlx::query!` was the plan; it does not work against DSQL. The macro opens its own connection to describe each query at compile time, and `sqlx-macros-core` unconditionally sends a `DO $$ ... $$` block on that connection to set `plan_cache_mode`. DSQL has no PL/pgSQL and answers `ERROR: DO with language plpgsql not supported`, so every `query!` and every `cargo sqlx prepare` fails — online or offline, with or without a cache. There is no setting that turns the block off. So `users.rs`/`meetings.rs` use `sqlx::query`/`query_as::<_, T>` with `.bind(...)`, and there is no `.sqlx/` directory, no `db-prepare` target and no offline mode to think about. The cost is real: a wrong column name is a 500 at runtime rather than a build error, so the tables the queries name are worth re-reading when a migration changes them. Verify with `psql` before assuming a newer sqlx has fixed this; the block is in `sqlx-macros-core/src/database/mod.rs`.

**What is in the database.** `users.rs` keys `users` by the email `/auth/request-key/{email}` already issues tokens for, and `auth.rs` writes a row there (best effort — a token that is already signed should not be withheld because the database is down). `meetings.rs` keys `meetings` by the Chime meeting code from `POST /private/meetings`: code, creator, created_at, with a real foreign key to `users`. That write is not best effort — a meeting missing from the history is worse than an empty Chime meeting, which Chime drops on its own. `GET /private/meetings` reads it back, and `frontend/src/components/MeetingHistory.jsx` is the visible list in the meeting lobby.

**`RouteTester` derives its form from its props** (`frontend/src/components/RouteTester.jsx`): `{placeholders}` in `route` become path inputs, `fields` (`{name: "string" | "boolean" | "number"}`) becomes the JSON body, `onResult` gets the parsed body of a 2xx — that is how the token from `/auth/request-key/{email}` reaches the protected testers in `App.jsx`. `src/api.js` holds the URL/param helpers and `callApi`, which flattens a network or CORS failure into the same `{ok, status, ms, data}` shape as a 401 so there is one render path.

**The handoff between the halves is `backend/.env.<stage>` and `frontend/.env.<stage>`.** `infra/make output <stage>` prints a `backend_env_file` output (`LAMBDA_FUNCTION_NAME`, `API_URL`, `JWT_SECRET_ID`, `DATABASE_ENDPOINT`) that must be pasted into `backend/.env.dev` / `.env.prod`; `backend/make deploy` fails with a pointer to that command if the file, `LAMBDA_FUNCTION_NAME` or `DATABASE_ENDPOINT` is missing. `make run` sources the same file, so local runs read the real per-stage JWT secret with your AWS profile.

**Auth policy is declared in `app()`, implemented in `backend/src/auth/mod.rs`.** `app()` builds an `AuthConfig` naming the protected prefixes (`.protect(vec!["/private"])`) and the email whitelist (`.allow_emails(...)`), so both lists sit beside the routes they govern; `auth.rs` holds no policy of its own. `require_bearer` is layered once over the whole router and checks the prefixes per request, so adding one to `.protect(...)` is the entire cost of protecting a route tree. Verified `Claims` are inserted as a request extension. `/auth/request-key/{email}` signs an HS256 token (48h) and returns it directly — there is no proof of email ownership, which is fine for a sample and not for real users.

**The signing key is a Secrets Manager secret rotated weekly** (`infra/secrets.tf`, `omoffice/jwt-key-<stage>`, `cron(0 7 ? * SUN *)` from `var.jwt_rotation_schedule`). `infra/rotator/index.py` is a standard four-step rotation lambda that terraform owns end to end — unlike the API lambda, its code is not deployed separately. The API verifies against `AWSCURRENT` then `AWSPREVIOUS`, so a 48h token survives a rotation; keys are cached in-process for 5 minutes. `KeySource::from_env()` picks the key once at startup: `JWT_KEY` env var, then `JWT_SECRET_ID`, then a built-in insecure key that is a hard startup error inside Lambda.

**Meetings are Chime plus a history row, and `meetings.rs` is the only module that knows both** (`backend/src/meetings/mod.rs`, `backend/src/core/chime.rs`). `POST /private/meetings` takes an `org_id` — the caller must be a member, checked via `rbac::require_member` before Chime is ever called — then calls `ChimeState::create_meeting` and `Meeting::record`; the Chime `MeetingId` *is* the meeting code, `meetings.rs` never invents one. A meeting is a shared org resource, the same as the org's chat room: `GET /private/meetings?org_id=...` (also membership-gated) lists every meeting that org has, not just the caller's own, so any member can see and rejoin any of them. `schema::MEETINGS.org_id` is nullable only because it was added after rows already existed with nothing to put there — every meeting `create` makes now always has one. `POST /private/meetings/{code}/attendees` is `get_meeting` + `create_attendee`, needs nothing but the code from the database, and a code dies with its meeting (`404 meeting_not_found`) since Chime drops an empty meeting on its own; it also writes a `meeting_attendees` row for the caller, best effort — bookkeeping only (listing runs on org membership, not this), for a meeting reached without an org at all (a `meeting_invites` link, or a bare code) to still have some record the caller was there. Both responses carry `Meeting`/`Attendee` objects keyed the way the browser SDK's `MeetingSessionConfiguration` reads them; the Rust SDK types are not `Serialize`, so `chime::meeting_json`/`chime::attendee_json` spell that shape out and must keep matching what `frontend/src/components/Meeting.jsx` feeds the SDK. `ChimeState` and `Db` reach `meetings.rs`'s handlers as `Extension`s, so `AuthState` stays the router's only `State`. `infra/chime.tf` grants the calls and creates nothing.

**The meeting UI is a roster, not a tile list** (`frontend/src/components/Meeting.jsx`). `MeetingPanel.jsx` is only the lobby — create a meeting, or type a code — and then mounts `<Meeting code token onLeave>`, keyed by the code so a new meeting is a new session. `Meeting` joins on mount and renders one area per attendee the SDK reports *present*, not one per video tile: a camera-less attendee keeps their area and shows a placeholder, and toggling a camera never reshuffles the layout. A screen share arrives as its own attendee (Chime's `#content` modality, read with `DefaultModality`) and so gets its own area, auto-expanded when it starts. `ControlPanel.jsx` is the overlay (settings, mic, camera, share, leave); its two settings — where the panel sits, and where the other participants sit while one is expanded — are the `PANEL_POSITIONS`/`PARTICIPANT_POSITIONS` maps beside it and the `STRIP` map in `Meeting.jsx`. The per-attendee hover controls in `ParticipantTile.jsx` mute or stop the camera of somebody else. Icons are hand-written SVGs in `icons.jsx` rather than a package, because the bundle is already over vite's warning.

**Moderation is a broadcast the target obeys** (`MODERATION_TOPIC` in `Meeting.jsx`). Chime data messages have no per-attendee delivery, so the message carries its `target` and every client but that one drops it; the target mutes itself *at the source*, which is why A muting B is silent for C as well, with nothing enforced per-listener. B's mute badge on the other screens then comes from the ordinary volume indicator, not from the message. There is deliberately no `unmute` action — only B can unmute B — and stopping a camera works the same way. A hostile client could simply ignore the message; real enforcement would mean holding the roster in the backend and calling Chime's server-side mute.

**Video elements are bound, not rendered.** `Meeting` owns `bindVideo(tileId, element)` and a tileId -> element map; `ParticipantTile` only reports the `<video>` it mounted. Both layouts (grid, and expanded + strip) move elements between containers, so React remounts them and the tile rebinds — the map is what stops a plain re-render from rebinding an already-bound element.

**CORS is wide open, on purpose** (`cors()` in `backend/src/main.rs`). The API carries no cookies, so the bearer token is the only thing granting access and any origin may call it. The layer sits *outside* the auth middleware, or the browser's unauthenticated `OPTIONS` preflight for a `/private` route would be answered with a 401.

**Stage isolation is by terraform workspace**, not by directory: one root module in `infra/`, per-stage `envs/<stage>.tfvars` (currently only `stage = "..."`), separate state per workspace. Resource names are derived in `locals` as `<project>-api-<stage>` etc.

**State is local** (`infra/terraform.tfstate.d/`, gitignored). Move to an S3 backend before a second person deploys.

## Constraints

- Lambda architecture must match on both sides: `lambda_architecture` in `infra/envs/<stage>.tfvars` (default `arm64` from `variables.tf`) and `ARCH` in `backend/Makefile`. Change them together.
- `var.stage` is validated against `["dev", "prod"]`; adding a stage means touching `variables.tf`, a new `envs/*.tfvars`, and the Makefiles' phony stage targets.
- The API Gateway route is `$default` on a `$default` auto-deploy stage — all paths reach the Lambda, so routing is entirely in the axum router.
- `jsonwebtoken` has no default crypto provider: the `rust_crypto` feature is what supplies one. The AWS SDK deps are pinned to the rustls/ring TLS backend (`legacy-client` / `rustls`) because the default `aws-lc-rs` needs cmake and a C toolchain that the zig cross-build does not have. Changing either feature set breaks `make build`.
- The dev secret has `recovery_window_in_days = 0` so `make destroy` frees the name immediately instead of holding it for 30 days.
- `frontend/make deploy` gives hashed assets a one-year immutable cache and `index.html` `no-cache`, then invalidates `/*`. Uploading `index.html` with a long TTL would serve a stale page pointing at deleted asset hashes.
- `amazon-chime-sdk-js` probes for browser APIs through node's `global` (`'document' in global` in its `CSPMonitor`, reached while a meeting session is constructed), so `frontend/vite.config.js` maps `global` to `globalThis` in *both* `define` (the build) and `optimizeDeps.esbuildOptions.define` (the dependency vite pre-bundles for `vite dev`). Dropping either brings back `ReferenceError: global is not defined` on join.
- `amazon-chime-sdk-js` dominates the frontend bundle (~1.5 MB raw, ~374 kB gzipped), which is over vite's chunk-size warning. Code-split it before adding anything else large.
- Write DSQL, not postgres. The compatibility list moves — check the current docs before designing a schema — but as of September 2026: one database (`postgres`) per cluster, no views, no triggers, no extensions, no PL/pgSQL, no temporary tables, no `TRUNCATE`; foreign keys work (since August 2026) and so do sequences and identity columns, though UUID or otherwise random primary keys are still what spreads writes across the cluster. Indexes are `CREATE INDEX ASYNC` and build in the background. A primary key is fixed at `CREATE TABLE` and cannot be added later. DDL and DML need separate transactions, one DDL statement per transaction, and 3,000 modified rows per transaction. Isolation is `Repeatable Read`, concurrency is optimistic: a write can fail at commit with SQLSTATE `40001`, which is why writes go through `db::retry_on_conflict`. Connections are closed after an hour.
- There are no indexes yet, on purpose: there is no data to speak of, so they buy nothing and their absence keeps the first migrations simple.
- One statement per migration file is the shipped convention (a test asserts it) — the runner splits files on top-level semicolons and runs the statements one at a time, since DSQL allows one DDL statement per transaction.
- The release profile is size-tuned (`opt-level = "z"`, `lto`, `strip`, `panic = "abort"`); `panic = "abort"` means no unwinding, so avoid code that relies on catching panics.

## dev-docs

`dev-docs/phase/<N>/prompt.md` holds the original feature prompts per phase, in order. Phase 1 (and point releases like 1.1) is the current implementation; later phases are the planned direction. Phases 1, 1.1, 2 (JWT auth), 3 (frontend), 4 (Chime meetings) and 6 (Aurora DSQL) are implemented.
