import { callApi } from "../api";
import { useAuth } from "../auth/AuthContext";

/**
 * Accepting an org or meeting invite, shared between `PendingInvites` and
 * `Toast` so the two don't drift. `onJoinMeeting` carries the code to `Shell`.
 */
export function useInviteActions({ onJoinMeeting } = {}) {
  const { loginWithToken } = useAuth();

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
      onJoinMeeting?.(response.data.code);
    }
    return response;
  }

  return { acceptOrg, acceptMeeting };
}
