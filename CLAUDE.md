# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Omoffice: meetings app for organizations. Rust (axum) backend deployed as both a
local server and a Lambda handler from the same router, React/Vite/Tailwind
frontend, Terraform-managed AWS infra (Lambda, API Gateway incl. a WebSocket
API, Aurora DSQL, S3/CloudFront, Secrets Manager). Stages are `dev` and `prod`
(`local` too, for the frontend, meaning "talk to localhost:8000").

## Commands

Each of `backend/`, `frontend/`, `infra/` is driven by its own Makefile; `make
help` in any of them lists the full target set. The stage is a bare second
word (`make run dev`, `make deploy prod`) — `backend`/`infra` default to `dev`,
`frontend` to `local`. All three source AWS_PROFILE/AWS_REGION/PROJECT from
the repo-root `.env` automatically.

```sh
cd backend && make run dev    # http://localhost:8000
cd frontend && make run local # http://localhost:5173
```

Backend: `make build [stage]` (cross-compile lambda zip via cargo-lambda),
`make deploy [stage]` (build + push zip only — never touches infra), 
`make test | fmt | lint` (lint is `clippy --all-targets -- -D warnings`). 
Single test: `cd backend && cargo test <test_name>`.

Backend DB (Aurora DSQL, no local emulator — `dev` is a real cluster):
edit `backend/src/schema.rs` → `make db-generate` (diffs it against
`migrations/.snapshot.json`, writes new `.up.sql`/`.down.sql` — local, no
stage, no DB touched) → review the generated SQL → `make db-migrate <stage>`.
Also `make db-status <stage>`, `make db-revert <stage>` (destructive, asks
first), `make db-shell <stage>` (psql with a fresh IAM token). A test in
`schema.rs` fails the build if `TABLES` was edited without a matching
`make db-generate` run.

Frontend: `make build [stage]` (into `dist/`), `make deploy [stage]` (build +
S3 sync + CloudFront invalidation, `dev`/`prod` only), `make preview [stage]`,
`make lint`. No test runner is configured.

Infra: `make plan <stage>`, `make deploy <stage>` (apply, then print
outputs), `make output <stage>`, `make destroy <stage>` (asks first). State
is local (`infra/terraform.tfstate.d/`, gitignored). `make deploy`/`make
output` print a `backend_env_file` / `frontend_env_file` block — copy those
into `backend/.env.<stage>` / `frontend/.env.<stage>` by hand. That handoff
is manual on purpose: Terraform owns infrastructure, each app's own `make
deploy` owns code/assets, so applying infra never overwrites what's running.

## Backend architecture (`backend/src/`)

`main.rs::app()` builds one `axum::Router` used both by `axum::serve`
(local) and `lambda_http::run` (Lambda) — same handlers, same middleware,
either way. Route list, CORS, and the auth middleware layering all live
there; read it first when adding a route.

- `core/` — building blocks with no feature knowledge: `db` (DSQL), `chime`
  (Amazon Chime SDK client), `apigw_management` (push to WebSocket
  connections when deployed), `orm` (schema-as-Rust-data, SQL rendering,
  diffing — see the DB workflow above), `messenger` (below).
- `auth/` — JWT bearer auth. `AuthConfig` in `main.rs::app()` decides which
  path prefixes are protected; `keys.rs` holds the signing key.
- `rbac/` — organizations and per-user roles. `require_role`/`require_member`
  are the gates other modules call. Roles native to the RBAC live in
  `rbac::roles`; a project's own extra roles (this project has one: `"post"`,
  in `main.rs`) are plain strings registered via `RolesConfig::project_roles`.
- `users/`, `meetings/`, `chat/`, `presence/` — feature modules, each owning
  its own routes/queries. `meetings/` is the only module that knows both
  Chime and DSQL for a meeting.
- `migrate/` — the `migrate` subcommand (`generate`/`up`/`down`/`status`),
  invoked via `cargo run -- migrate ...`; `main.rs` dispatches to it before
  starting the server when `argv[1] == "migrate"`.
- `schema.rs` — every table's current shape, in one place. The only file to
  edit to change the schema (see DB workflow above).

**Messenger** (`core/messenger/`): one interface, two directions. Out:
`Messenger::send`/`send_to_users_on_org`/`send_to_users_on_meeting` take a
JSON message, a channel name, transports (`Via::Websocket`/`Via::Email`) and
an audience. In: `Messenger::on_message(channel, handler)` registers a
handler called once per frame a client sends on that channel — `chat/`,
`presence/`, and `rbac/invites.rs` each register their own. The wire frame is
`{"channel": ..., "message": ...}` in both directions over the one `/ws`
socket (not under `/private` — a handshake carries no header, so it
authenticates itself with `?token=...`). Delivery goes through
`active_users` and whichever transport each row is on: the in-process
`websocket::Hub` locally, API Gateway's Management API when deployed
(`core/messenger::lambda_entry` is the router's fallback, catching a
WebSocket-API event since it claims no real HTTP route path).

`presence/` decides who may join an org's "room": a browser reports its
current org/meeting on the `presence` channel, presence checks it against
RBAC and stores it, and everything else (`chat/`'s room, who a broadcast
reaches) trusts that stored context rather than re-checking. Presence
snapshots are pushed on change (`org_presence` channel), not polled; the
`GET /private/orgs/{id}/presence` route only covers a tab's first paint.

## Frontend architecture (`frontend/src/`)

No router or state library — `router.jsx` is a from-scratch History-API
router (5 routes, `matchPath("/meeting/:code", path)` pattern matching) kept
deliberately dependency-free because the bundle is already over budget from
the Chime SDK. `App.jsx`'s provider nesting order is the dependency order —
read the comment there before reordering:
`Theme → Router → Auth → InviteLink → Messenger → Org → Meeting → Shell`.
`InviteLink` accepts `?invite=org:{id}` / `?invite=meeting:{id}` magic-link
query params on mount (calling the matching `accept` route) before rendering
anything else. The meeting is held in a provider above the router
(`meeting/MeetingContext.jsx`) so navigating away from `/meeting/{code}`
doesn't end the call — `MeetingDock`, mounted once at the app root beside the
routed content, owns the actual Chime session and covers whatever `slot`
the current page registered as its stage.

`messenger/messenger.js`'s `Messenger` class mirrors the backend: one
socket, opened by `MessengerContext` for the session's token, with
`CHANNELS` (`chat`/`invites`/`presence`) matching the backend's channel
names exactly. No component below `MessengerContext` opens a socket of its
own or knows about it directly — a feature just names a channel. `Shell`
calls `usePresence({ org: selectedOrgId })` once, so the org a tab is
"looking at" (and thus its chat room) is whatever `OrgContext` currently has
selected.

`api.js::callApi` always resolves to the same `{ok, status, ms, data}`
shape (including on a network/CORS failure, as `status: 0`) so a failed
request and a 401 render the same way. `rbac.js` mirrors the backend's role
logic client-side (`hasRole`, `admin` implying `team-management`/`billing`)
to gate UI — a defense against a confusing click, not the actual
authorization boundary, which the backend still enforces.
