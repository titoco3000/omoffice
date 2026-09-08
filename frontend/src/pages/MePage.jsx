import { useState } from "react";
import { API_URL, callApi, describeFailure, STAGE } from "../api";
import { useAuth } from "../auth/AuthContext";
import CreateOrgForm from "../components/org/CreateOrgForm";
import PendingInvites from "../components/PendingInvites";
import ThemeToggle from "../components/ThemeToggle";
import DeveloperTools from "../components/DeveloperTools";
import { useOrgContext } from "../org/OrgContext";
import { hasRole, visibleRoles } from "../rbac";
import { Link, useNavigate } from "../router";
import Avatar from "../ui/Avatar";
import Badge from "../ui/Badge";
import Button from "../ui/Button";
import Card, { CardHeader } from "../ui/Card";
import Dropdown, { MenuItem } from "../ui/Dropdown";
import Notice from "../ui/Notice";
import PageHeader from "../layout/PageHeader";
import { IconChevronDown, IconOrg, IconPlus } from "../ui/icons";

/// `/me` — who you are, which org you are acting in, and the settings that
/// belong to a person rather than an org.
export default function MePage() {
  const { token, email, logout } = useAuth();
  const { orgs, selectedOrg, selectOrg, reload } = useOrgContext();
  const navigate = useNavigate();
  const [creating, setCreating] = useState(false);
  const [notice, setNotice] = useState("");

  const canManageOrg = selectedOrg
    ? hasRole(orgs, selectedOrg.id, "team-management")
    : false;

  async function leaveOrg() {
    if (!selectedOrg) return;
    setNotice("");
    const response = await callApi({
      method: "DELETE",
      path: `/private/orgs/${encodeURIComponent(selectedOrg.id)}/members/me`,
      token,
    });
    if (!response.ok) {
      setNotice(describeFailure(response));
      return;
    }
    reload();
  }

  return (
    <>
      <PageHeader title="profile" />

      <div className="mx-auto grid w-full max-w-2xl min-h-0 gap-4 overflow-y-auto p-4 pb-10">
        <Card>
          <div className="flex flex-wrap items-center gap-4">
            <Avatar email={email} size="lg" />
            <div className="min-w-0 flex-1">
              <p className="truncate font-mono text-sm text-ink">{email}</p>
              <p className="mt-0.5 text-xs text-ink-faint">
                signed in — tokens last a week
              </p>
            </div>
            <Button
              variant="outline"
              onClick={() => {
                logout();
                navigate("/");
              }}
            >
              log out
            </Button>
          </div>
        </Card>

        <PendingInvites />

        <Card>
          <CardHeader
            title="organization"
            subtitle="the org your roster, chat and meetings belong to"
            actions={
              canManageOrg && (
                <Link
                  to="/org"
                  className="inline-flex h-8 items-center gap-2 rounded-lg border border-line px-3 text-xs font-medium text-ink transition-colors hover:bg-muted"
                >
                  <IconOrg className="h-4 w-4" />
                  manage
                </Link>
              )
            }
          />

          <div className="flex flex-wrap items-center gap-2">
            <Dropdown
              trigger={({ toggle }) => (
                <Button variant="outline" onClick={toggle} className="min-w-48 justify-between!">
                  <span className="truncate">
                    {selectedOrg ? selectedOrg.name : "no org yet"}
                  </span>
                  <IconChevronDown className="h-4 w-4 shrink-0" />
                </Button>
              )}
            >
              {(close) => (
                <>
                  {orgs.map((org) => (
                    <MenuItem
                      key={org.id}
                      onClick={() => {
                        selectOrg(org.id);
                        close();
                      }}
                    >
                      <span className="min-w-0 flex-1 truncate">{org.name}</span>
                      {org.id === selectedOrg?.id && <Badge tone="brand">current</Badge>}
                    </MenuItem>
                  ))}
                  <MenuItem
                    onClick={() => {
                      close();
                      setCreating(true);
                    }}
                  >
                    <IconPlus className="h-4 w-4" />
                    new organization
                  </MenuItem>
                </>
              )}
            </Dropdown>

            {selectedOrg && (
              <span className="flex flex-wrap gap-1">
                {visibleRoles(selectedOrg.roles).map((role) => (
                  <Badge key={role} tone="brand">
                    {role}
                  </Badge>
                ))}
              </span>
            )}
          </div>

          {creating && (
            <div className="mt-4 border-t border-line pt-4">
              <CreateOrgForm
                token={token}
                onCancel={() => setCreating(false)}
                onCreated={(id) => {
                  setCreating(false);
                  reload();
                  if (id) selectOrg(id);
                }}
              />
            </div>
          )}
        </Card>

        <Card>
          <CardHeader title="settings" subtitle="kept in this browser" />
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <p className="text-sm text-ink">appearance</p>
              <p className="text-xs text-ink-faint">system follows your OS</p>
            </div>
            <ThemeToggle />
          </div>
          <div className="mt-4 flex flex-wrap items-center justify-between gap-3 border-t border-line pt-4">
            <div className="min-w-0">
              <p className="text-sm text-ink">api</p>
              <p className="truncate font-mono text-xs text-ink-faint">{API_URL}</p>
            </div>
            <Badge>{STAGE}</Badge>
          </div>
        </Card>

        <DeveloperTools token={token} />

        <Card className="border-danger/40">
          <CardHeader title="danger zone" subtitle="only affects you" />
          {notice && <Notice className="mb-3">{notice}</Notice>}
          <div className="flex flex-wrap items-center justify-between gap-3">
            <p className="text-sm text-ink-muted">
              leave{" "}
              <span className="font-medium text-ink">
                {selectedOrg ? selectedOrg.name : "—"}
              </span>
              . the backend refuses if you are its last admin.
            </p>
            <Button variant="danger" disabled={!selectedOrg} onClick={leaveOrg}>
              leave org
            </Button>
          </div>
        </Card>
      </div>
    </>
  );
}
