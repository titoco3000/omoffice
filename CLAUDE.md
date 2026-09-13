# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Omoffice: meetings app for organizations. Three parts, each with its own Makefile:

```
backend/    Rust crate (axum). One binary: local HTTP server + Lambda handler.
frontend/   React + Vite + Tailwind.
infra/      Terraform stack (Lambda, API Gateway, IAM, CloudWatch, Secrets Manager, S3, CloudFront)
old-backend/  pre-overhaul backend, kept for reference only -- not built or run.
```

Stages are `dev` and `prod`, each its own Terraform workspace (separate state). Most `make` targets take the stage as a bare second word: `make run dev`, `make deploy prod`. `backend`/`infra` default to `dev`; `frontend` always runs as `local` (talks to `localhost:8000`) regardless of what stage the backend is on.

Root `.env` holds `AWS_PROFILE`, `AWS_REGION`, `PROJECT` and is sourced by every sub-Makefile automatically.

## Commands

```sh
make init             # init in backend, frontend, infra
make run [stage]      # backend (dev/prod) + frontend (local) together, ctrl+c stops both
make fmt              # fmt in backend, frontend, infra
```

### backend/

```sh
make run [stage]      # http://localhost:8000, reloads on src edits (cargo-watch; WATCH=0 to disable)
make test | fmt | lint   # cargo test / fmt / clippy --all-targets -D warnings
cargo test --lib some_test_name   # single test, headless (no socket, no binary)
```

