import { useState } from "react";
import { AuthProvider, useAuth } from "./auth/AuthContext";
import Login from "./components/Login";
import MeetingPanel from "./components/MeetingPanel";
import Organizations from "./components/Organizations";
import RouteTester from "./components/RouteTester";
import { API_URL, STAGE } from "./api";

const TABS = [
  { id: "meetings", label: "meetings" },
  { id: "orgs", label: "organizations" },
];

/// Top-level shell: `AuthProvider` owns the token, everything else is a
/// child of it. Logged out shows `Login`; logged in shows the two-tab app
/// shell — a plain `useState` switch rather than a router, since there are
/// only ever two pages.
export default function App() {
  return (
    <AuthProvider>
      <Shell />
    </AuthProvider>
  );
}

function Shell() {
  const { token, email, isAuthenticated, logout } = useAuth();
  const [tab, setTab] = useState("meetings");

  if (!isAuthenticated) return <Login />;

  return (
    <div className="min-h-screen bg-slate-950 text-slate-100">
      <div className="mx-auto max-w-5xl px-4 py-10">
        <header className="mb-8 flex items-start justify-between gap-4">
          <div>
            <h1 className="text-2xl font-semibold">omoffice</h1>
            <p className="mt-1 font-mono text-sm text-slate-400">
              <span className="rounded bg-slate-800 px-1.5 py-0.5">{STAGE}</span>{" "}
              {API_URL}
            </p>
          </div>
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
        </header>

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
          {tab === "meetings" && <MeetingPanel token={token} />}
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
            <RouteTester method="POST" route="/private/meetings" token={token} />
            <RouteTester route="/private/meetings" token={token} />
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
