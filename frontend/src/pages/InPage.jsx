import { useState } from "react";
import { FiUsers, FiX } from "react-icons/fi";
import { Link } from "../router/Router";
import { useAuth } from "../auth/AuthContext";
import { useOrgSession } from "../org/OrgSessionContext";
import { useGraphics } from "../theme/GraphicsContext";
import { useIsMobile } from "../layout/useViewport";
import OrgOnlineView from "../components/OrgOnlineView";
import AsyncButton from "../components/AsyncButton";
import MapCanvas from "../map/MapCanvas";

// Graphics mode "map": the canvas is the whole page. Desktop gets the
// roster in `NavContainer`'s sidebar (see its `mode === "map"` check);
// mobile has no sidebar, so a floating bottom-right toggle reveals it
// in an overlay instead.
function MapView() {
  const { token } = useAuth();
  const { myId, currentOrg, roster } = useOrgSession();
  const isMobile = useIsMobile();
  const [rosterOpen, setRosterOpen] = useState(false);

  const online = roster.members.filter((member) => member.online);

  return (
    <div className="relative size-full">
      <MapCanvas
        token={token}
        orgId={currentOrg.id}
        members={online}
        myId={myId}
      />
      {isMobile && (
        <>
          <button
            onClick={() => setRosterOpen((v) => !v)}
            title={rosterOpen ? "Hide roster" : "Show roster"}
            className="absolute bottom-4 right-4 flex size-11 items-center justify-center rounded-full bg-brand text-on-brand shadow-lg"
          >
            {rosterOpen ? <FiX /> : <FiUsers />}
          </button>
          {rosterOpen && (
            <div className="absolute bottom-20 right-4 max-h-[70%] w-64 overflow-y-auto rounded-card border bg-surface p-3 shadow-lg">
              <OrgOnlineView />
            </div>
          )}
        </>
      )}
    </div>
  );
}

export default function InPage() {
  const { orgs, currentOrg, session } = useOrgSession();
  const { mode } = useGraphics();
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

  if (mode === "map" && currentOrg) {
    return (
      <main className="size-full">
        <MapView />
      </main>
    );
  }

  return (
    <main className="mx-auto flex max-w-2xl flex-col gap-6 px-6 py-10">
      {currentOrg ? (
        <section>
          <div className="flex items-center justify-between">
            <h2 className="text-lg font-semibold text-ink">
              {currentOrg.name}
            </h2>
            <AsyncButton
              onClick={() => create()}
              disabled={!!myMeetingId}
              className="rounded-card bg-brand px-3 py-1.5 text-sm font-medium text-on-brand hover:bg-brand-strong disabled:opacity-50"
            >
              New meeting
            </AsyncButton>
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
