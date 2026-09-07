import { useState } from "react";
import { useAuth } from "../auth/AuthContext";
import { describeFailure } from "../api";

/** The entire login flow: `GET /auth/request-key/{email}` hands back a token. */
export default function Login() {
  const { login } = useAuth();
  const [email, setEmail] = useState("");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");

  async function submit(event) {
    event.preventDefault();
    const wanted = email.trim();
    if (!wanted) return;
    setError("");
    setPending(true);
    const response = await login(wanted);
    setPending(false);
    if (!response.ok) setError(describeFailure(response));
  }

  return (
    <div className="flex min-h-screen items-center justify-center bg-slate-950 px-4 text-slate-100">
      <div className="w-full max-w-sm rounded-xl border border-slate-800 bg-slate-900/60 p-6">
        <h1 className="text-lg font-semibold">omoffice</h1>
        <p className="mt-1 text-sm text-slate-400">
          Sign in with your email — no password, a token comes right back.
        </p>

        <form onSubmit={submit} className="mt-4 grid gap-3">
          <input
            type="email"
            required
            autoFocus
            value={email}
            onChange={(event) => setEmail(event.target.value)}
            placeholder="you@example.com"
            className="h-9 rounded-lg border border-slate-700 bg-slate-950 px-2 font-mono text-sm text-slate-100 outline-none focus:border-sky-500"
          />
          <button
            type="submit"
            disabled={pending || !email.trim()}
            className="rounded-lg bg-sky-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-sky-500 disabled:opacity-50"
          >
            {pending ? "..." : "sign in"}
          </button>
        </form>

        {error && (
          <p className="mt-3 rounded-lg border border-rose-800 bg-slate-950 p-3 font-mono text-xs break-all text-rose-300">
            {error}
          </p>
        )}
      </div>
    </div>
  );
}
