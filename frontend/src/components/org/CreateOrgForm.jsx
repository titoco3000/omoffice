import { useState } from "react";
import { callApi, describeFailure } from "../../api";
import Button from "../../ui/Button";
import Input from "../../ui/Input";
import Notice from "../../ui/Notice";
import RoleCheckboxes from "./RoleCheckboxes";

/// A new org, plus whoever should be invited into it on day one. Its
/// creator is always its first `admin` (`rbac::create_org`).
export default function CreateOrgForm({ token, onCreated, onCancel }) {
  const [name, setName] = useState("");
  const [invites, setInvites] = useState([]);
  const [error, setError] = useState("");
  const [submitting, setSubmitting] = useState(false);

  function updateInviteRow(index, field, value) {
    setInvites((rows) =>
      rows.map((row, i) => (i === index ? { ...row, [field]: value } : row)),
    );
  }

  async function submit(event) {
    event.preventDefault();
    const wanted = name.trim();
    if (!wanted) return;
    setError("");
    setSubmitting(true);
    const response = await callApi({
      method: "POST",
      path: "/private/orgs",
      token,
      body: {
        name: wanted,
        // A row with no roles checked is still a real invite — the backend
        // folds in its own hidden default role.
        invites: invites.filter((row) => row.email.trim()),
      },
    });
    setSubmitting(false);
    if (!response.ok) {
      setError(describeFailure(response));
      return;
    }
    onCreated(response.data?.org?.id ?? null);
  }

  return (
    <form onSubmit={submit} className="grid gap-3">
      <Input
        type="text"
        required
        autoFocus
        value={name}
        onChange={(event) => setName(event.target.value)}
        placeholder="organization name"
      />

      {invites.map((row, index) => (
        <div key={index} className="grid gap-2 rounded-lg bg-muted p-2">
          <div className="flex items-center gap-2">
            <Input
              type="email"
              mono
              value={row.email}
              onChange={(event) => updateInviteRow(index, "email", event.target.value)}
              placeholder="invite@example.com"
              className="flex-1"
            />
            <Button
              variant="ghost"
              size="sm"
              onClick={() => setInvites((rows) => rows.filter((_, i) => i !== index))}
            >
              remove
            </Button>
          </div>
          <RoleCheckboxes
            roles={row.roles}
            onChange={(roles) => updateInviteRow(index, "roles", roles)}
          />
        </div>
      ))}

      <div className="flex items-center gap-2">
        <Button
          variant="ghost"
          size="sm"
          onClick={() => setInvites((rows) => [...rows, { email: "", roles: [] }])}
        >
          + invite someone
        </Button>
        {onCancel && (
          <Button variant="ghost" size="sm" onClick={onCancel}>
            cancel
          </Button>
        )}
        <Button type="submit" className="ml-auto" disabled={submitting || !name.trim()}>
          {submitting ? "..." : "create"}
        </Button>
      </div>

      {error && <Notice>{error}</Notice>}
    </form>
  );
}
