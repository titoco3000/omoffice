import { useEffect, useState } from "react";
import { me, renameMe, createOrg, leaveOrg } from "../lib/rbac";

export default function UserPanel({ token, orgs, orgId, onSelectOrg, onOrgsChanged, onLogout }) {
  const [name, setName] = useState("");
  const [newOrgName, setNewOrgName] = useState("");
  const [error, setError] = useState(null);

  useEffect(() => {
    me(token)
      .then((data) => setName(data.name ?? ""))
      .catch(() => {});
  }, [token]);

  const saveName = () => {
    renameMe(token, name).catch((err) => setError(err.message));
  };

  const addOrg = async (event) => {
    event.preventDefault();
    if (!newOrgName.trim()) return;
    try {
      const { org } = await createOrg(token, newOrgName.trim());
      setNewOrgName("");
      await onOrgsChanged();
      onSelectOrg(org.id);
    } catch (err) {
      setError(err.message);
    }
  };

  const leave = async () => {
    try {
      await leaveOrg(token, orgId);
      await onOrgsChanged();
      onSelectOrg(null);
    } catch (err) {
      setError(err.message);
    }
  };

  return (
    <section className="rounded-card border bg-surface p-5">
      <div className="flex items-center justify-between">
        <h2 className="text-lg font-semibold text-ink">You</h2>
        <button onClick={onLogout} className="text-sm text-ink-muted hover:text-ink">
          Log out
        </button>
      </div>

      <label className="mt-4 block text-sm text-ink-muted">Display name</label>
      <input
        value={name}
        onChange={(event) => setName(event.target.value)}
        onBlur={saveName}
        className="mt-1 w-full rounded-card border bg-bg px-3 py-1.5 text-ink"
      />

      <label className="mt-4 block text-sm text-ink-muted">Organization</label>
      <select
        value={orgId ?? ""}
        onChange={(event) => onSelectOrg(event.target.value || null)}
        className="mt-1 w-full rounded-card border bg-bg px-3 py-1.5 text-ink"
      >
        <option value="">-- choose an org --</option>
        {orgs.map((org) => (
          <option key={org.id} value={org.id}>
            {org.name}
          </option>
        ))}
      </select>

      <form onSubmit={addOrg} className="mt-2 flex gap-2">
        <input
          value={newOrgName}
          onChange={(event) => setNewOrgName(event.target.value)}
          placeholder="New org name"
          className="flex-1 rounded-card border bg-bg px-3 py-1.5 text-ink"
        />
        <button className="rounded-card bg-brand px-3 py-1.5 text-sm font-medium text-on-brand hover:bg-brand-strong">
          Create
        </button>
      </form>

      {orgId && (
        <div className="mt-4 rounded-card border border-danger/40 p-3">
          <p className="text-sm text-ink-muted">Leaving removes you from this org.</p>
          <button onClick={leave} className="mt-2 text-sm font-medium text-danger hover:underline">
            Leave organization
          </button>
        </div>
      )}

      {error && <p className="mt-3 text-sm text-danger">{error}</p>}
    </section>
  );
}
