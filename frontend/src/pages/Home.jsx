import { useAuth } from "../auth/AuthContext";
import { Link } from "../router";
import ThemeToggle from "../components/ThemeToggle";
import { IconLogo } from "../ui/icons";

/// `/` — the one page that is not the app: what omoffice is, and a way in.
/// Deliberately outside `NavContainer`, so a logged-out visitor sees no
/// roster and no org.
export default function Home() {
  const { isAuthenticated } = useAuth();

  return (
    <div className="flex min-h-dvh flex-col items-center justify-center px-6 py-16">
      <div className="absolute top-4 right-4">
        <ThemeToggle />
      </div>

      <main className="w-full max-w-lg text-center">
        <IconLogo className="mx-auto h-14 w-14 text-brand" />
        <h1 className="mt-5 text-3xl font-semibold tracking-tight">omoffice</h1>
        <p className="mt-4 text-base leading-relaxed text-ink-muted">
          The office, minus the building. Your team sits in one list: see who
          is around, click a name, and you are talking — no scheduling, no
          links to send, no meeting that has to be arranged before it can
          happen. Every room belongs to an organization, so the people, the
          chat and the meetings are the same group all the way down.
        </p>

        <Link
          to={isAuthenticated ? "/users" : "/login"}
          className="mt-8 inline-flex h-10 items-center justify-center rounded-lg bg-brand px-6 text-sm font-medium text-on-brand transition-colors hover:bg-brand-strong"
        >
          {isAuthenticated ? "open omoffice" : "sign in"}
        </Link>
      </main>
    </div>
  );
}
