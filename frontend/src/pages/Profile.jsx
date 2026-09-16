import { useEffect, useState } from "react";
import { useAuth } from "../auth/AuthContext";
import { useOrgSession } from "../org/OrgSessionContext";
import { Link } from "../router/Router";
import { me, renameMe, meInOrg, renameMeInOrg, leaveOrg } from "../lib/rbac";
import { avatarUrl, removeAvatar } from "../lib/avatars";
import ThemeToggle from "../theme/ThemeToggle";
import AsyncButton from "../components/AsyncButton";
import Avatar from "../components/Avatar";
import AvatarUploadPanel from "../components/AvatarUploadPanel";

export default function Profile() {
  const { token, logout } = useAuth();
  const {
    orgs,
    orgId,
    currentOrg,
    selectOrg,
    reloadOrgs,
    canManageTeam,
    reloadProfile,
  } = useOrgSession();

  const [name, setName] = useState("");
  const [picture, setPicture] = useState(null);
  const [error, setError] = useState(null);
  const [editingPhoto, setEditingPhoto] = useState(null); // null | "me" | "org"
  // The caller's own raw name/picture/roles within `currentOrg` -- no
  // fallback to the global name/photo, unlike a roster entry -- see
  // `GET /rbac/me/org/{org_id}`.
  const [selfInOrg, setSelfInOrg] = useState(null);

  useEffect(() => {
    me(token)
      .then((data) => {
        setName(data.name ?? "");
        setPicture(data.picture ?? null);
      })
      .catch(() => {});
  }, [token]);

  const reloadSelfInOrg = () => {
    if (!currentOrg) return Promise.resolve();
    return meInOrg(token, currentOrg.id)
      .then(setSelfInOrg)
      .catch(() => {});
  };

  useEffect(() => {
    if (!currentOrg?.id) return;
    reloadSelfInOrg();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [token, currentOrg?.id]);

  const saveName = () =>
    renameMe(token, name).catch((err) => setError(err.message));
  const saveOrgName = (event) => {
    const value = event.target.value.trim();
    renameMeInOrg(token, currentOrg.id, value)
      .then(reloadSelfInOrg)
      .catch((err) => setError(err.message));
  };
  const orgDisplayName = selfInOrg?.name ?? "";
  const orgPicture = selfInOrg?.picture ?? null;

  const removeMyPhoto = () =>
    removeAvatar(token, { target: "me" })
      .then(() => {
        setPicture(null);
        reloadProfile?.();
      })
      .catch((err) => setError(err.message));

  const removeOrgPhoto = () =>
    removeAvatar(token, { target: "org_me", org_id: currentOrg.id })
      .then(reloadSelfInOrg)
      .catch((err) => setError(err.message));

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
        <div className="flex items-center gap-3">
          <Avatar url={avatarUrl(picture)} name={name || "?"} size="size-14" />
          <div className="flex flex-col items-start gap-1">
            <button
              onClick={() => setEditingPhoto("me")}
              className="text-sm text-brand hover:underline"
            >
              Change photo
            </button>
            {picture && (
              <AsyncButton
                onClick={removeMyPhoto}
                className="text-sm text-danger hover:underline"
              >
                Remove photo
              </AsyncButton>
            )}
          </div>
        </div>

        <label className="mt-4 block text-sm text-ink-muted">
          Display name
        </label>
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

            <div className="mt-3 flex items-center gap-3">
              {orgPicture && (
                <Avatar
                  url={avatarUrl(orgPicture)}
                  name={orgDisplayName || "?"}
                  size="size-10"
                />
              )}
              <div className="flex flex-col items-start gap-1">
                <button
                  onClick={() => setEditingPhoto("org")}
                  className="text-sm text-brand hover:underline"
                >
                  {orgPicture
                    ? `Change photo in ${currentOrg.name}`
                    : `Override photo in ${currentOrg.name}`}
                </button>
                {orgPicture && (
                  <AsyncButton
                    onClick={removeOrgPhoto}
                    className="text-sm text-danger hover:underline"
                  >
                    Remove photo in {currentOrg.name}
                  </AsyncButton>
                )}
              </div>
            </div>
          </>
        )}
      </section>

      {editingPhoto === "me" && (
        <AvatarUploadPanel
          title="Profile photo"
          target={{ target: "me" }}
          onClose={() => setEditingPhoto(null)}
          onDone={setPicture}
        />
      )}
      {editingPhoto === "org" && currentOrg && (
        <AvatarUploadPanel
          title={`Photo in ${currentOrg.name}`}
          target={{ target: "org_me", org_id: currentOrg.id }}
          onClose={() => setEditingPhoto(null)}
          onDone={reloadSelfInOrg}
        />
      )}

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
