import { createContext, useContext, useState } from "react";
import { useAuth } from "../auth/AuthContext";
import { useOrgs } from "../hooks/useOrgs";

const STORAGE_KEY = "omoffice_selected_org";

const OrgContext = createContext(null);

function loadStoredOrgId() {
  try {
    return localStorage.getItem(STORAGE_KEY) || null;
  } catch {
    return null; // private browsing, storage disabled, etc.
  }
}

function storeOrgId(id) {
  try {
    if (id) localStorage.setItem(STORAGE_KEY, id);
    else localStorage.removeItem(STORAGE_KEY);
  } catch {
    // Nothing to do if storage is unavailable -- the selection just won't
    // survive a refresh.
  }
}

/**
 * The one org a person is "currently in" -- what `OrgSwitcher`'s dropdown
 * picks, and what `ChatPanel` (which room) and `MeetingPanel`/`MeetingHistory`
 * (which org's meetings, and which org a new one is created under) read
 * instead of each keeping their own org list and picker the way `ChatPanel`
 * used to.
 *
 * Wraps `useOrgs` (the same fetch `Organizations`/`OrgDetail` use) rather
 * than re-fetching: one source for "which orgs does this token belong to",
 * this context just adds "and which one is selected" on top.
 */
export function OrgProvider({ children }) {
  const { token } = useAuth();
  const { orgs, loading, error, reload } = useOrgs(token);
  const [selectedOrgId, setSelectedOrgId] = useState(loadStoredOrgId);

  // If there's no valid selection (nothing stored, it named an org this
  // token isn't in, or an org was just left) but the list has something,
  // fall back to the first one -- adjusted during render, react.dev's own
  // pattern for this, rather than an effect that would leave `ChatPanel`
  // etc. rendering "no org selected" for one extra frame after every load.
  const validSelection = orgs.some((org) => org.id === selectedOrgId);
  if (!validSelection && orgs.length > 0) {
    setSelectedOrgId(orgs[0].id);
  }
  const effectiveOrgId = validSelection ? selectedOrgId : (orgs[0]?.id ?? null);

  function selectOrg(id) {
    setSelectedOrgId(id);
    storeOrgId(id);
  }

  const selectedOrg = orgs.find((org) => org.id === effectiveOrgId) ?? null;

  return (
    <OrgContext.Provider
      value={{
        orgs,
        loading,
        error,
        reload,
        selectedOrgId: effectiveOrgId,
        selectedOrg,
        selectOrg,
      }}
    >
      {children}
    </OrgContext.Provider>
  );
}

export function useOrgContext() {
  const context = useContext(OrgContext);
  if (!context) throw new Error("useOrgContext must be used inside an OrgProvider");
  return context;
}
