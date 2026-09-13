import { useState } from "react";
import { requestKey } from "../lib/rbac";

function Centered({ children }) {
  return (
    <main className="flex min-h-full items-center justify-center px-6">
      {children}
    </main>
  );
}

export default function Login() {
  const [email, setEmail] = useState("");
  const [sent, setSent] = useState(false);
  const [error, setError] = useState(null);

  const submit = async (event) => {
    event.preventDefault();
    setError(null);
    try {
      await requestKey(email.trim());
      setSent(true);
    } catch (err) {
      setError(err.message);
    }
  };

  if (sent) {
    return (
      <Centered>
        <p className="text-ink">Check {email} for a sign-in link.</p>
      </Centered>
    );
  }

  return (
    <Centered>
      <form onSubmit={submit} className="flex w-full max-w-sm flex-col gap-3">
        <h1 className="text-2xl font-semibold text-ink">Log in</h1>
        <input
          type="email"
          required
          value={email}
          onChange={(event) => setEmail(event.target.value)}
          placeholder="you@example.com"
          className="rounded-card border bg-surface px-3 py-2 text-ink"
        />
        {error && <p className="text-sm text-danger">{error}</p>}
        <button
          type="submit"
          className="rounded-card bg-brand px-4 py-2 font-medium text-on-brand hover:bg-brand-strong"
        >
          Send link
        </button>
      </form>
    </Centered>
  );
}
