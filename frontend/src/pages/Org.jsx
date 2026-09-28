import { useState } from "react";
import { useAuth } from "../core/auth/AuthContext";
import { navigate } from "../core/Router";
import {
  useOrgSession,
  UsersManagement,
  renameOrg,
  deleteOrg,
} from "../features/org";
import { avatarUrl, AvatarUploadPanel } from "../features/avatars";
import AsyncButton from "../core/ui/AsyncButton";
import Avatar from "../core/ui/Avatar";

export default function Org() {
  const { token } = useAuth();
  const { currentOrg, isAdmin, canManageTeam, reloadOrgs, selectOrg } =
    useOrgSession();
  const [error, setError] = useState(null);
  const [editingIcon, setEditingIcon] = useState(false);

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
      <div className="flex items-center gap-3">
        <Avatar
          url={avatarUrl(currentOrg.picture)}
          name={currentOrg.name}
          size="size-14"
        />
        <div className="flex-1">
          {isAdmin ? (
            <input
              key={currentOrg.id}
              defaultValue={currentOrg.name}
              onBlur={saveName}
              className="w-full rounded-card border bg-bg px-3 py-1.5 text-2xl font-semibold text-ink"
            />
          ) : (
            <h1 className="text-2xl font-semibold text-ink">
              {currentOrg.name}
            </h1>
          )}
          {isAdmin && (
            <button
              onClick={() => setEditingIcon(true)}
              className="mt-1 text-sm text-brand hover:underline"
            >
              Change icon
            </button>
          )}
        </div>
      </div>

      {editingIcon && (
        <AvatarUploadPanel
          title={`Icon for ${currentOrg.name}`}
          target={{ target: "org_icon", org_id: currentOrg.id }}
          onClose={() => setEditingIcon(false)}
          onDone={() => reloadOrgs()}
        />
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
