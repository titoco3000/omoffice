import { useState } from "react";
import { useAuth } from "../auth/AuthContext";
import { useOrgSession } from "../org/OrgSessionContext";
import { navigate } from "../router/Router";
import { renameOrg, deleteOrg } from "../lib/rbac";
import UsersManagement from "../components/UsersManagement";
import AsyncButton from "../components/AsyncButton";

export default function Org() {
  const { token } = useAuth();
  const { currentOrg, isAdmin, canManageTeam, reloadOrgs, selectOrg } =
    useOrgSession();
  const [error, setError] = useState(null);

  if (!currentOrg)
    return (
      <p className="p-10 text-center text-ink-muted">
        Choose an organization first.
      </p>
    );
  if (!canManageTeam) {
    return (
      <p className="p-10 text-center text-ink-muted">
        You don&apos;t have access to manage this organization.
      </p>
    );
  }

  const saveName = (event) => {
    const value = event.target.value.trim();
    if (!value || value === currentOrg.name) return;
    renameOrg(token, currentOrg.id, value)
      .then(reloadOrgs)
      .catch((err) => setError(err.message));
  };

  const remove = async () => {
    if (!window.confirm(`Delete ${currentOrg.name}? This cannot be undone.`))
      return;
    try {
      await deleteOrg(token, currentOrg.id);
      reloadOrgs();
      selectOrg(null);
      navigate("/in");
    } catch (err) {
      setError(err.message);
    }
  };

  return (
    <main className="mx-auto flex max-w-2xl flex-col gap-6 px-6 py-10">
      {isAdmin ? (
        <input
          key={currentOrg.id}
          defaultValue={currentOrg.name}
          onBlur={saveName}
          className="rounded-card border bg-bg px-3 py-1.5 text-2xl font-semibold text-ink"
        />
      ) : (
        <h1 className="text-2xl font-semibold text-ink">{currentOrg.name}</h1>
      )}

      <UsersManagement />

      {isAdmin && (
        <section className="rounded-card border border-danger/40 p-4">
          <p className="text-sm text-ink-muted">
            Deletes the org for every member. Cannot be undone.
          </p>
          <AsyncButton
            onClick={remove}
            className="mt-2 text-sm font-medium text-danger hover:underline"
          >
            Delete organization
          </AsyncButton>
        </section>
      )}

      {error && <p className="text-sm text-danger">{error}</p>}
    </main>
  );
}
