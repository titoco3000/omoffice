//! Shared meeting/invite state for a whole session -- one Chime session per
//! tab (see `chime.js`), so this lives in `MeetingSessionContext` rather than
//! inside `MeetingDock`, `NotificationsOverlay` or `OrgOnlineView`: accepting
//! an invite from `NotificationsOverlay` or inviting someone from `OrgOnlineView` has to land in the same
//! `myMeetingId`/`roomStatus` state `MeetingDock` renders, not a copy of it.
//!
//! `orgId` is which org's panels are currently open, not which org a
//! meeting happens to belong to -- switching it leaves whatever meeting
//! this tab is in (same as the idle-alone timeout: a real `leave` call, not
//! just a local reset), and narrows `invites` down to the ones for that
//! org. The backend itself has no notion of "current org" and hands back
//! every pending invite regardless -- filtering to `orgId` happens here,
//! client-side, on every render.
//!
//! `onIntent` lets something else follow what the user deliberately asked
//! for, as opposed to everything else that moves this tab between meetings
//! (proximity, the idle timeout, an org switch, a forced join): currently
//! `{type: "join", userId}` from `joinOrForce` and `acceptInvite`, and
//! `{type: "leave"}` from `leaveByUser` -- the office map (`features/map`) uses them to walk the
//! avatar over to whoever was joined, or away from the meeting just left,
//! whichever layout the click came from.
//!
//! Camera on/off and mic muted/unmuted are a standing preference for the
//! tab, not per-meeting state: carried from one meeting into the next, but
//! not across a reload (both start off). Only the hardware is released on
//! leave -- the preference stays whatever it last was. Except that joining
//! on this user's own initiative (`create`, `join`, `acceptInvite`, walking
//! up to someone on the map via `followJoin`) always unmutes the mic; a
//! forced join keeps whatever it was.
//!
//! Waves (`wave`/`waves`) are a nudge with no meeting behind them -- see
//! `features::meetings::waves` (backend/lib/features/meetings/waves.rs).
//! Answering one (`answerWave`) is the same as clicking the waver in the
//! roster: `joinOrForce`, so the map walks this user over to them too.

import { useCallback, useEffect, useRef, useState } from "react";
import { messenger } from "../../core/messenger";
import { decodeJwt } from "../../core/jwt";
import { useIsMobile } from "../../core/hooks/useViewport";
import * as chime from "./chime";
import {
  IDLE_AFTER_SECONDS,
  MEETING_ACCESS_REQUEST_CHANNEL,
  MEETING_CHAT_CHANNEL,
  MEETING_CONTROL_CHANNEL,
  MEETING_FORCE_CHANNEL,
  MEETING_INVITES_CHANNEL,
  MEETING_LIST_CHANNEL,
  MEETING_ROOM_CHANNEL,
  MEETING_WAVE_CHANNEL,
  acceptMeetingInvite,
  createMeeting,
  declineMeetingInvite,
  forceJoinMeeting,
  inviteToMeeting,
  joinMeeting,
  leaveMeeting,
  listMeetings,
  listMyInvites,
  lockMeeting,
  unlockMeeting,
} from "./api";

// How long `expectJoin` shows a meeting nothing has confirmed yet.
const EXPECT_JOIN_MS = 15000;
// How long after leaving a meeting a room push for it is still taken as a
// late echo of that meeting, not as the backend putting this tab back in.
const LEFT_RECENTLY_MS = 5000;
// How long a wave's bubble stays up without an answer.
const WAVE_TTL_MS = 60000;
// How long the waver's own "waved" mark shows after the wave is delivered.
const WAVED_MARK_MS = 5000;
// Desktop's shorter take on `IDLE_AFTER_SECONDS` for a meeting this tab
// sits in alone -- see the idle-leave effect at the bottom.
const DESKTOP_IDLE_AFTER_SECONDS = 3;

