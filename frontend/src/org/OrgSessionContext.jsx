//! Which org is "current" plus everything scoped to it -- orgs list,
//! member roster, meeting session -- shared by every page and by
//! `NavContainer`'s sidebar, rather than each one fetching its own copy.
//! Mirrors what `pages/Panels.jsx` used to hold before the page split.

import { createContext, useContext, useEffect, useState } from "react";
import { decodeJwt } from "../lib/jwt";
import { messenger } from "../lib/messenger";
import {
  ORG_ACTIVE_CHANNEL,
  ORG_ROSTER_CHANNEL,
  listOrgs,
  me as fetchMe,
} from "../lib/rbac";
import { useMeetingSession } from "../lib/useMeetingSession";
import { useOrgRoster } from "../lib/useOrgRoster";
import { useAuth } from "../auth/AuthContext";

const CURRENT_ORG_KEY = "omoffice_current_org";
const OrgSessionContext = createContext(null);

export function OrgSessionProvider({ children }) {
  const { token } = useAuth();
  const myId = decodeJwt(token)?.sub;

  const [orgs, setOrgs] = useState([]);
  const [orgId, setOrgId] = useState(() =>
    localStorage.getItem(CURRENT_ORG_KEY),
  );
  const [error, setError] = useState(null);
  // The signed-in user's own name/picture, independent of any org -- what
  // `NavContainer`'s profile link shows, since it's visible with no org
  // selected at all.
  const [profile, setProfile] = useState(null);

  const reloadOrgs = () =>
    listOrgs(token)
      .then((data) => setOrgs(data.orgs ?? []))
      .catch((err) => setError(err.message));

  const reloadProfile = () =>
    fetchMe(token)
      .then((data) => setProfile({ name: data.name, picture: data.picture }))
      .catch(() => {});

  useEffect(() => {
    reloadOrgs();
    reloadProfile();
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
  // belongs to.
  useEffect(() => {
    messenger.send(ORG_ACTIVE_CHANNEL, { org_id: orgId });
    return () => messenger.send(ORG_ACTIVE_CHANNEL, { org_id: null });
  }, [orgId]);

  // A `removed_you` push means this browser lost membership -- either just
  // now, or from a change made in another tab -- so drop the org from the
  // list and off the selection.
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
  const roster = useOrgRoster(token, currentOrg?.id ?? null);
  const session = useMeetingSession(token, currentOrg?.id ?? null);

  const me = roster.members.find((member) => member.user_id === myId);
  const isAdmin = me?.roles.includes("admin") ?? false;
  const canManageTeam =
    isAdmin || (me?.roles.includes("team-management") ?? false);

  const value = {
    myId,
    orgs,
    orgId,
    currentOrg,
    selectOrg,
    reloadOrgs,
    roster,
    session,
    isAdmin,
    canManageTeam,
    profile,
    reloadProfile,
    error,
  };

  return (
    <OrgSessionContext.Provider value={value}>
      {children}
    </OrgSessionContext.Provider>
  );
}

export function useOrgSession() {
  return useContext(OrgSessionContext);
}
