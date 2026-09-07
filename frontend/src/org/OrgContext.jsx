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
 * The one org a person is "currently in". Wraps `useOrgs` rather than
 * re-fetching, adding just "and which one is selected".
 */
export function OrgProvider({ children }) {
  const { token } = useAuth();
  const { orgs, loading, error, reload } = useOrgs(token);
  const [selectedOrgId, setSelectedOrgId] = useState(loadStoredOrgId);

  // Falls back to the first org when there's no valid selection, adjusted
  // during render (react.dev's pattern) so nothing flashes "no org" first.
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
