import { Link } from "../router/Router";
import { useOrgSession } from "../org/OrgSessionContext";
import Inbox from "../components/Inbox";
import OrgOnlineView from "../components/OrgOnlineView";

export default function InPage() {
  const { orgs, currentOrg, session } = useOrgSession();
  const { myMeetingId, create } = session;

  if (orgs.length === 0) {
    return (
      <main className="mx-auto flex max-w-md flex-col items-center gap-4 px-6 py-16 text-center">
        <p className="text-ink-muted">
          You aren&apos;t member of any organization.
        </p>
        <Link
          to="/org/new"
          className="rounded-card bg-brand px-4 py-2 font-medium text-on-brand hover:bg-brand-strong"
        >
          Create an organization
        </Link>
      </main>
    );
  }

  return (
    <main className="mx-auto flex max-w-2xl flex-col gap-6 px-6 py-10">
      <Inbox />

      {currentOrg ? (
        <section>
          <div className="flex items-center justify-between">
            <h2 className="text-lg font-semibold text-ink">
              {currentOrg.name}
            </h2>
            <button
              onClick={() => create()}
              disabled={!!myMeetingId}
              className="rounded-card bg-brand px-3 py-1.5 text-sm font-medium text-on-brand hover:bg-brand-strong disabled:opacity-50"
            >
              New meeting
            </button>
          </div>
          <div className="mt-4">
            <OrgOnlineView />
          </div>
        </section>
      ) : (
        <p className="text-ink-muted">
          Choose an organization from your{" "}
          <Link to="/profile" className="text-brand hover:underline">
            profile
          </Link>
          .
        </p>
      )}
    </main>
  );
}
