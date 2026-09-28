import { useAuth } from "../core/auth/AuthContext";
import { Link } from "../core/Router";

export default function Home() {
  const { token } = useAuth();

  return (
    <main className="mx-auto flex min-h-full max-w-2xl flex-col items-center justify-center gap-6 px-6 text-center">
      <h1 className="text-4xl font-semibold text-ink">omoffice</h1>
      <p className="text-ink-muted">
        Meetings and org management in one place -- rooms, roles and invites,
        without a separate tool for each.
      </p>
      <Link
        to={token ? "/in" : "/login"}
        className="rounded-card bg-brand px-6 py-2.5 font-medium text-on-brand hover:bg-brand-strong"
      >
        {token ? "Access my orgs" : "Log in"}
      </Link>
    </main>
  );
}
