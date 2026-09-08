import { useEffect, useState } from "react";
import { callApi, describeFailure } from "../api";
import { useAuth } from "../auth/AuthContext";
import InviteForm from "../components/org/InviteForm";
import MemberRow from "../components/org/MemberRow";
import PendingOrgInvites from "../components/org/PendingOrgInvites";
import PageHeader from "../layout/PageHeader";
import { useOrgContext } from "../org/OrgContext";
import { hasRole } from "../rbac";
import { Redirect, useNavigate } from "../router";
import Button from "../ui/Button";
import Card, { CardHeader } from "../ui/Card";
import Notice from "../ui/Notice";
import { IconOrg } from "../ui/icons";

/// `/org` — the selected org itself: who is in it, what they may do, and
/// the one button that ends it. Which org this is comes from `OrgContext`,
/// the same selection the sidebar's roster uses.
export default function OrgPage() {
  const { token, email: myEmail } = useAuth();
  const { orgs, selectedOrg, reload } = useOrgContext();
  const navigate = useNavigate();
  const [notice, setNotice] = useState("");
  const [reloads, setReloads] = useState(0);
  // Same "stamp" pattern as `useOrgs`: loading is derived from whether the
  // held result matches the current fetch.
  const [result, setResult] = useState({ stamp: null, members: [], error: "" });

  const orgId = selectedOrg?.id ?? null;
  const stamp = `${orgId}:${token}:${reloads}`;
  const loading = Boolean(orgId) && result.stamp !== stamp;

  useEffect(() => {
    if (!orgId) return undefined;
    let cancelled = false;
    callApi({ path: `/private/orgs/${encodeURIComponent(orgId)}`, token }).then(
      (response) => {
        if (cancelled) return;
        setResult({
          stamp: `${orgId}:${token}:${reloads}`,
          members: response.ok ? (response.data.members ?? []) : [],
          error: response.ok ? "" : describeFailure(response),
        });
      },
    );
    return () => {
      cancelled = true;
    };
  }, [orgId, token, reloads]);

  if (!selectedOrg) return <Redirect to="/me" />;

  const canManage = hasRole(orgs, selectedOrg.id, "team-management");
  const canDelete = hasRole(orgs, selectedOrg.id, "admin");
  const shown = result.stamp === stamp ? result : { members: [], error: "" };

  function refresh() {
    setReloads((count) => count + 1);
  }

  async function deleteOrg() {
    setNotice("");
    const response = await callApi({
      method: "DELETE",
      path: `/private/orgs/${encodeURIComponent(selectedOrg.id)}`,
      token,
    });
    if (!response.ok) {
      setNotice(describeFailure(response));
      return;
    }
    reload();
    navigate("/me");
  }

  return (
    <>
      <PageHeader title="organization" subtitle={selectedOrg.name} />

      <div className="mx-auto grid w-full max-w-3xl min-h-0 gap-4 overflow-y-auto p-4 pb-10">
        <Card>
          <div className="flex flex-wrap items-center gap-4">
            <span className="inline-flex h-14 w-14 items-center justify-center rounded-card bg-brand/15 text-brand">
              <IconOrg className="h-7 w-7" />
            </span>
            <div className="min-w-0 flex-1">
              <p className="truncate text-lg font-semibold text-ink">
                {selectedOrg.name}
              </p>
              <p className="truncate font-mono text-xs text-ink-faint">
                {selectedOrg.id}
              </p>
            </div>
          </div>
        </Card>

        <Card>
          <CardHeader
            title="members"
            subtitle="roles decide what each of them may do"
            actions={
              <Button variant="ghost" size="sm" onClick={refresh} disabled={loading}>
                {loading ? "..." : "refresh"}
              </Button>
            }
          />
          {shown.error && <Notice className="mb-3">{shown.error}</Notice>}
          <ul className="rounded-lg border border-line">
            {shown.members.map((member) => (
              <MemberRow
                key={member.email}
                member={member}
                you={member.email === myEmail}
                canManage={canManage}
                org={selectedOrg}
                token={token}
                onChanged={refresh}
              />
            ))}
            {shown.members.length === 0 && (
              <li className="px-3 py-2.5 text-xs text-ink-faint">
                {loading ? "loading..." : "nobody yet"}
              </li>
            )}
          </ul>
        </Card>

        {canManage && (
          <Card>
            <CardHeader
              title="add someone"
              subtitle="they join once they accept the invite"
            />
            <InviteForm org={selectedOrg} token={token} onInvited={refresh} />
            <div className="mt-4 border-t border-line pt-4">
              <PendingOrgInvites org={selectedOrg} token={token} />
            </div>
          </Card>
        )}

        {canDelete && (
          <Card className="border-danger/40">
            <CardHeader title="danger zone" subtitle="this cannot be undone" />
            {notice && <Notice className="mb-3">{notice}</Notice>}
            <div className="flex flex-wrap items-center justify-between gap-3">
              <p className="text-sm text-ink-muted">
                delete <span className="font-medium text-ink">{selectedOrg.name}</span>,
                its membership and its invites.
              </p>
              <Button variant="danger" onClick={deleteOrg}>
                delete organization
              </Button>
            </div>
          </Card>
        )}
      </div>
    </>
  );
}
