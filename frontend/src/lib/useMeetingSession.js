//! Shared meeting/invite state for a whole session -- one Chime session per
//! tab (see lib/chime.js), so this lives above `MeetingsPanel`, `InvitesPanel`
//! and `PeoplePanel` rather than inside any one of them: accepting an
//! invite from `InvitesPanel` or joining someone's meeting from
//! `PeoplePanel` has to land in the same `myMeetingId`/`roomStatus` state
//! `MeetingsPanel` renders, not a copy of it.
//!
//! `orgId` is which org's panels are currently open, not which org a
//! meeting happens to belong to -- switching it leaves whatever meeting
//! this tab is in (same as the idle-alone timeout: a real `leave` call, not
//! just a local reset), and narrows `invites` down to the ones for that
//! org. The backend itself has no notion of "current org" and hands back
//! every pending invite regardless -- filtering to `orgId` happens here,
//! client-side, on every render.

import { useEffect, useRef, useState } from "react";
import { messenger } from "./messenger";
import { decodeJwt } from "./jwt";
import * as chime from "./chime";
import {
  IDLE_AFTER_SECONDS,
  MEETING_CHAT_CHANNEL,
  MEETING_FORCE_CHANNEL,
  MEETING_INVITES_CHANNEL,
  MEETING_LIST_CHANNEL,
  MEETING_ROOM_CHANNEL,
  acceptMeetingInvite,
  createMeeting,
  declineMeetingInvite,
  forceJoinMeeting,
  inviteToMeeting,
  joinMeeting,
  leaveMeeting,
  listMeetings,
  listMyInvites,
} from "./meetings";

