import { useState } from "react";
import { useAuth } from "../auth/AuthContext";
import MeetingHistory from "../components/MeetingHistory";
import PendingInvites from "../components/PendingInvites";
import PageHeader from "../layout/PageHeader";
import OrgUserList from "../layout/OrgUserList";
import { useActiveMeeting } from "../meeting/MeetingContext";
import { useOrgContext } from "../org/OrgContext";
import { useNavigate } from "../router";
import Button from "../ui/Button";
import Card, { CardHeader } from "../ui/Card";
import Input from "../ui/Input";

/**
 * `/users` — the roster as a page. On a phone this is where the people are,
 * since there is no sidebar to put them in; on a desktop it repeats the
 * sidebar's list and adds the two things too big for it: joining a meeting
 * by its code, and the org's past meetings.
 */
export default function UsersPage() {
  const { token } = useAuth();
  const { selectedOrg, selectedOrgId } = useOrgContext();
  const { join } = useActiveMeeting();
  const navigate = useNavigate();
  const [codeInput, setCodeInput] = useState("");

  function enter(code) {
    join(code);
    navigate(`/meeting/${encodeURIComponent(code)}`);
  }

  function submitCode(event) {
    event.preventDefault();
    const wanted = codeInput.trim();
    if (wanted) enter(wanted);
  }

  return (
    <>
      <PageHeader title="people" subtitle={selectedOrg?.name} />

      <div className="mx-auto grid w-full max-w-2xl min-h-0 gap-4 overflow-y-auto p-4 pb-10">
        <PendingInvites />

        <Card>
          <CardHeader title="who is around" subtitle="click a name to meet" />
          <OrgUserList />
        </Card>

        <Card>
          <CardHeader title="join by code" subtitle="for a meeting shared with you" />
          <form onSubmit={submitCode} className="flex gap-2">
            <Input
              mono
              value={codeInput}
              onChange={(event) => setCodeInput(event.target.value)}
              placeholder="meeting code"
              className="flex-1"
            />
            <Button type="submit" disabled={!codeInput.trim()}>
              join
            </Button>
          </form>
        </Card>

        <Card>
          <CardHeader title="org meetings" subtitle="every meeting this org has made" />
          <MeetingHistory token={token} orgId={selectedOrgId} onPick={enter} />
        </Card>
      </div>
    </>
  );
}
