import { callApi } from "../api";
import { useAuth } from "../auth/AuthContext";
import { useActiveMeeting } from "../meeting/MeetingContext";
import { useNavigate } from "../router";

/**
 * Accepting an org or meeting invite, shared between `PendingInvites` and
 * the invite toast so the two don't drift. An accepted meeting invite is
 * also an instruction to go there, which is the navigation below.
 */
export function useInviteActions() {
  const { loginWithToken } = useAuth();
  const { join } = useActiveMeeting();
  const navigate = useNavigate();

  async function acceptOrg(inviteId) {
    const response = await callApi({
      method: "POST",
      path: `/invites/org/${encodeURIComponent(inviteId)}/accept`,
    });
    if (response.ok) loginWithToken(response.data.token);
    return response;
  }

  async function acceptMeeting(inviteId) {
    const response = await callApi({
      method: "POST",
      path: `/invites/meetings/${encodeURIComponent(inviteId)}/accept`,
    });
    if (response.ok) {
      loginWithToken(response.data.token);
      join(response.data.code);
      navigate(`/meeting/${encodeURIComponent(response.data.code)}`);
    }
    return response;
  }

  return { acceptOrg, acceptMeeting };
}
