import { useEffect, useState } from "react";
import { useAuth } from "../auth/AuthContext";
import { listOrgs } from "../lib/rbac";
import UserPanel from "../panels/UserPanel";
import OrgPanel from "../panels/OrgPanel";
import PendingInvite from "../panels/PendingInvite";

const CURRENT_ORG_KEY = "omoffice_current_org";

export default function Panels() {
  const { token, logout } = useAuth();
  const [orgs, setOrgs] = useState([]);
  const [orgId, setOrgId] = useState(() => localStorage.getItem(CURRENT_ORG_KEY));
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
          key={currentOrg.id}
          token={token}
          org={currentOrg}
          onOrgChanged={reloadOrgs}
          onOrgDeleted={() => selectOrg(null)}
        />
      )}
    </main>
  );
}
