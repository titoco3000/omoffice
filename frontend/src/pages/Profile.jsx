import { useEffect, useState } from "react";
import { useAuth } from "../auth/AuthContext";
import { useOrgSession } from "../org/OrgSessionContext";
import { Link } from "../router/Router";
import { me, renameMe, renameMeInOrg, leaveOrg } from "../lib/rbac";
import ThemeToggle from "../theme/ThemeToggle";
import AsyncButton from "../components/AsyncButton";

export default function Profile() {
  const { token, logout } = useAuth();
  const {
    myId,
    orgs,
    orgId,
    currentOrg,
    selectOrg,
    reloadOrgs,
    roster,
    canManageTeam,
  } = useOrgSession();

  const [name, setName] = useState("");
  const [error, setError] = useState(null);

  useEffect(() => {
    me(token)
      .then((data) => setName(data.name ?? ""))
      .catch(() => {});
  }, [token]);

  const saveName = () =>
    renameMe(token, name).catch((err) => setError(err.message));
  const saveOrgName = (event) => {
    const value = event.target.value.trim();
    renameMeInOrg(token, currentOrg.id, value)
      .then(roster.reload)
      .catch((err) => setError(err.message));
  };
  const orgDisplayName =
    roster.members.find((member) => member.user_id === myId)?.display_name ??
    "";

  const leave = async () => {
    if (!window.confirm(`Leave ${currentOrg.name}?`)) return;
    try {
      await leaveOrg(token, currentOrg.id);
      await reloadOrgs();
      selectOrg(null);
    } catch (err) {
      setError(err.message);
    }
  };

  return (
    <main className="mx-auto flex max-w-lg flex-col gap-6 px-6 py-10">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold text-ink">Profile</h1>
        <button
          onClick={() => logout()}
          className="text-sm text-ink-muted hover:text-ink"
        >
          Log out
        </button>
      </div>

      <section className="rounded-card border bg-surface p-5">
        <label className="block text-sm text-ink-muted">Display name</label>
        <input
          value={name}
          onChange={(event) => setName(event.target.value)}
          onBlur={saveName}
          className="mt-1 w-full rounded-card border bg-bg px-3 py-1.5 text-ink"
        />

        {currentOrg && (
          <>
            <label className="mt-4 block text-sm text-ink-muted">
              Name inside {currentOrg.name}
            </label>
            <input
              key={`${currentOrg.id}-${orgDisplayName}`}
              defaultValue={orgDisplayName}
              onBlur={saveOrgName}
              className="mt-1 w-full rounded-card border bg-bg px-3 py-1.5 text-ink"
            />
          </>
        )}
      </section>

      <section className="rounded-card border bg-surface p-5">
        <label className="block text-sm text-ink-muted">Organization</label>
        <select
          value={orgId ?? ""}
          onChange={(event) => selectOrg(event.target.value || null)}
          className="mt-1 w-full rounded-card border bg-bg px-3 py-1.5 text-ink"
        >
          <option value="">-- choose an org --</option>
          {orgs.map((org) => (
            <option key={org.id} value={org.id}>
              {org.name}
            </option>
          ))}
        </select>

        <div className="mt-3 flex flex-wrap items-center gap-3 text-sm">
          <Link to="/org/new" className="text-brand hover:underline">
            Create an organization
          </Link>
          {currentOrg && canManageTeam && (
            <Link to="/org" className="text-brand hover:underline">
              Manage {currentOrg.name}
            </Link>
          )}
        </div>

        {currentOrg && (
          <AsyncButton
            onClick={leave}
            className="mt-4 text-sm font-medium text-danger hover:underline"
          >
            Leave {currentOrg.name}
          </AsyncButton>
        )}
      </section>

      <section className="rounded-card border bg-surface p-5">
        <label className="block text-sm text-ink-muted">Theme</label>
        <ThemeToggle className="mt-2" />
      </section>

      {error && <p className="text-sm text-danger">{error}</p>}
    </main>
  );
}
