import { useRef, useState } from "react";
import { useAuth } from "../core/auth/AuthContext";
import { navigate } from "../core/Router";
import { useOrgSession, createOrg } from "../features/org";
import AsyncButton from "../core/ui/AsyncButton";

export default function OrgNew() {
  const { token } = useAuth();
  const { reloadOrgs, selectOrg } = useOrgSession();
  const [name, setName] = useState("");
  const [error, setError] = useState(null);
  // Enter-to-submit routes through the same button `AsyncButton` renders,
  // rather than a separate submit path with its own loading state --
  // implicit form submission fires the form's `submit` event, not a click,
  // so this is the one way to still land on the button's own handler.
  const buttonRef = useRef(null);

  const preventImplicitSubmit = (event) => {
    event.preventDefault();
    if (name.trim()) buttonRef.current?.click();
  };

  const create = async () => {
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
      <form onSubmit={preventImplicitSubmit} className="flex flex-col gap-3">
        <input
          value={name}
          onChange={(event) => setName(event.target.value)}
          placeholder="Organization name"
          autoFocus
          className="rounded-card border bg-surface px-3 py-2 text-ink"
        />
        {error && <p className="text-sm text-danger">{error}</p>}
        <AsyncButton
          ref={buttonRef}
          onClick={create}
          disabled={!name.trim()}
          className="justify-center rounded-card bg-brand px-4 py-2 font-medium text-on-brand hover:bg-brand-strong disabled:opacity-50"
        >
          Create
        </AsyncButton>
      </form>
    </main>
  );
}
