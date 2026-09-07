import { useEffect, useState } from "react";
import { AuthProvider, useAuth } from "./auth/AuthContext";
import ChatPanel from "./components/ChatPanel";
import Login from "./components/Login";
import MeetingPanel from "./components/MeetingPanel";
import Organizations from "./components/Organizations";
import OrgSwitcher from "./components/OrgSwitcher";
import PendingInvites from "./components/PendingInvites";
import RouteTester from "./components/RouteTester";
import Toast from "./components/Toast";
import { useInviteActions } from "./hooks/useInviteActions";
import { useNotifications } from "./hooks/useNotifications";
import { MessengerProvider, usePresence } from "./messenger/MessengerContext";
import { OrgProvider, useOrgContext } from "./org/OrgContext";
import { API_URL, callApi, describeFailure, STAGE } from "./api";

const TABS = [
  { id: "meetings", label: "meetings" },
  { id: "chat", label: "chat" },
  { id: "orgs", label: "organizations" },
];

/// Top-level shell: `AuthProvider` owns the token. Logged out shows `Login`;
/// logged in shows the two-tab shell, a plain `useState` switch (no router).
export default function App() {
  return (
    <AuthProvider>
      <InviteLink>
        <MessengerProvider>
          <OrgProvider>
            <Shell />
          </OrgProvider>
        </MessengerProvider>
      </InviteLink>
    </AuthProvider>
  );
}

/// `?invite=org:{id}` / `?invite=meeting:{id}`: the magic link. Accepts and
/// adopts the token on mount, then strips the query string either way.
function InviteLink({ children }) {
  const { loginWithToken } = useAuth();
  const invite = parseInviteParam();
  const [status, setStatus] = useState(invite ? "joining" : "idle");
  const [error, setError] = useState("");

  useEffect(() => {
    if (!invite) return undefined;
    let cancelled = false;

    (async () => {
      const path =
        invite.kind === "org"
          ? `/invites/org/${encodeURIComponent(invite.id)}/accept`
          : `/invites/meetings/${encodeURIComponent(invite.id)}/accept`;
      const response = await callApi({ method: "POST", path });

      const url = new URL(window.location.href);
      url.searchParams.delete("invite");
      window.history.replaceState({}, "", url);

      if (cancelled) return;
      if (response.ok) {
        loginWithToken(response.data.token);
        setStatus("done");
      } else {
        setError(describeFailure(response));
        setStatus("error");
      }
    })();

    return () => {
      cancelled = true;
    };
    // Only ever run once, against the link that was in the URL on mount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  if (status === "joining") {
    return (
      <div className="flex min-h-screen items-center justify-center bg-slate-950 text-slate-100">
        <p className="text-sm text-slate-400">joining…</p>
      </div>
    );
  }
  if (status === "error") {
    return (
      <div className="flex min-h-screen items-center justify-center bg-slate-950 px-4 text-slate-100">
        <p className="max-w-sm rounded-lg border border-rose-800 bg-slate-900/60 p-4 text-center font-mono text-xs break-all text-rose-300">
          {error}
        </p>
      </div>
    );
  }
  return children;
}

function parseInviteParam() {
  const raw = new URLSearchParams(window.location.search).get("invite");
  if (!raw) return null;
  const [kind, id] = raw.split(":");
  if ((kind !== "org" && kind !== "meeting") || !id) return null;
  return { kind, id };
}

function Shell() {
  const { token, email, isAuthenticated, logout } = useAuth();
  const { selectedOrgId } = useOrgContext();
  const [tab, setTab] = useState("meetings");
  const [meetingCodeToJoin, setMeetingCodeToJoin] = useState("");
  const { toasts, dismiss } = useNotifications();

  // The org this tab is looking at. Chat's room is whatever this reports,
  // which is why no component below opens a socket of its own.
  usePresence({ org: selectedOrgId });

  function joinMeeting(code) {
    setMeetingCodeToJoin(code);
    setTab("meetings");
  }

  // Shared with `PendingInvites` in behavior, not instance — both call
  // through to `AuthContext.loginWithToken`.
  const { acceptOrg, acceptMeeting } = useInviteActions({ onJoinMeeting: joinMeeting });

  function acceptToast(toast) {
    dismiss(toast.id);
    if (!toast.inviteId) return;
    if (toast.kind === "meeting") acceptMeeting(toast.inviteId);
    else if (toast.kind === "org") acceptOrg(toast.inviteId);
  }

  if (!isAuthenticated) return <Login />;

  return (
    <div className="min-h-screen bg-slate-950 text-slate-100">
      <Toast toasts={toasts} onDismiss={dismiss} onAccept={acceptToast} />
      <div className="mx-auto max-w-5xl px-4 py-10">
        <header className="mb-8 flex items-start justify-between gap-4">
          <div>
            <h1 className="text-2xl font-semibold">omoffice</h1>
            <p className="mt-1 font-mono text-sm text-slate-400">
              <span className="rounded bg-slate-800 px-1.5 py-0.5">{STAGE}</span>{" "}
              {API_URL}
            </p>
          </div>
          <div className="flex items-start gap-3">
            <OrgSwitcher />
            <div className="text-right">
              <p className="font-mono text-xs text-slate-400">{email}</p>
              <button
                type="button"
                onClick={logout}
                className="mt-1 text-xs text-slate-500 hover:text-slate-300"
              >
                sign out
              </button>
            </div>
          </div>
        </header>

        <PendingInvites onJoinMeeting={joinMeeting} />

        <nav className="mb-6 flex gap-1 border-b border-slate-800">
          {TABS.map(({ id, label }) => (
            <button
              key={id}
              type="button"
              onClick={() => setTab(id)}
              className={`-mb-px border-b-2 px-3 py-2 text-sm font-medium ${
                tab === id
                  ? "border-sky-500 text-slate-100"
                  : "border-transparent text-slate-500 hover:text-slate-300"
              }`}
            >
              {label}
            </button>
          ))}
        </nav>

        <div className="mb-8">
          {tab === "meetings" && (
            <MeetingPanel
              token={token}
              joinCode={meetingCodeToJoin}
              onJoinCodeConsumed={() => setMeetingCodeToJoin("")}
            />
          )}
          {tab === "chat" && <ChatPanel email={email} />}
          {tab === "orgs" && <Organizations token={token} />}
        </div>

        <details className="rounded-xl border border-slate-800 bg-slate-900/60 p-4">
          <summary className="cursor-pointer text-sm font-medium text-slate-300">
            developer tools
          </summary>
          <div className="mt-4 grid gap-4">
            <RouteTester route="/" />
            <RouteTester route="/health" />
            <RouteTester route="/private/me" token={token} />
            <RouteTester
              method="POST"
              route="/private/sample-post/{var}"
              fields={{ org_id: "string", name: "string", male: "boolean" }}
              token={token}
              note='requires the "post" role in org_id'
            />
            <RouteTester
              method="POST"
              route="/private/meetings"
              fields={{ org_id: "string" }}
              token={token}
            />
            <RouteTester route="/private/meetings?org_id={org_id}" token={token} />
            <RouteTester
              method="POST"
              route="/private/meetings/{code}/attendees"
              token={token}
            />
          </div>
        </details>
      </div>
    </div>
  );
}
