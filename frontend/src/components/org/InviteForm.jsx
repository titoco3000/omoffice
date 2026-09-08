import { useState } from "react";
import { callApi, describeFailure } from "../../api";
import Button from "../../ui/Button";
import Input from "../../ui/Input";
import Notice from "../../ui/Notice";
import RoleCheckboxes from "./RoleCheckboxes";

/// Invite one email into `org`, with the roles it should get on accepting.
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
    <form onSubmit={submit} className="grid gap-3">
      <div className="flex flex-wrap items-center gap-2">
        <Input
          type="email"
          mono
          required
          value={email}
          onChange={(event) => setEmail(event.target.value)}
          placeholder="invite@example.com"
          className="flex-1"
        />
        <Button type="submit" disabled={submitting || !email.trim()}>
          {submitting ? "..." : "invite"}
        </Button>
      </div>
      <RoleCheckboxes roles={roles} onChange={setRoles} />
      {error && <Notice>{error}</Notice>}
    </form>
  );
}
