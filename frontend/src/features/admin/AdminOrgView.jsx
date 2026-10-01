import { useEffect, useState } from "react";
import { useAuth } from "../../core/auth/AuthContext";
import { Link, navigate } from "../../core/Router";
import AsyncButton from "../../core/ui/AsyncButton";
import Avatar from "../../core/ui/Avatar";
import Spinner from "../../core/ui/Spinner";
import { AvatarUploadPanel, avatarUrl } from "../avatars";
import { StaticMapCanvas } from "../map";
import {
  SysAdminUsersManagement,
  deleteOrg,
  renameOrg,
  useOrgRoster,
} from "../org";
import { getAdminOrg } from "./api";

// Everything an org's own admin can do on `/org` -- rename, icon, members,
// roles, invites, delete -- plus its map. The backend's `require_role` lets a
// sys-admin through, so these are the regular `/rbac/org/{id}/*` calls.
export default function AdminOrgView({ orgId }) {
  const { token } = useAuth();
  const [org, setOrg] = useState(null);
  const [error, setError] = useState(null);
  const [editingIcon, setEditingIcon] = useState(false);
  const { members } = useOrgRoster(token, orgId);

  const load = () =>
    getAdminOrg(token, orgId)
      .then((data) => setOrg(data.org))
      .catch((err) => setError(err.message));

  useEffect(() => {
    load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [token, orgId]);

  if (!org) {
    return error ? (
      <p className="text-sm text-danger">{error}</p>
    ) : (
      <Spinner className="size-5" />
    );
  }

  const saveName = (event) => {
    const value = event.target.value.trim();
    if (!value || value === org.name) return;
    renameOrg(token, orgId, value)
      .then(load)
      .catch((err) => setError(err.message));
  };

  const remove = async () => {
    if (!window.confirm(`Delete ${org.name}? This cannot be undone.`)) return;
    try {
      await deleteOrg(token, orgId);
      navigate("/admin");
    } catch (err) {
      setError(err.message);
    }
  };

  return (
    <>
      <Link to="/admin" className="text-sm text-brand hover:underline">
        Back to admin
      </Link>

      <div className="flex items-center gap-3">
        <Avatar url={avatarUrl(org.picture)} name={org.name} size="size-14" />
        <div className="flex-1">
          <input
            key={org.name}
            defaultValue={org.name}
            onBlur={saveName}
            className="w-full rounded-card border bg-bg px-3 py-1.5 text-2xl font-semibold text-ink"
          />
          <button
            onClick={() => setEditingIcon(true)}
            className="mt-1 text-sm text-brand hover:underline"
          >
            Change icon
          </button>
          <p className="text-sm text-ink-muted">
            {members.length} users,{" "}
            {members.filter((member) => member.online).length} online
          </p>
        </div>
      </div>

      {editingIcon && (
        <AvatarUploadPanel
          title={`Icon for ${org.name}`}
          target={{ target: "org_icon", org_id: orgId }}
          onClose={() => setEditingIcon(false)}
          onDone={load}
        />
      )}

      <section className="rounded-card border bg-surface p-5">
        <h2 className="mb-2 text-sm text-ink-muted">Map</h2>
        <StaticMapCanvas token={token} orgId={orgId} members={members} />
      </section>

      <SysAdminUsersManagement org={org} />

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

      {error && <p className="text-sm text-danger">{error}</p>}
    </>
  );
}
