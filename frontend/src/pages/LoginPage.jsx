import { useState } from "react";
import { describeFailure } from "../api";
import { useAuth } from "../auth/AuthContext";
import ThemeToggle from "../components/ThemeToggle";
import { Link, Redirect } from "../router";
import Button from "../ui/Button";
import Input from "../ui/Input";
import Notice from "../ui/Notice";
import { IconLogo } from "../ui/icons";

/// `/login` — the entire sign-in flow: `GET /auth/request-key/{email}`
/// hands a token straight back, so there is nothing to wait for in a inbox.
export default function LoginPage() {
  const { login, isAuthenticated } = useAuth();
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

  // The token landing is what ends this page — the guard in `App` sends an
  // authenticated visitor on to the roster.
  if (isAuthenticated) return <Redirect to="/users" />;

  return (
    <div className="flex min-h-dvh items-center justify-center px-4 py-12">
      <div className="absolute top-4 right-4">
        <ThemeToggle />
      </div>

      <div className="w-full max-w-sm rounded-card border border-line bg-surface p-6">
        <Link to="/" className="flex items-center gap-2">
          <IconLogo className="h-7 w-7 text-brand" />
          <span className="text-base font-semibold">omoffice</span>
        </Link>
        <p className="mt-3 text-sm text-ink-muted">
          Sign in with your email — no password, a token comes right back.
        </p>

        <form onSubmit={submit} className="mt-5 grid gap-3">
          <Input
            type="email"
            mono
            required
            autoFocus
            value={email}
            onChange={(event) => setEmail(event.target.value)}
            placeholder="you@example.com"
          />
          <Button type="submit" disabled={pending || !email.trim()}>
            {pending ? "..." : "sign in"}
          </Button>
        </form>

        {error && <Notice className="mt-4">{error}</Notice>}
      </div>
    </div>
  );
}