Database (Aurora DSQL, no local emulator -- every DB command needs `backend/.env.<stage>`, produced by `infra`'s `make output`):

```sh
make db-generate         # diff schema.rs against migrations/.snapshot.json, write new migration (local, no stage, no DB)
make db-status [stage]
make db-migrate [stage]
make db-revert [stage]   # destructive, asks for confirmation
make db-shell [stage]    # psql with a fresh IAM token
```

Schema workflow: edit `backend/lib/schema.rs` -> `make db-generate` -> review the generated `.up.sql`/`.down.sql` in `backend/migrations/` -> `make db-migrate <stage>`.

### frontend/

```sh
make run [stage]     # http://localhost:5173, talks to the given stage's API (local/dev/prod)
make build [stage] | make preview [stage] | make lint
```
Stage config comes from `frontend/.env.<stage>`, sourced into Vite's `process.env`.

### infra/

```sh
make plan|deploy|output|destroy [stage]
```
`deploy`/`output` print a `backend_env_file` and `frontend_env_file` block -- copy those into `backend/.env.<stage>` / `frontend/.env.<stage>` by hand. That handoff is manual on purpose: infra owns infrastructure, each app's own `make deploy` owns code/assets, so applying infra never overwrites what's running. Terraform state is local (`infra/terraform.tfstate.d/`, gitignored).

## Backend architecture

`backend/main.rs` is a thin binary: parses `migrate <status|up|down|generate>` as a special case, otherwise builds shared state (`Db`, `ChimeState`, `Messenger`), calls each feature's own `register()` to wire its channels, and serves `router::app`. Everything else lives in the `omoffice_api` library crate under `backend/lib/` (`backend/lib/lib.rs` is its root), so the whole app is exercisable headless via `cargo test --lib`.

- `core/` -- cross-feature building blocks: `auth` (JWT), `rbac` (orgs/membership/roles), `data::db` (DSQL pool), `data::orm` (schema-driven migrations), `communication` (email, websocket pub/sub), `chime`, `debug`.
- `features/meetings/` -- the one feature module so far: `mod.rs` (create/join/leave/list), `rooms.rs` (Chime room lifecycle), `chat.rs`, `meeting_invites.rs`.
- `schema.rs` -- every table declared as a `static Table`, registered into `orm::TABLES` via `#[distributed_slice]` (from `linkme`) -- no separate list to keep in sync by hand.
- `router.rs` -- mounts `/auth` (public), `/rbac` and `/meeting` (behind `auth::require_bearer`), and `/ws/{user_id}` (auth happens after the socket opens, since a handshake carries no header to check).

**Schema/migrations (`core/data/orm/`)**: `generate()` is pure local codegen -- it diffs every table in `TABLES` against `.snapshot.json` (the shape as of the last run) and writes one `.up.sql`/`.down.sql` pair covering the difference, in dependency order (a table's foreign keys must already exist). It touches no database. `migrate` is the separate piece that actually applies those files against DSQL, via a ledger that marks each migration `pending` then `applied` in two steps -- DSQL can't put DDL and a ledger write in one transaction, so a crash is visible rather than silently half-done.

**DSQL (`core/data/db.rs`)**: authenticated with a SigV4 IAM token, not a stored password -- the pool re-signs before the token goes stale (`TOKEN_REFRESH_AFTER` < `TOKEN_TTL`). DSQL has no row locks (serializable instead) and no cascading deletes: a racing write fails with a conflict rather than corrupting, so every write goes through `retry_on_conflict` (its closure must be safe to re-run from scratch), and anything with children (orgs, meetings) deletes them explicitly before the parent row.

**Auth (`core/auth/`)**: magic-link only, no password. `GET /auth/request-key/{email}` registers/looks up the user, mints a JWT, and "emails" it (`core::communication::email` is a stub) -- there is no route that hands a token back directly; the mailbox is the only way to redeem one. `require_bearer` middleware verifies the bearer token and inserts `Claims` as an extension for downstream handlers.

**RBAC (`core/rbac/`)**: membership (`org_users`) and permission (`org_roles`) are deliberately separate tables -- see the module doc in `rbac/mod.rs` for the full reasoning, including why `admin` implicitly satisfies `team-management`/`billing` checks, why an org can never lose its last admin, and how `org_users.online` is scoped to whichever org a connection last reported as active (via the messenger's `CONNECTION` and `ACTIVE_CHANNEL`) rather than to a user's whole session.

**Messenger (`core/communication/web_messenger/`)**: one websocket per browser tab carries every "channel" (`{"channel": ..., "message": ...}` frames both ways). `Messenger::send(channel, to, message)` pushes to whoever of `to` has an open socket (best-effort); `Messenger::on_message(channel, handler)` registers one handler per channel. Connect/disconnect is itself a channel (`CONNECTION`) rather than a separate presence table. Nothing above this module knows a socket exists.

**Meetings (`features/meetings/`)**: a meeting starts with no room and costs nothing until a second attendee shows up (`join` claims a room via `rooms::assign` only once attendee count reaches two). Below two attendees a meeting is "idle" (`idle_since`); there is no background sweep task -- whoever next touches an idle-past-`IDLE_AFTER_SECONDS` meeting (`list_route`, or `join` on that specific meeting) terminates it lazily. See the doc comment at the top of `features/meetings/mod.rs` for the full state machine (room reconnects, `LIST_CHANNEL` vs `ROOM_CHANNEL` vs `meeting_invites::CHANNEL`).

## Frontend architecture

- `router/Router.jsx` -- no dependency: History API + context, a handful of top-level pages only.
- `lib/api.js` -- fetch wrapper: attaches the bearer token, turns non-2xx into `ApiError{status, code}` from the backend's `{error, message}` body. A 401 fires `UNAUTHORIZED_EVENT` (`AuthContext` listens and bounces to `/login`) rather than each caller handling it; a 403 (wrong role) is left for the caller.
- `lib/messenger.js` -- browser side of `core::communication::web_messenger`.
- `lib/useMeetingSession.js` -- mirrors the backend's idle-meeting rule locally (see `pending_invites` on a listed meeting) so a lone tab leaves proactively instead of waiting to be swept.
- `pages/` top-level routes, `panels/` the dashboard's panel components, `auth/AuthContext.jsx` the JWT/session context.

## Other

`dev-docs/phase/*.md` is a running log of each development phase's original prompt/intent -- check it for *why* something was built a certain way before assuming a design choice is accidental.
