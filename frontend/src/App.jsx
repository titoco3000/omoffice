import { useEffect, useState } from "react";
import { callApi, describeFailure } from "./api";
import { AuthProvider, useAuth } from "./auth/AuthContext";
import Toast from "./components/Toast";
import { useNotifications } from "./hooks/useNotifications";
import NavContainer from "./layout/NavContainer";
import { MeetingProvider } from "./meeting/MeetingContext";
import MeetingDock from "./meeting/MeetingDock";
import { MessengerProvider, usePresence } from "./messenger/MessengerContext";
import { OrgProvider, useOrgContext } from "./org/OrgContext";
import Home from "./pages/Home";
import LoginPage from "./pages/LoginPage";
import MeetingPage from "./pages/MeetingPage";
import MePage from "./pages/MePage";
import OrgPage from "./pages/OrgPage";
import UsersPage from "./pages/UsersPage";
import { matchPath, Redirect, RouterProvider, useNavigate, usePath } from "./router";
import { ThemeProvider } from "./theme/ThemeContext";
import Notice from "./ui/Notice";

/// Top-level shell. The provider order is the dependency order: the theme
/// paints even a failed invite, the router is read by the invite guards, and
/// the meeting is held below the org so leaving one does not leave the other.
export default function App() {
  return (
    <ThemeProvider>
      <RouterProvider>
        <AuthProvider>
          <InviteLink>
            <MessengerProvider>
              <OrgProvider>
                <MeetingProvider>
                  <Shell />
                </MeetingProvider>
              </OrgProvider>
            </MessengerProvider>
          </InviteLink>
        </AuthProvider>
      </RouterProvider>
    </ThemeProvider>
  );
}

/// `?invite=org:{id}` / `?invite=meeting:{id}`: the magic link. Accepts and
/// adopts the token on mount, then strips the query string either way.
function InviteLink({ children }) {
  const { loginWithToken } = useAuth();
  const navigate = useNavigate();
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

      // The link is single-use, so it comes out of the URL either way.
      const url = new URL(window.location.href);
      url.searchParams.delete("invite");
      window.history.replaceState({}, "", url);

      if (cancelled) return;
      if (response.ok) {
        loginWithToken(response.data.token);
        // A meeting link lands in the meeting: the accept route hands back
        // the code, and the path is how the app says "be in it".
        if (invite.kind === "meeting" && response.data.code) {
          navigate(`/meeting/${encodeURIComponent(response.data.code)}`, {
            replace: true,
          });
        }
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
      <div className="flex min-h-dvh items-center justify-center">
        <p className="text-sm text-ink-muted">joining…</p>
      </div>
    );
  }
  if (status === "error") {
    return (
      <div className="flex min-h-dvh items-center justify-center px-4">
        <Notice className="max-w-sm text-center">{error}</Notice>
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

/**
 * The routing table, and the two things that outlive a page: the toast
 * stack and the meeting dock. Both sit outside the routed content on
 * purpose — a meeting must survive navigation (CLAUDE.md), and a toast
 * announcing an invite is not any one page's business.
 */
function Shell() {
  const { isAuthenticated } = useAuth();
  const { selectedOrgId } = useOrgContext();
  const path = usePath();
  const { toasts, dismiss } = useNotifications();

  // The org this tab is looking at. Chat's room is whatever this reports,
  // which is why no component below opens a socket of its own.
  usePresence({ org: selectedOrgId });

  return (
    <>
      <Toast toasts={toasts} onDismiss={dismiss} />
      {routed(path, isAuthenticated)}
      {isAuthenticated && <MeetingDock />}
    </>
  );
}

/// Five paths. `/` and `/login` stand alone; everything else is the app,
/// and the app needs a token.
function routed(path, isAuthenticated) {
  if (path === "/") return <Home />;
  if (path === "/login") return <LoginPage />;

  if (!isAuthenticated) return <Redirect to="/login" />;

  if (path === "/users") {
    return (
      <NavContainer>
        <UsersPage />
      </NavContainer>
    );
  }
  if (path === "/me") {
    return (
      <NavContainer>
        <MePage />
      </NavContainer>
    );
  }
  if (path === "/org") {
    return (
      <NavContainer>
        <OrgPage />
      </NavContainer>
    );
  }

  const meeting = matchPath("/meeting/:code", path);
  if (meeting) {
    return (
      <NavContainer>
        <MeetingPage code={meeting.code} />
      </NavContainer>
    );
  }

  return <Redirect to="/users" />;
}
