import { apiFetch } from "./api";

export const MEETING_LIST_CHANNEL = "meeting_list";
export const IDLE_AFTER_SECONDS = 55;
export const MEETING_ROOM_CHANNEL = "meeting_room";
export const MEETING_INVITES_CHANNEL = "meeting_invites";
export const MEETING_FORCE_CHANNEL = "meeting_force";

export const listMeetings = (token, orgId) =>
  apiFetch(`/meeting?org_id=${encodeURIComponent(orgId)}`, { token });

export const createMeeting = (token, orgId, inviteOnly = false) =>
  apiFetch("/meeting", {
    method: "POST",
    token,
    body: { org_id: orgId, invite_only: inviteOnly },
  });

export const joinMeeting = (token, meetingId) =>
  apiFetch(`/meeting/${meetingId}/join`, { method: "POST", token });

export const leaveMeeting = (token, meetingId) =>
  apiFetch(`/meeting/${meetingId}/leave`, { method: "POST", token });

export const listMyInvites = (token) => apiFetch("/meeting/invites", { token });

export const inviteToMeeting = (token, meetingId, invitedUserId) =>
  apiFetch(`/meeting/${meetingId}/invites`, {
    method: "POST",
    token,
    body: { invited_user_id: invitedUserId },
  });

export const revokeMeetingInvite = (token, meetingId, inviteId) =>
  apiFetch(`/meeting/${meetingId}/invites/${inviteId}`, {
    method: "DELETE",
    token,
  });

export const acceptMeetingInvite = (token, inviteId) =>
  apiFetch(`/meeting/invites/${inviteId}/accept`, { method: "POST", token });

export const forceJoinMeeting = (token, meetingId, userId) =>
  apiFetch(`/meeting/${meetingId}/force`, {
    method: "POST",
    token,
    body: { user_id: userId },
  });

export const declineMeetingInvite = (token, inviteId) =>
  apiFetch(`/meeting/invites/${inviteId}/decline`, { method: "POST", token });
