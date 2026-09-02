import { useState } from "react";
import RouteTester from "./components/RouteTester";
import { API_URL, STAGE } from "./api";

/// One RouteTester per route the API serves. The token from
/// /auth/request-key/{email} is held here and handed to the protected ones.
export default function App() {
  const [token, setToken] = useState("");

  return (
    <div className="min-h-screen bg-slate-950 text-slate-100">
      <div className="mx-auto max-w-3xl px-4 py-10">
        <header className="mb-8">
          <h1 className="text-2xl font-semibold">omoffice</h1>
          <p className="mt-1 font-mono text-sm text-slate-400">
            <span className="rounded bg-slate-800 px-1.5 py-0.5">{STAGE}</span>{" "}
            {API_URL}
          </p>
        </header>

        <section className="mb-8 rounded-xl border border-slate-800 bg-slate-900/60 p-4">
          <div className="flex items-center justify-between">
            <h2 className="text-sm font-medium text-slate-300">token</h2>
            {token && (
              <button
                type="button"
                onClick={() => setToken("")}
                className="text-xs text-slate-400 hover:text-slate-200"
              >
                clear
              </button>
            )}
          </div>
          <p className="mt-2 break-all font-mono text-xs text-slate-500">
            {token || "none — request one below"}
          </p>
        </section>

        <div className="grid gap-4">
          <RouteTester route="/" />
          <RouteTester route="/health" />
          <RouteTester
            route="/auth/request-key/{email}"
            note="whitelisted: admin@omoffice.example, dev@omoffice.example"
            onResult={(result) => setToken(result.token ?? "")}
          />
          <RouteTester route="/private/me" token={token} />
          <RouteTester
            method="POST"
            route="/private/sample-post/{var}"
            fields={{ name: "string", male: "boolean" }}
            token={token}
          />
        </div>
      </div>
    </div>
  );
}
