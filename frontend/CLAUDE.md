# frontend/

React + Vite + Tailwind. See the root `CLAUDE.md` for stages and the infra handoff.

## Commands

```sh
make run [stage]     # http://localhost:5173, talks to the given stage's API (local/dev/prod)
make build [stage] | make preview [stage] | make lint
```

Stage config comes from `frontend/.env.<stage>`, sourced into Vite's `process.env`.

## Architecture

`src/` mirrors the backend's `core/` + `features/` split:

```
main.jsx       mounts app/App.jsx + app/index.css
app/           composition root: App.jsx (providers + route table), NavContainer.jsx, index.css
core/          cross-feature building blocks: api.js, jwt.js, messenger.js, Router.jsx,
               auth/, theme/, ui/ (AsyncButton, Spinner, Avatar), hooks/ (useViewport)
features/      org/, avatars/, meetings/, map/, notifications/ -- each with an index.js
pages/         thin route shells: Home, Login, InPage, Profile, Org, OrgNew
```

### Import rules

Enforced by `no-restricted-imports` patterns in `eslint.config.js`:

- `core/` imports only `core/`.
- `features/X` imports `core/`, and another feature only through `features/Y/index.js` -- never a file inside it. Inside one feature, relative imports.
- `pages/` imports `core/` and feature `index.js` files.
- Only `app/` imports `pages/`.
- A feature's `index.js` re-exports only what's used outside the feature.

Feature dependencies run one way: `meetings -> org`, `map -> meetings, org`, `notifications -> org, meetings`, and `org`/`meetings`/`map` -> `avatars`. Nothing points back at a dependent -- in particular `org` never imports `meetings`: the meeting session lives in its own `MeetingSessionProvider` (nested inside `OrgSessionProvider` in `App.jsx`), and `OrgOnlineView` takes it as a `meetingSession` prop from whoever renders it. Likewise `meetings` never imports `map`: the map follows explicit joins/leaves through the session's `onIntent`.

### Pieces

- `core/Router.jsx` -- no dependency: History API + context, a handful of top-level pages only. Lives in `core/` (not `app/`) because `AuthContext` and every page navigate with it.
- `core/api.js` -- fetch wrapper: attaches the bearer token, turns non-2xx into `ApiError{status, code}` from the backend's `{error, message}` body. A 401 fires `UNAUTHORIZED_EVENT` (`AuthContext` listens and bounces to `/login`) rather than each caller handling it; a 403 (wrong role) is left for the caller.
- `core/messenger.js` -- browser side of `core::communication::web_messenger`.
- `core/auth/AuthContext.jsx` -- the JWT/session context.
- `core/theme/ThemeContext.jsx` -- system/light/dark, toggled by a `.dark` class on `<html>`; the whole palette lives in `app/index.css` as CSS variables, so components only ever use semantic Tailwind utilities (`bg-surface`, `text-ink-muted`), never a raw color.
- `features/org/` -- `OrgSessionContext` (which org is "current" plus its roster; shared by every authenticated page and by `NavContainer`'s sidebar rather than each fetching its own copy), the `/rbac/*` calls and role names (`api.js`), `OrgOnlineView` (roster grouped by meeting/offline), `UsersManagement` (`/org`'s member table + invite form).
- `features/meetings/` -- `MeetingSessionContext` (one `useMeetingSession` per org, shared), `useMeetingSession` (mirrors the backend's idle-meeting rule locally, see `pending_invites` on a listed meeting, so a lone tab leaves proactively instead of waiting to be swept), `chime.js`, `dock/` (`MeetingDock`: persistent floating/draggable on desktop, half-screen/fullscreen toggle on mobile via `MeetingLayoutContext`), `panel/` (`MeetingPanel`: tiles + chat + controls).
- `features/map/` -- `MapModeContext` picks the `/in` layout (map canvas vs. plain roster; unrelated to theme), `MapCanvas`, `useMapSession`/`useMapProximity`. Joining/leaving a meeting from the roster or dock moves the avatar in either layout (`relocation.js`): the open map walks it there itself, and with no map open `MapRelocationProvider` (inside `MeetingSessionProvider`) sends that one move without following anyone's live position. `realtime.js` is the browser side of `features::map::realtime`: live map moves go straight to an AppSync Events API (authorized by the crate's separate `map-authorizer` Lambda), not through the API Lambda; the messenger's `map_move` is only the local fallback.
- `features/notifications/` -- `NotificationsOverlay`: org invites, meeting invites and access requests as floating bubbles.
- `features/avatars/` -- avatar upload/crop and `avatarUrl`.
- `app/NavContainer.jsx` -- persistent chrome for every authenticated page: a collapsible sidebar (desktop) or bottom/side bar (mobile, via `core/hooks/useViewport.js`).
