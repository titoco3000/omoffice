# backend/

Rust crate (axum). One binary: local HTTP server + Lambda handler. See the root `CLAUDE.md` for stages and the infra handoff.

## Commands

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

## Architecture

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
