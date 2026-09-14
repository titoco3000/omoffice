import { useState } from "react";
import { useAuth } from "../auth/AuthContext";
import { useOrgSession } from "../org/OrgSessionContext";
import { navigate } from "../router/Router";
import { createOrg } from "../lib/rbac";

export default function OrgNew() {
  const { token } = useAuth();
  const { reloadOrgs, selectOrg } = useOrgSession();
  const [name, setName] = useState("");
  const [error, setError] = useState(null);

  const submit = async (event) => {
    event.preventDefault();
    if (!name.trim()) return;
    try {
      const { org } = await createOrg(token, name.trim());
      await reloadOrgs();
      selectOrg(org.id);
      navigate("/in");
    } catch (err) {
      setError(err.message);
    }
  };

  return (
    <main className="mx-auto flex min-h-full max-w-sm flex-col justify-center gap-3 px-6 py-10">
      <h1 className="text-2xl font-semibold text-ink">
        Create an organization
      </h1>
      <form onSubmit={submit} className="flex flex-col gap-3">
        <input
          value={name}
          onChange={(event) => setName(event.target.value)}
          placeholder="Organization name"
          autoFocus
          className="rounded-card border bg-surface px-3 py-2 text-ink"
        />
        {error && <p className="text-sm text-danger">{error}</p>}
        <button className="rounded-card bg-brand px-4 py-2 font-medium text-on-brand hover:bg-brand-strong">
          Create
        </button>
      </form>
    </main>
  );
}
