import { apiFetch } from "./api";

export const MEETING_LIST_CHANNEL = "meeting_list";
export const IDLE_AFTER_SECONDS = 55;
export const MEETING_ROOM_CHANNEL = "meeting_room";

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
