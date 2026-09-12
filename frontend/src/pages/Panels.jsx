import { useEffect, useState } from "react";
import { useAuth } from "../auth/AuthContext";
import { ORG_ACTIVE_CHANNEL, ORG_ROSTER_CHANNEL, listOrgs } from "../lib/rbac";
import { messenger } from "../lib/messenger";
import UserPanel from "../panels/UserPanel";
import OrgPanel from "../panels/OrgPanel";
import MeetingsPanel from "../panels/MeetingsPanel";
import PendingInvite from "../panels/PendingInvite";

const CURRENT_ORG_KEY = "omoffice_current_org";

export default function Panels() {
  const { token, logout } = useAuth();
  const [orgs, setOrgs] = useState([]);
  const [orgId, setOrgId] = useState(() =>
    localStorage.getItem(CURRENT_ORG_KEY),
  );
  const [error, setError] = useState(null);

  const reloadOrgs = () =>
    listOrgs(token)
      .then((data) => setOrgs(data.orgs ?? []))
      .catch((err) => setError(err.message));

  useEffect(() => {
    reloadOrgs();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [token]);

  const selectOrg = (id) => {
    setOrgId(id);
    if (id) localStorage.setItem(CURRENT_ORG_KEY, id);
    else localStorage.removeItem(CURRENT_ORG_KEY);
  };

  // Tell the backend which org this connection means -- see
  // `core::rbac::ACTIVE_CHANNEL` (backend/lib/core/rbac/mod.rs): "online" is
  // scoped to the one org actually being looked at, not every org this user
  // belongs to. The cleanup clears it, both on switching away from an org
  // and on leaving this page entirely.
  useEffect(() => {
    messenger.send(ORG_ACTIVE_CHANNEL, { org_id: orgId });
    return () => messenger.send(ORG_ACTIVE_CHANNEL, { org_id: null });
  }, [orgId]);

  // A `removed_you` push (see `core::rbac::notify_removed`,
  // backend/lib/core/rbac/mod.rs) means this browser lost membership --
  // either just now (self-leave, kicked) or from a change made in another
  // tab -- so drop the org from the list and off the selection.
  useEffect(
    () =>
      messenger.on(ORG_ROSTER_CHANNEL, (message) => {
        if (!message.removed_you) return;
        reloadOrgs();
        if (message.org_id === orgId) selectOrg(null);
      }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [orgId],
  );

  const currentOrg = orgs.find((org) => org.id === orgId) ?? null;

  return (
    <main className="mx-auto flex max-w-4xl flex-col gap-6 px-6 py-10">
      {error && <p className="text-sm text-danger">{error}</p>}
      <PendingInvite
        token={token}
        onAccepted={(orgId) => {
          reloadOrgs();
          selectOrg(orgId);
        }}
        onDeclined={reloadOrgs}
      />
      <UserPanel
        token={token}
        orgs={orgs}
        orgId={orgId}
        onSelectOrg={selectOrg}
        onOrgsChanged={reloadOrgs}
        onLogout={logout}
      />
      {currentOrg && (
        <OrgPanel
          key={`org-${currentOrg.id}`}
          token={token}
          org={currentOrg}
          onOrgChanged={reloadOrgs}
          onOrgDeleted={() => selectOrg(null)}
        />
      )}
      {currentOrg && (
        <MeetingsPanel
          key={`meetings-${currentOrg.id}`}
          token={token}
          org={currentOrg}
        />
      )}
    </main>
  );
}
