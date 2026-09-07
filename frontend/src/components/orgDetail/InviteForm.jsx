import { useState } from "react";
import { callApi, describeFailure } from "../../api";
import RoleCheckboxes from "../RoleCheckboxes";

export default function InviteForm({ org, token, onInvited }) {
  const [email, setEmail] = useState("");
  // No roles checked is a real choice, not an empty form: the backend folds
  // in its hidden default role, so this invites a plain member.
  const [roles, setRoles] = useState([]);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState("");

  async function submit(event) {
    event.preventDefault();
    const wanted = email.trim();
    if (!wanted) return;
    setError("");
    setSubmitting(true);
    // A pending invite, not instant membership — joins only once accepted.
    const response = await callApi({
      method: "POST",
      path: `/private/orgs/${encodeURIComponent(org.id)}/invites`,
      token,
      body: { email: wanted, roles },
    });
    setSubmitting(false);
    if (!response.ok) {
      setError(describeFailure(response));
      return;
    }
    setEmail("");
    onInvited();
  }

  return (
    <form
      onSubmit={submit}
      className="mt-3 flex flex-wrap items-center gap-3 border-t border-slate-800 pt-3"
    >
      <input
        type="email"
        required
        value={email}
        onChange={(event) => setEmail(event.target.value)}
        placeholder="invite@example.com"
        className="h-9 min-w-0 flex-1 rounded-lg border border-slate-700 bg-slate-950 px-2 font-mono text-sm text-slate-100 outline-none focus:border-sky-500"
      />
      <RoleCheckboxes roles={roles} onChange={setRoles} />
      <button
        type="submit"
        disabled={submitting || !email.trim()}
        className="rounded-lg bg-sky-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-sky-500 disabled:opacity-50"
      >
        {submitting ? "..." : "invite"}
      </button>
      {error && (
        <p className="w-full font-mono text-xs break-all text-rose-300">{error}</p>
      )}
    </form>
  );
}
