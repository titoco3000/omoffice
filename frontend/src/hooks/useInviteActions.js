import { callApi } from "../api";
import { useAuth } from "../auth/AuthContext";

/**
 * Accepting an org or meeting invite, shared between `PendingInvites` (the
 * banner, for someone who logged in on their own) and `Toast` (clicking the
 * live notification) so the two don't drift: both public accept routes
 * (`/invites/org/{id}/accept`, `/invites/meetings/{id}/accept`) hand back a
 * token, which this adopts via `loginWithToken` -- for a meeting, also
 * handing the code up through `onJoinMeeting` so `Shell` can switch to the
 * meetings tab with it prefilled (there's no router to carry that as a URL).
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
