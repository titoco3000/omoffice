//! One org's member roster (`core::rbac::list_members`,
//! backend/lib/core/rbac/mod.rs), shared across every panel that needs it
//! -- `OrgPanel`, `PeoplePanel`, `MeetingsPanel` and `InvitesPanel` all
//! want the same `{user_id, display_name, online, roles}` list rather than
//! each fetching it and subscribing to `ORG_ROSTER_CHANNEL` on its own.
//!
//! `orgId: null` (no org selected) yields an empty roster with nothing
//! fetched or subscribed.

import { useEffect, useState } from "react";
import { messenger } from "./messenger";
import { ORG_ROSTER_CHANNEL, listMembers } from "./rbac";

export function useOrgRoster(token, orgId) {
  const [members, setMembers] = useState([]);
  const [error, setError] = useState(null);

  const reload = () => {
    if (!orgId) return Promise.resolve();
    return listMembers(token, orgId).then((data) =>
      setMembers(data.members ?? []),
    );
  };

  useEffect(() => {
    if (!orgId) return;
    reload().catch((err) => setError(err.message));
    return () => setMembers([]);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [token, orgId]);

  // Live updates: a role change, join or removal anywhere in this org --
  // see `core::rbac::broadcast_roster` (backend/lib/core/rbac/mod.rs) --
  // so every consumer of this hook stays current without polling.
  useEffect(() => {
    if (!orgId) return;
    return messenger.on(ORG_ROSTER_CHANNEL, (message) => {
      if (message.org_id === orgId && message.members)
        setMembers(message.members);
    });
  }, [orgId]);

  return { members, reload, error };
}
