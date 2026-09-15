import { useEffect, useRef, useState } from "react";
import { useAuth } from "../auth/AuthContext";
import { navigate } from "../router/Router";
import { requestKey } from "../lib/rbac";
import AsyncButton from "../components/AsyncButton";

// How long the explanation sits on screen before continuing on its own --
// long enough to actually read it, short enough not to feel stuck.
const UNSAFE_LOGIN_DELAY_MS = 10000;

function Centered({ children }) {
  return (
    <main className="flex min-h-full items-center justify-center px-6">
      {children}
    </main>
  );
}

function UnsafeLoginNotice({ token }) {
  const { applyToken } = useAuth();

  const proceed = () => {
    applyToken(token);
    navigate("/in");
  };

  useEffect(() => {
    const timer = setTimeout(proceed, UNSAFE_LOGIN_DELAY_MS);
    return () => clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [token]);

  return (
    <Centered>
      <div className="flex max-w-sm flex-col gap-3 text-center">
        <h1 className="text-xl font-semibold text-ink">Unsafe login enabled</h1>
        <p className="text-sm text-ink-muted">
          This server has email delivery disabled for testing, so the sign-in
          link is being applied directly instead of requiring you to open it
          from an inbox. A normal deployment has no such shortcut -- signing in
          there always means checking your email.
        </p>
        <button
          onClick={proceed}
          className="rounded-card bg-brand px-4 py-2 font-medium text-on-brand hover:bg-brand-strong"
        >
          Continue
        </button>
      </div>
    </Centered>
  );
}

export default function Login() {
  const [email, setEmail] = useState("");
  const [sent, setSent] = useState(false);
  const [unsafeToken, setUnsafeToken] = useState(null);
  const [error, setError] = useState(null);
  // Implicit (Enter-key) submission fires the form's `submit` event, not a
  // click -- routing it through the same button `AsyncButton` renders is
  // the one way to land on its loading state either way.
  const buttonRef = useRef(null);

  const preventImplicitSubmit = (event) => {
    event.preventDefault();
    buttonRef.current?.click();
  };

  const submit = async () => {
    setError(null);
    try {
      const data = await requestKey(email.trim());
      if (data?.token) setUnsafeToken(data.token);
      else setSent(true);
    } catch (err) {
      setError(err.message);
    }
  };

  if (unsafeToken) return <UnsafeLoginNotice token={unsafeToken} />;

  if (sent) {
    return (
      <Centered>
        <p className="text-ink">Check {email} for a sign-in link.</p>
      </Centered>
    );
  }

  return (
    <Centered>
      <form
        onSubmit={preventImplicitSubmit}
        className="flex w-full max-w-sm flex-col gap-3"
      >
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
        <AsyncButton
          ref={buttonRef}
          onClick={submit}
          className="rounded-card bg-brand px-4 py-2 font-medium text-on-brand hover:bg-brand-strong"
        >
          Send link
        </AsyncButton>
      </form>
    </Centered>
  );
}