export function useMeetingSession(token, orgId) {
  const audioRef = useRef(null);
  const selfVideoRef = useRef(null);

  const [meetings, setMeetings] = useState([]);
  // The meeting this tab currently considers itself "in", and whether that
  // means an actual live Chime session or just the app-level fake-connected
  // state a solo attendee sits in -- see `features::meetings::join`
  // (backend/lib/features/meetings/mod.rs).
  const [myMeetingId, setMyMeetingId] = useState(null);
  const [roomStatus, setRoomStatus] = useState(null); // "waiting" | "connected" | null
  const [cameraOn, setCameraOnState] = useState(false);
  const [micMuted, setMicMutedState] = useState(false);
  const [sharingScreen, setSharingScreen] = useState(false);
  const [remoteTiles, setRemoteTiles] = useState([]); // [{tileId, attendeeId, isContent}]
  // Every pending invite the backend knows about for this user, regardless
  // of org -- narrowed to `orgId` below, before this hook hands anything
  // back.
  const [allInvites, setAllInvites] = useState([]);
  const [error, setError] = useState(null);
  // In-meeting chat -- frontend state only, the backend never stores a
  // message, just relays it to whoever else is currently an attendee, so
  // this array is the only copy that exists anywhere. Reset whenever
  // `myMeetingId` changes -- a fresh meeting (joined, switched, or this tab
  // just left) starts with no history, same as anyone else opening it late
  // would see.
  const [chatMessages, setChatMessages] = useState([]);
  const myId = decodeJwt(token)?.sub;

  // Mirrors `myMeetingId` for the unmount cleanup below, which is set up
  // once and would otherwise only ever see whatever it was at that moment,
  // not whatever it is by the time the tab actually leaves.
  const myMeetingIdRef = useRef(null);
  useEffect(() => {
    myMeetingIdRef.current = myMeetingId;
  }, [myMeetingId]);

  useEffect(() => {
    chime.setAudioElement(audioRef.current);
    chime.onRemoteTiles(setRemoteTiles);
    chime.onScreenShareStopped(() => setSharingScreen(false));
  }, []);

  // The self-preview `<video>` only exists in the DOM while `myMeetingId`
  // is set -- wiring this up once on mount would have grabbed `null`
  // forever, since that element hasn't been created yet at that point.
  // Re-running whenever `myMeetingId` changes catches it exactly when it
  // mounts (and clears it when it unmounts).
  useEffect(() => {
    chime.setSelfVideoElement(selfVideoRef.current);
    return () => chime.setSelfVideoElement(null);
  }, [myMeetingId]);

  const reloadMeetings = () =>
    listMeetings(token, orgId)
      .then((data) => setMeetings(data.meetings ?? []))
      .catch((err) => setError(err.message));

  useEffect(() => {
    // No org selected -- nothing renders `meetings` anyway (`MeetingsPanel`/
    // `PeoplePanel` only mount once one is), so there's nothing to clear:
    // the next org actually selected refetches fresh before either mounts.
    if (!orgId) return;
    reloadMeetings();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [token, orgId]);

  const reloadInvites = () =>
    listMyInvites(token)
      .then((data) => setAllInvites(data.invites ?? []))
      .catch((err) => setError(err.message));

  useEffect(() => {
    reloadInvites();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [token]);

  useEffect(
    () =>
      messenger.on(MEETING_INVITES_CHANNEL, (message) => {
        setAllInvites(message.invites ?? []);
      }),
    [],
  );

  // Invites for a meeting outside `orgId` aren't shown at all -- see the
  // module doc. A `null` `org_id` (a meeting created with no org) is shown
  // regardless: there's no org to have switched away from.
  const invites = allInvites.filter(
    (invite) => invite.org_id == null || invite.org_id === orgId,
  );

  // Drops a Chime session but stays "in" the meeting -- the room was
  // cleared out from under this tab, or a fresh join hasn't found anyone to
  // connect it to yet. Camera/mic preference is kept (the camera falls back
  // to a chimeless preview automatically, see `lib/chime.js`); a screen
  // share obviously can't survive with nobody left to see it.
  const disconnectChime = async () => {
    await chime.disconnect();
    setSharingScreen(false);
  };

  // Drops out of the meeting entirely -- leaving on purpose, the meeting
  // ending, or it disappearing from under this tab -- so the camera and mic
  // are released too, not just the Chime session.
  const teardownMedia = async () => {
    await chime.setCameraOn(false);
    await chime.disconnect();
    setCameraOnState(false);
    setMicMutedState(false);
    setSharingScreen(false);
  };

  // Live updates: a meeting appearing or disappearing in `orgId` lands here
  // -- see `features::meetings::broadcast_list`
  // (backend/lib/features/meetings/mod.rs). If the meeting this tab is in
  // just vanished from under it (terminated elsewhere -- everyone else
  // left, or it sat idle too long), drop out of it locally too.
  useEffect(
    () =>
      messenger.on(MEETING_LIST_CHANNEL, (message) => {
        if (message.org_id !== orgId) return;
        const next = message.meetings ?? [];
        setMeetings(next);
        if (myMeetingId && !next.some((m) => m.id === myMeetingId)) {
          teardownMedia();
          setMyMeetingId(null);
          setRoomStatus(null);
        }
      }),

    [orgId, myMeetingId],
  );

  // Room reconnects/disconnects for whichever meeting this tab is in -- see
  // `features::meetings::ROOM_CHANNEL` (backend/lib/features/meetings/mod.rs).
  useEffect(() => {
    if (!myMeetingId) return;
    return messenger.on(MEETING_ROOM_CHANNEL, (message) => {
      if (message.meeting_id !== myMeetingId) return;
      if (message.ended) {
        teardownMedia();
        setMyMeetingId(null);
        setRoomStatus(null);
      } else if (message.connected === false) {
        disconnectChime();
        setRoomStatus("waiting");
      } else if (message.connected === true) {
        chime
          .connect(message.Meeting, message.Attendee)
          .then(() => setRoomStatus("connected"))
          .catch((err) => setError(err.message));
      }
    });
  }, [myMeetingId]);

  // Chat for whichever meeting this tab is in -- see
  // `features::meetings::chat::CHANNEL` (backend/lib/features/meetings/chat.rs).
  // Cleared every time `myMeetingId` changes, not just on leave: a switch
  // straight into a different meeting (force-join) shouldn't drag the old
  // one's messages along with it.
  useEffect(() => {
    if (!myMeetingId) return;
    const unsubscribe = messenger.on(MEETING_CHAT_CHANNEL, (message) => {
      if (message.meeting_id !== myMeetingId) return;
      setChatMessages((prev) => [
        ...prev,
        { from: message.from, text: message.text, at: Date.now() },
      ]);
    });
    return () => {
      unsubscribe();
      setChatMessages([]);
    };
  }, [myMeetingId]);

  // No HTTP route behind this -- a chat message is nothing but a frame on
  // the wire, so it goes straight out over the messenger. The backend
  // relays it to every other attendee but never back to the sender (see
  // `chat::register`), so it's added here locally, same as it will appear
  // in everyone else's `chatMessages` once their own copy of this handler
  // receives it.
  const sendChatMessage = (text) => {
    const trimmed = text.trim();
    if (!trimmed || !myMeetingId) return;
    messenger.send(MEETING_CHAT_CHANNEL, {
      meeting_id: myMeetingId,
      text: trimmed,
    });
    setChatMessages((prev) => [
      ...prev,
      { from: myId, text: trimmed, at: Date.now(), self: true },
    ]);
  };

  // Logging out (this hook's one mount, at the top of `Panels`) shouldn't
  // leave a ghost attendee behind, or the camera running.
  useEffect(() => {
    return () => {
      teardownMedia();
      const meetingId = myMeetingIdRef.current;
      if (meetingId) leaveMeeting(token, meetingId).catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Shared tail of `join`/`acceptInvite`: both hit the same response shape
  // (`features::meetings::join`'s own JSON, backend/lib/features/meetings/mod.rs).
  const applyJoinResponse = async (meetingId, data) => {
    setMyMeetingId(meetingId);
    if (data.connected) {
      await chime.connect(data.Meeting, data.Attendee);
      setRoomStatus("connected");
    } else {
      setRoomStatus("waiting");
    }
  };

  const create = async (inviteOnly = false) => {
    setError(null);
    try {
      const { meeting } = await createMeeting(token, orgId, inviteOnly);
      // No room yet -- nobody else is in it. The frontend just shows
      // itself as "in" the meeting; see the module doc on
      // `features::meetings::join`.
      setMyMeetingId(meeting.id);
      setRoomStatus("waiting");
      return meeting.id;
    } catch (err) {
      setError(err.message);
      return null;
    }
  };

  const join = async (meetingId) => {
    setError(null);
    try {
      const data = await joinMeeting(token, meetingId);
      await applyJoinResponse(meetingId, data);
    } catch (err) {
      setError(err.message);
    }
  };

  const leave = async () => {
    if (!myMeetingId) return;
    const meetingId = myMeetingId;
    setMyMeetingId(null);
    setRoomStatus(null);
    await teardownMedia();
    try {
      await leaveMeeting(token, meetingId);
    } catch (err) {
      setError(err.message);
    }
  };

  // Forced straight into someone else's meeting, no invite or accept -- see
  // `features::meetings::force_join` (backend/lib/features/meetings/mod.rs).
  // Leaves whatever meeting this tab was already in first (same as
  // switching org does), then connects immediately, always muted and with
  // the camera off regardless of whatever this tab's own preference was.
  useEffect(
    () =>
      messenger.on(MEETING_FORCE_CHANNEL, async (message) => {
        if (
          myMeetingIdRef.current &&
          myMeetingIdRef.current !== message.meeting_id
        ) {
          await leave();
        }
        setMyMeetingId(message.meeting_id);
        await chime.setCameraOn(false);
        await chime.setMicMuted(true);
        try {
          await chime.connect(message.Meeting, message.Attendee);
          setCameraOnState(false);
          setMicMutedState(true);
          setRoomStatus("connected");
        } catch (err) {
          setError(err.message);
        }
      }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [],
  );

  // `orgId`'s previous value, so the effect below can tell an actual switch
  // apart from this hook's own first render (where there's nothing to have
  // left yet).
  const previousOrgIdRef = useRef(orgId);

  // Switching org leaves whatever meeting this tab is in -- a meeting
  // belongs to the org it was created under, so looking at a different
  // org's panels isn't a context a meeting you joined through the old
  // one's roster/invites still makes sense in.
  useEffect(() => {
    const switched = previousOrgIdRef.current !== orgId;
    previousOrgIdRef.current = orgId;
    if (switched && myMeetingIdRef.current) leave();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [orgId]);

  const acceptInvite = async (inviteId) => {
    setError(null);
    try {
      const invite = invites.find((i) => i.id === inviteId);
      const data = await acceptMeetingInvite(token, inviteId);
      await applyJoinResponse(data.meeting_id ?? invite?.meeting_id, data);
    } catch (err) {
      setError(err.message);
    }
  };

  const declineInvite = async (inviteId) => {
    try {
      await declineMeetingInvite(token, inviteId);
    } catch (err) {
      setError(err.message);
    }
  };

  // Invite `userId` to a meeting -- starting one under `orgId` first if
  // this tab isn't already in one, rather than making the caller juggle
  // "create, then invite" itself.
  const inviteUser = async (userId) => {
    setError(null);
    try {
      const meetingId = myMeetingId ?? (await create());
      if (!meetingId) return;
      await inviteToMeeting(token, meetingId, userId);
    } catch (err) {
      setError(err.message);
    }
  };

  // Force `userId` straight into a meeting, no invite or accept -- starting
  // one under `orgId` first if this tab isn't already in one, same as
  // `inviteUser`. Only meaningful as the meeting's creator, since the
  // backend gates `force` on that -- see `features::meetings::force_join`.
  const forceUser = async (userId) => {
    setError(null);
    try {
      const meetingId = myMeetingId ?? (await create());
      if (!meetingId) return;
      await forceJoinMeeting(token, meetingId, userId);
    } catch (err) {
      setError(err.message);
    }
  };

  const toggleCamera = async () => {
    const next = !cameraOn;
    try {
      await chime.setCameraOn(next);
      setCameraOnState(next);
    } catch (err) {
      setError(err.message);
    }
  };

  const toggleMic = async () => {
    const next = !micMuted;
    await chime.setMicMuted(next);
    setMicMutedState(next);
  };

  const toggleScreenShare = async () => {
    try {
      if (sharingScreen) {
        chime.stopScreenShare();
        setSharingScreen(false);
      } else {
        await chime.startScreenShare();
        setSharingScreen(true);
      }
    } catch (err) {
      setError(err.message);
    }
  };

  // There is no server-side timer for this -- the backend only terminates
  // an idle meeting lazily, whenever someone next lists or tries to join
  // it (see `IDLE_AFTER_SECONDS` in lib/meetings.js). So once this tab
  // itself has sat "waiting" (alone, no live room) for the same window, it
  // just leaves on its own rather than wait to be cleaned up from outside.
  //
  // Skipped while this meeting has a pending invite (`pending_invites` on
  // `meetings`, see `features::meetings::MeetingWithAttendees` --
  // backend/lib/features/meetings/mod.rs): the backend itself pauses its
  // own idle clock the same way, so leaving here too would just mean
  // inviting someone silently kicks the inviter out from under their own
  // invite, then starts a whole new meeting the next time they try.
  useEffect(() => {
    if (roomStatus !== "waiting") return;
    if (meetings.find((m) => m.id === myMeetingId)?.pending_invites) return;
    const timer = setTimeout(() => leave(), IDLE_AFTER_SECONDS * 1000);
    return () => clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [roomStatus, myMeetingId, meetings]);

  return {
    audioRef,
    selfVideoRef,
    meetings,
    myMeetingId,
    roomStatus,
    cameraOn,
    micMuted,
    sharingScreen,
    remoteTiles,
    invites,
    error,
    chatMessages,
    sendChatMessage,
    create,
    join,
    leave,
    acceptInvite,
    declineInvite,
    inviteUser,
    forceUser,
    toggleCamera,
    toggleMic,
    toggleScreenShare,
  };
}