export function useMeetingSession(token, orgId) {
  const audioRef = useRef(null);
  // A callback ref, not `useRef` + an effect keyed on `myMeetingId`: the
  // self tile can remount into a different spot in the DOM tree -- the
  // pinned/sidebar layout switching, the mobile tiles/chat toggle -- with
  // `myMeetingId` never changing, so an effect like that would miss the
  // remount and leave the camera bound to an element no longer on screen.
  // A callback ref fires on every mount and unmount directly, so it can't
  // go stale that way.
  const selfVideoRef = useCallback((element) => {
    chime.setSelfVideoElement(element);
  }, []);

  const [meetings, setMeetings] = useState([]);
  // The meeting this tab currently considers itself "in", and whether that
  // means an actual live Chime session or just the app-level fake-connected
  // state a solo attendee sits in -- see `features::meetings::join`
  // (backend/lib/features/meetings/mod.rs).
  const [myMeetingId, setMyMeetingId] = useState(null);
  // "connecting" (Chime handshake in flight, ours or a forced-in one) |
  // "waiting" (in the meeting, no live room yet) | "connected" | null
  const [roomStatus, setRoomStatus] = useState(null);
  // True from the moment `create` is clicked until the backend hands back a
  // real meeting id -- lets the "New meeting" button and the dock react
  // before there's anything to key state off yet.
  const [creating, setCreating] = useState(false);
  const [cameraOn, setCameraOnState] = useState(false);
  const [micMuted, setMicMutedState] = useState(true);
  const [sharingScreen, setSharingScreen] = useState(false);
  const [remoteTiles, setRemoteTiles] = useState([]); // [{tileId, attendeeId, externalUserId, isContent}]
  const [speakingUserIds, setSpeakingUserIds] = useState(new Set());
  const [mutedUserIds, setMutedUserIds] = useState(new Set());
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
  // Pending "let me in" requests for whichever locked meeting this tab is
  // currently an attendee of -- see `features::meetings::access_requests`
  // (backend/lib/features/meetings/access_requests.rs). Frontend-only, same
  // as `chatMessages`: the backend relays a request but never stores it, so
  // a repeat request from the same person just refreshes its timestamp
  // rather than piling up, and the list resets whenever `myMeetingId` does.
  const [accessRequests, setAccessRequests] = useState([]);
  // Waves at this user, `[{userId, orgId, at}]` -- frontend-only, same as
  // `accessRequests`: the backend relays a wave but never stores it, so a
  // repeat wave from the same person just refreshes its timestamp. Every
  // org's, narrowed to `orgId` before this hook hands them back, same as
  // `invites`.
  const [allWaves, setAllWaves] = useState([]);
  // This user's own waves the other side just received -- same shape, same
  // org narrowing.
  const [allSentWaves, setAllSentWaves] = useState([]);
  const myId = decodeJwt(token)?.sub;
  const isMobile = useIsMobile();

  // Camera/mic changes all go through these, whoever asked for them (this
  // user or a remote mute) -- see the module doc.
  const applyCamera = async (on) => {
    await chime.setCameraOn(on);
    setCameraOnState(on);
  };

  const applyMicMuted = async (muted) => {
    await chime.setMicMuted(muted);
    setMicMutedState(muted);
  };

  // Mirrors `myMeetingId` for the unmount cleanup below, which is set up
  // once and would otherwise only ever see whatever it was at that moment,
  // not whatever it is by the time the tab actually leaves.
  const myMeetingIdRef = useRef(null);
  useEffect(() => {
    myMeetingIdRef.current = myMeetingId;
  }, [myMeetingId]);

  // A meeting this tab is showing itself in before the backend has actually
  // put it there -- see `expectJoin`. `{meetingId, previous: {id, status}}`
  // while one is outstanding, `null` otherwise.
  const expectedJoinRef = useRef(null);
  // The meeting a `MEETING_ROOM_CHANNEL` reconnect last arrived for -- one
  // can land while an expected join is still outstanding (someone else
  // joining claims the room and reconnects everyone already in it), and
  // then the join's own answer has nothing left to connect.
  const roomPushedForRef = useRef(null);
  // `{meetingId, at}` of this tab's last `leave` -- see the room push
  // handler below.
  const leftRef = useRef(null);

  useEffect(() => {
    // Match the initial state above -- `chime.js` outlives this hook across
    // a logout/login, so it may still hold the last session's values.
    chime.setMicMuted(true);
    chime.onRemoteTiles(setRemoteTiles);
    chime.onScreenShareStopped(() => setSharingScreen(false));
    chime.onSpeakingChange(setSpeakingUserIds);
    chime.onMutedChange(setMutedUserIds);
  }, []);

  useEffect(() => {
    chime.setAudioElement(audioRef.current);
    return () => chime.setAudioElement(null);
  }, [myMeetingId]);

  const reloadMeetings = () =>
    listMeetings(token, orgId)
      .then((data) => setMeetings(data.meetings ?? []))
      .catch((err) => setError(err.message));

  useEffect(() => {
    // No org selected -- nothing renders `meetings` anyway (`MeetingDock`/
    // `OrgOnlineView` only mount once one is), so there's nothing to clear:
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
  // to a chimeless preview automatically, see `chime.js`); a screen
  // share obviously can't survive with nobody left to see it.
  const disconnectChime = async () => {
    await chime.disconnect();
    setSharingScreen(false);
  };

  // Drops out of the meeting entirely -- leaving on purpose, the meeting
  // ending, or it disappearing from under this tab -- so the mic is
  // released too, not just the Chime session. The camera follows on its
  // own once nothing is showing it anymore (see `chime.js`); on desktop the
  // sidebar keeps previewing it while `cameraOn` stays set.
  const teardownMedia = async () => {
    await chime.releaseMic();
    await chime.disconnect();
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
        // An expected meeting may not exist yet at all -- `expectJoin`'s own
        // timeout is what gives up on it, not its absence from a list.
        if (expectedJoinRef.current?.meetingId === myMeetingId) return;
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
  // A room handed to this user for a meeting this tab isn't showing means
  // the backend has them in it anyway (this tab lost track, e.g. an
  // expected join that gave up too early): follow it rather than leave a
  // connected user with no meeting on screen. Unless this tab only just
  // left that meeting -- then it's a push that crossed the leave in flight.
  useEffect(() => {
    return messenger.on(MEETING_ROOM_CHANNEL, (message) => {
      if (message.meeting_id !== myMeetingId) {
        const left = leftRef.current;
        const leftRecently =
          left?.meetingId === message.meeting_id &&
          Date.now() - left.at < LEFT_RECENTLY_MS;
        if (message.connected !== true || leftRecently) return;
        expectedJoinRef.current = null;
        myMeetingIdRef.current = message.meeting_id;
        setMyMeetingId(message.meeting_id);
      }
      if (message.ended) {
        teardownMedia();
        setMyMeetingId(null);
        setRoomStatus(null);
      } else if (message.connected === false) {
        disconnectChime();
        setRoomStatus("waiting");
      } else if (message.connected === true) {
        roomPushedForRef.current = message.meeting_id;
        // Actually in it now, whatever the request that expected it still
        // has to say -- nothing left to revert to if that runs long.
        if (expectedJoinRef.current?.meetingId === message.meeting_id)
          expectedJoinRef.current = null;
        setRoomStatus("connecting");
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

  // Access requests for whichever meeting this tab is in -- see
  // `features::meetings::access_requests::CHANNEL`
  // (backend/lib/features/meetings/access_requests.rs). Reset the same way
  // `chatMessages` is: a switch to a different meeting shouldn't drag along
  // requests addressed to the one just left.
  useEffect(() => {
    if (!myMeetingId) return;
    const unsubscribe = messenger.on(
      MEETING_ACCESS_REQUEST_CHANNEL,
      (message) => {
        if (message.meeting_id !== myMeetingId) return;
        setAccessRequests((prev) => [
          ...prev.filter((r) => r.userId !== message.from),
          { userId: message.from, at: Date.now() },
        ]);
      },
    );
    return () => {
      unsubscribe();
      setAccessRequests([]);
    };
  }, [myMeetingId]);

  // Both sides of a wave: `wave` lands in `allWaves` (at this user), `sent`
  // in `allSentWaves` (from this user, once the backend confirms the other
  // side got it). Each entry expires on its own -- the bubble after
  // `WAVE_TTL_MS`, the waver's mark after `WAVED_MARK_MS` -- not on
  // anything the other side does.
  useEffect(() => {
    const add = (setList, userId, orgId, ttl) => {
      const entry = { userId, orgId, at: Date.now() };
      setList((prev) => [...prev.filter((w) => w.userId !== userId), entry]);
      setTimeout(() => setList((prev) => prev.filter((w) => w !== entry)), ttl);
    };
    return messenger.on(MEETING_WAVE_CHANNEL, (message) => {
      if (message.action === "wave")
        add(setAllWaves, message.from, message.org_id, WAVE_TTL_MS);
      else if (message.action === "sent")
        add(setAllSentWaves, message.to, message.org_id, WAVED_MARK_MS);
    });
  }, []);

  const waves = allWaves.filter((w) => w.orgId === orgId);
  const wavedUserIds = new Set(
    allSentWaves.filter((w) => w.orgId === orgId).map((w) => w.userId),
  );

  // Fire-and-forget over the messenger, same as `requestAccess` -- the
  // `sent` ack above is what marks it in `wavedUserIds`.
  const wave = (userId) => {
    if (!orgId) return;
    messenger.send(MEETING_WAVE_CHANNEL, {
      action: "wave",
      org_id: orgId,
      to: userId,
    });
  };

  const dismissWave = (userId) => {
    setAllWaves((prev) => prev.filter((w) => w.userId !== userId));
  };

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

  // Remote mute/camera-off -- see `features::meetings::controls`
  // (backend/lib/features/meetings/controls.rs). Any other attendee may
  // push this at this tab; there is no accept step and no way to force the
  // mic or camera back *on*, only off. Once applied here, every other
  // attendee already sees it through Chime's own live audio/video state --
  // nothing to push back out.
  useEffect(() => {
    if (!myMeetingId) return;
    return messenger.on(MEETING_CONTROL_CHANNEL, async (message) => {
      if (message.meeting_id !== myMeetingId) return;
      if (message.action === "mute") await applyMicMuted(true);
      else if (message.action === "camera_off") await applyCamera(false);
    });
  }, [myMeetingId]);

  // Force `userId` (another attendee of this same meeting) muted or
  // camera-off. Fire-and-forget over the messenger, same as
  // `sendChatMessage` -- the backend checks both this tab and `userId` are
  // actually attendees before relaying it on.
  const muteAttendee = (userId) => {
    if (!myMeetingId) return;
    messenger.send(MEETING_CONTROL_CHANNEL, {
      meeting_id: myMeetingId,
      target_user_id: userId,
      action: "mute",
    });
  };

  const turnOffAttendeeCamera = (userId) => {
    if (!myMeetingId) return;
    messenger.send(MEETING_CONTROL_CHANNEL, {
      meeting_id: myMeetingId,
      target_user_id: userId,
      action: "camera_off",
    });
  };

  // Logging out (this hook's one mount, at the top of `Panels`) shouldn't
  // leave a ghost attendee behind, or the camera running.
  useEffect(() => {
    return () => {
      teardownMedia();
      chime.setCameraOn(false);
      const meetingId = myMeetingIdRef.current;
      if (meetingId) leaveMeeting(token, meetingId).catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Shared tail of `join`/`acceptInvite`: both hit the same response shape
  // (`features::meetings::join`'s own JSON, backend/lib/features/meetings/mod.rs).
  // `myMeetingId` is already set by the caller before this runs (optimistic
  // open) -- this only carries `roomStatus` the rest of the way.
  const applyJoinResponse = async (meetingId, data) => {
    expectedJoinRef.current = null;
    setMyMeetingId(meetingId);
    if (data.connected) {
      setRoomStatus("connecting");
      await chime.connect(data.Meeting, data.Attendee);
      setRoomStatus("connected");
    } else {
      setRoomStatus("waiting");
    }
  };

  const create = async (inviteOnly = false) => {
    setError(null);
    setCreating(true);
    await applyMicMuted(false);
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
    } finally {
      setCreating(false);
    }
  };

  // Opens the dock immediately (the meeting id is already known -- it's
  // just this tab that hasn't joined yet), tagged "connecting" until the
  // join actually lands, rather than waiting on the round trip to show
  // anything.
  const join = async (meetingId) => {
    setError(null);
    setMyMeetingId(meetingId);
    setRoomStatus("connecting");
    await applyMicMuted(false);
    try {
      const data = await joinMeeting(token, meetingId);
      await applyJoinResponse(meetingId, data);
    } catch (err) {
      setError(err.message);
      setMyMeetingId(null);
      setRoomStatus(null);
    }
  };

  const leave = async () => {
    if (!myMeetingId) return;
    const meetingId = myMeetingId;
    leftRef.current = { meetingId, at: Date.now() };
    expectedJoinRef.current = null;
    // Ahead of the effect that would sync it -- see `expectJoin`, which
    // can run right after this and needs to see this tab as out already.
    myMeetingIdRef.current = null;
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
  // `features::meetings::force_join` (backend/lib/features/meetings/mod.rs),
  // or pulled in by someone walking up on the map
  // (`features::map::proximity`). The backend has already taken this
  // user out of whatever meeting they were in before, so there's nothing to
  // `leave` here -- `chime.connect` drops the old session itself. Connects
  // immediately, with this tab's own camera/mic preference, same as any
  // other join (see the module doc).
  useEffect(
    () =>
      messenger.on(MEETING_FORCE_CHANNEL, async (message) => {
        expectedJoinRef.current = null;
        setMyMeetingId(message.meeting_id);
        setRoomStatus("connecting");
        try {
          await chime.connect(message.Meeting, message.Attendee);
          setRoomStatus("connected");
        } catch (err) {
          setError(err.message);
        }
      }),
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

  // Same optimistic open as `join` -- the meeting id is already on the
  // invite, so there's no need to wait on the accept round trip to show
  // the dock.
  const acceptInvite = async (inviteId) => {
    setError(null);
    const invite = invites.find((i) => i.id === inviteId);
    const meetingId = invite?.meeting_id;
    if (meetingId) {
      setMyMeetingId(meetingId);
      setRoomStatus("connecting");
    }
    await applyMicMuted(false);
    try {
      const data = await acceptMeetingInvite(token, inviteId);
      // Accepting is this user's own choice to go there -- walk over to the
      // meeting (see `onIntent`): next to the inviter if they're still in
      // it, else whoever is.
      const attendees =
        meetings.find((m) => m.id === (data.meeting_id ?? meetingId))
          ?.attendees ?? [];
      const target = attendees.includes(invite?.invited_by)
        ? invite.invited_by
        : attendees.find((id) => id !== myId);
      if (target) emitIntent({ type: "join", userId: target });
      await applyJoinResponse(data.meeting_id ?? meetingId, data);
    } catch (err) {
      setError(err.message);
      if (meetingId) {
        setMyMeetingId(null);
        setRoomStatus(null);
      }
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

  const intentListenersRef = useRef(new Set());
  const onIntent = useCallback((listener) => {
    intentListenersRef.current.add(listener);
    return () => intentListenersRef.current.delete(listener);
  }, []);
  const emitIntent = (intent) =>
    intentListenersRef.current.forEach((listener) => listener(intent));

  // `leave`, as the user's own explicit choice -- see `onIntent`.
  const leaveByUser = async () => {
    if (!myMeetingId) return;
    emitIntent({ type: "leave" });
    await leave();
  };

  const joinOrForce = async (userId, meetingId) => {
    if (myMeetingId) {
      if (!window.confirm("Leave your current meeting?")) return;
      await leave();
    }
    emitIntent({ type: "join", userId });
    if (meetingId) {
      await join(meetingId);
      return;
    }
    setError(null);
    try {
      const newMeetingId = await create();
      if (!newMeetingId) return;
      await forceJoinMeeting(token, newMeetingId, userId);
    } catch (err) {
      setError(err.message);
    }
  };

  // Show this tab in `meetingId` right away, ahead of whatever is actually
  // going to put it there -- walking up to someone on the map
  // (`features::map::proximity`) takes a few seconds of room/Chime setup
  // before anyone is really in anything. Settled by `followJoin` (this
  // tab's own request answering) or a `MEETING_FORCE_CHANNEL` push (someone
  // else's request moving this tab); if neither shows up within
  // `EXPECT_JOIN_MS`, this tab goes back to wherever it was.
  const expectJoin = (meetingId) => {
    if (myMeetingIdRef.current === meetingId) return;
    const expected = {
      meetingId,
      previous: { id: myMeetingIdRef.current, status: roomStatus },
    };
    expectedJoinRef.current = expected;
    roomPushedForRef.current = null;
    myMeetingIdRef.current = meetingId;
    setMyMeetingId(meetingId);
    setRoomStatus("connecting");
    setTimeout(() => {
      if (expectedJoinRef.current === expected) revertExpectedJoin();
    }, EXPECT_JOIN_MS);
  };

  const revertExpectedJoin = () => {
    const expected = expectedJoinRef.current;
    if (!expected) return;
    expectedJoinRef.current = null;
    setMyMeetingId(expected.previous.id);
    setRoomStatus(expected.previous.status);
  };

  // Follow a join the backend already made on this tab's behalf, outside
  // any of the calls above -- walking up to someone on the map answers with
  // `join`'s response shape whenever it actually moved this tab, or
  // `{meeting_id, unchanged: true}` otherwise (`null` for an error, which
  // counts the same). Everyone else it moved hears about it over
  // `MEETING_FORCE_CHANNEL`.
  const followJoin = async (data) => {
    if (!data || data.unchanged) {
      // Expected to move, didn't: the guess in `expectJoin` was wrong.
      revertExpectedJoin();
      return;
    }
    // Walking up to someone is this user's own doing -- see the module doc.
    await applyMicMuted(false);
    if (roomPushedForRef.current === data.meeting_id) {
      // Already connected (or connecting) off a room push -- see
      // `roomPushedForRef`. Connecting again would just drop and redo it.
      expectedJoinRef.current = null;
      setMyMeetingId(data.meeting_id);
      return;
    }
    await applyJoinResponse(data.meeting_id, data);
  };

  // Go to whoever waved: into their meeting if they're in one (asking
  // first if it's locked, same as the roster would), else a new one with
  // them forced in -- see `joinOrForce`.
  const answerWave = async (userId) => {
    dismissWave(userId);
    const meeting = meetings.find((m) => m.attendees?.includes(userId));
    if (meeting && meeting.id === myMeetingId) return;
    if (meeting?.invite_only) {
      requestAccess(meeting.id);
      return;
    }
    await joinOrForce(userId, meeting?.id ?? null);
  };

  // Lock/unlock the meeting this tab is in
  const toggleLock = async () => {
    if (!myMeetingId) return;
    const locked = meetings.find((m) => m.id === myMeetingId)?.invite_only;
    setError(null);
    try {
      await (locked ? unlockMeeting : lockMeeting)(token, myMeetingId);
    } catch (err) {
      setError(err.message);
    }
  };

  // Ask whoever's already in a locked meeting to let this tab's user in --
  // fire-and-forget over the messenger, same as `sendChatMessage`: the
  // backend relays it to that meeting's current attendees alone (see
  // `features::meetings::access_requests`), and there is no accept step on
  // the wire, just whatever an attendee does about it (`admitAccessRequest`
  // below, or nothing).
  const requestAccess = (meetingId) => {
    messenger.send(MEETING_ACCESS_REQUEST_CHANNEL, { meeting_id: meetingId });
  };

  const dismissAccessRequest = (userId) => {
    setAccessRequests((prev) => prev.filter((r) => r.userId !== userId));
  };

  // An attendee's answer to a pending request: invite the requester into
  // this tab's own meeting (reusing `inviteUser`, since this tab is already
  // in it) and drop the request locally -- the requester's own invite list
  // updates from the invite itself, not from anything pushed back here.
  const admitAccessRequest = async (userId) => {
    await inviteUser(userId);
    dismissAccessRequest(userId);
  };

  const toggleCamera = async () => {
    try {
      await applyCamera(!cameraOn);
    } catch (err) {
      setError(err.message);
    }
  };

  const toggleMic = () => applyMicMuted(!micMuted);

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
  // it (see `IDLE_AFTER_SECONDS` in `api.js`). So once this tab
  // itself has sat "waiting" (alone, no live room) for the same window, it
  // just leaves on its own rather than wait to be cleaned up from outside.
  //
  // Skipped while this meeting has a pending invite (`pending_invites` on
  // `meetings`, see `features::meetings::MeetingWithAttendees` --
  // backend/lib/features/meetings/mod.rs): the backend itself pauses its
  // own idle clock the same way, so leaving here too would just mean
  // inviting someone silently kicks the inviter out from under their own
  // invite, then starts a whole new meeting the next time they try.
  //
  // Desktop leaves sooner (`DESKTOP_IDLE_AFTER_SECONDS`): a lone meeting
  // there is mostly a leftover -- someone walked off on the map or everyone
  // else hung up -- not someone waiting on a phone for others to arrive.
  useEffect(() => {
    if (roomStatus !== "waiting") return;
    if (meetings.find((m) => m.id === myMeetingId)?.pending_invites) return;
    const seconds = isMobile ? IDLE_AFTER_SECONDS : DESKTOP_IDLE_AFTER_SECONDS;
    const timer = setTimeout(() => leave(), seconds * 1000);
    return () => clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [roomStatus, myMeetingId, meetings, isMobile]);

  return {
    audioRef,
    selfVideoRef,
    meetings,
    myMeetingId,
    roomStatus,
    creating,
    cameraOn,
    micMuted,
    sharingScreen,
    remoteTiles,
    speakingUserIds,
    mutedUserIds,
    invites,
    error,
    chatMessages,
    accessRequests,
    waves,
    wavedUserIds,
    wave,
    answerWave,
    dismissWave,
    sendChatMessage,
    requestAccess,
    admitAccessRequest,
    dismissAccessRequest,
    muteAttendee,
    turnOffAttendeeCamera,
    create,
    join,
    leave,
    leaveByUser,
    onIntent,
    acceptInvite,
    declineInvite,
    inviteUser,
    joinOrForce,
    expectJoin,
    followJoin,
    toggleLock,
    toggleCamera,
    toggleMic,
    toggleScreenShare,
  };
}
