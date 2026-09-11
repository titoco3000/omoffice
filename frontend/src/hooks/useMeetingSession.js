import { useCallback, useEffect, useRef, useState } from "react";
import {
  ConsoleLogger,
  DefaultDeviceController,
  DefaultMeetingSession,
  DefaultModality,
  LogLevel,
  MeetingSessionConfiguration,
} from "amazon-chime-sdk-js";
import { callApi, describeFailure } from "../api";
import { useChannel } from "../messenger/MessengerContext";

/** A Chime data message broadcast; see CLAUDE.md for why it's self-enforced. */
const MODERATION_TOPIC = "moderation";

/// A lone viewer waits this long after everyone else leaves before Chime is
/// actually dropped -- long enough that a quick reconnect doesn't cost a
/// fresh session, short enough to free the room promptly either way.
const SOLO_GRACE_MS = 20_000;

/// How long Chime keeps the message for an attendee not connected right now.
const MODERATION_LIFETIME_MS = 5000;

function shortLabel(externalUserId) {
  return String(externalUserId ?? "").split("#")[0] || "attendee";
}

/**
 * Owns the Chime SDK session for `code`: roster, media controls,
 * moderation. Renders nothing — callers read `roster` and call `bindVideo`.
 *
 * Chime itself is joined lazily, not on mount: a lone viewer costs no
 * Chime room (`status` sits at "waiting"), connecting only once
 * `meeting_presence` reports someone else on the page too, and dropping
 * back to "waiting" [`SOLO_GRACE_MS`] after everyone else leaves again —
 * see `meetings::pool` for the backend half.
 */
export default function useMeetingSession({ code, token, onLeave }) {
  const [status, setStatus] = useState("waiting"); // waiting | joining | live | error | ended
  const [error, setError] = useState("");
  const [notice, setNotice] = useState(""); // "X muted you", and the like
  const [roster, setRoster] = useState([]);
  const [muted, setMuted] = useState(false);
  const [cameraOn, setCameraOn] = useState(false);
  const [sharing, setSharing] = useState(false);
  const [focused, setFocused] = useState(null); // attendee id, or null for the grid
  const [viewerCount, setViewerCount] = useState(null); // null until the first broadcast

  const sessionRef = useRef(null);
  const audioRef = useRef(null);
  const tileOwners = useRef(new Map()); // tileId -> attendee id
  const boundElements = useRef(new Map()); // tileId -> the element bound to it
  const volumeSubscriptions = useRef(new Set()); // attendee ids already watched
  const meRef = useRef(null); // this browser's attendee id
  const noticeTimer = useRef(null);
  const cancelledRef = useRef(false); // true once this hook has unmounted
  const connectingRef = useRef(false); // guards against a second connect() mid-flight
  const graceTimer = useRef(null); // the SOLO_GRACE_MS countdown, while it's running
  // Sessions `disposeSession` has already torn down on purpose -- its own
  // `audioVideo.stop()` fires the *same* `audioVideoDidStop` observer a
  // real Chime-side drop would, sometimes well after `disposeSession`
  // itself returns, so the observer needs to tell "we did this" from "this
  // just happened to us" apart, or it stomps whatever status we already
  // moved on to (see `disposeSession` and `buildSession`).
  const disposedSessions = useRef(new WeakSet());

  useChannel("meeting_presence", (message) => {
    if (message.meeting === code) setViewerCount(message.viewer_count);
  });

  /// Add or patch one attendee. Upserts since presence and a video tile can
  /// arrive in either order; `insert: false` avoids resurrecting a leaver.
  const upsertAttendee = useCallback((attendeeId, patch, insert = true) => {
    setRoster((current) => {
      const index = current.findIndex(
        (entry) => entry.attendeeId === attendeeId,
      );
      if (index === -1) {
        if (!insert) return current;
        const modality = new DefaultModality(attendeeId);
        return [
          ...current,
          {
            attendeeId,
            externalUserId: attendeeId,
            local: modality.base() === meRef.current,
            content: modality.hasModality(DefaultModality.MODALITY_CONTENT),
            tileId: null,
            muted: false,
            active: false,
            ...patch,
          },
        ];
      }
      const merged = { ...current[index], ...patch };
      const unchanged = Object.keys(patch).every(
        (key) => current[index][key] === merged[key],
      );
      if (unchanged) return current;
      const next = [...current];
      next[index] = merged;
      return next;
    });
  }, []);

  /// Bind a tile to the <video> showing it, or forget one that unmounted.
  /// The map stops a plain re-render from rebinding an already-bound element.
  const bindVideo = useCallback((tileId, element) => {
    if (!element) {
      boundElements.current.delete(tileId);
      return;
    }
    const audioVideo = sessionRef.current?.audioVideo;
    if (!audioVideo || boundElements.current.get(tileId) === element) return;
    audioVideo.bindVideoElement(tileId, element);
    boundElements.current.set(tileId, element);
  }, []);

  /// Tear a session down. Safe on a session that never started.
  const disposeSession = useCallback(async (session) => {
    if (!session) return;
    disposedSessions.current.add(session);
    const audioVideo = session.audioVideo;
    try {
      await audioVideo.stopContentShare();
      audioVideo.stop();
      await audioVideo.stopAudioInput();
      await audioVideo.stopVideoInput();
    } catch {
      // Already torn down, or the devices went away with the tab.
    }
  }, []);

  const leave = useCallback(async () => {
    clearTimeout(graceTimer.current);
    const session = sessionRef.current;
    sessionRef.current = null;
    await disposeSession(session);
    onLeave?.();
  }, [disposeSession, onLeave]);

  /// Drop Chime back to "waiting", without leaving the meeting page.
  const disconnectToWaiting = useCallback(async () => {
    const session = sessionRef.current;
    sessionRef.current = null;
    await disposeSession(session);
    if (cancelledRef.current) return;
    setStatus("waiting");
  }, [disposeSession]);

  /// Show a line over the video for a few seconds.
  const showNotice = useCallback((text) => {
    setNotice(text);
    clearTimeout(noticeTimer.current);
    noticeTimer.current = setTimeout(() => setNotice(""), 5000);
  }, []);

  useEffect(() => () => clearTimeout(noticeTimer.current), []);

  /// Stop the local camera. Both the button and a moderator's request end here.
  const stopCamera = useCallback(async (audioVideo) => {
    audioVideo.stopLocalVideoTile();
    await audioVideo.stopVideoInput();
    setCameraOn(false);
  }, []);

  /// Ask another attendee's client to mute itself, or to drop its camera.
  function moderate(cell, action) {
    const audioVideo = sessionRef.current?.audioVideo;
    if (!audioVideo) return;
    audioVideo.realtimeSendDataMessage(
      MODERATION_TOPIC,
      { action, target: cell.key },
      MODERATION_LIFETIME_MS,
    );
    const what = action === "mute" ? "muted" : "stopped the camera of";
    showNotice(`you ${what} ${cell.label}`);
  }

  /// Build the session and wire the observers. Nothing fires until `start()`.
  function buildSession({ Meeting: meeting, Attendee: attendee }) {
    const logger = new ConsoleLogger("chime", LogLevel.WARN);
    const deviceController = new DefaultDeviceController(logger);
    const session = new DefaultMeetingSession(
      new MeetingSessionConfiguration(meeting, attendee),
      logger,
      deviceController,
    );
    const me = attendee.AttendeeId;
    meRef.current = me;
    const audioVideo = session.audioVideo;

    audioVideo.realtimeSubscribeToAttendeeIdPresence(
      (attendeeId, present, externalUserId) => {
        if (!present) {
          volumeSubscriptions.current.delete(attendeeId);
          audioVideo.realtimeUnsubscribeFromVolumeIndicator(attendeeId);
          setRoster((current) =>
            current.filter((entry) => entry.attendeeId !== attendeeId),
          );
          return;
        }

        const modality = new DefaultModality(attendeeId);
        const content = modality.hasModality(DefaultModality.MODALITY_CONTENT);
        upsertAttendee(attendeeId, {
          externalUserId: externalUserId ?? attendeeId,
          local: modality.base() === me,
          content,
        });

        // A content attendee has no microphone to report on, and presence can
        // fire more than once for the same attendee.
        if (content || volumeSubscriptions.current.has(attendeeId)) return;
        volumeSubscriptions.current.add(attendeeId);
        audioVideo.realtimeSubscribeToVolumeIndicator(
          attendeeId,
          (id, volume, isMuted) => {
            upsertAttendee(id, {
              muted: isMuted === true,
              active: (volume ?? 0) > 0.2 && isMuted !== true,
            });
          },
        );
      },
    );

    audioVideo.addObserver({
      videoTileDidUpdate(tile) {
        if (!tile.boundAttendeeId) return;
        const first = !tileOwners.current.has(tile.tileId);
        tileOwners.current.set(tile.tileId, tile.boundAttendeeId);
        upsertAttendee(tile.boundAttendeeId, { tileId: tile.tileId });
        // A screen share is what everyone came to look at.
        if (first && tile.isContent) setFocused(tile.boundAttendeeId);
      },
      videoTileWasRemoved(tileId) {
        const owner = tileOwners.current.get(tileId);
        tileOwners.current.delete(tileId);
        boundElements.current.delete(tileId);
        if (owner) upsertAttendee(owner, { tileId: null }, false);
      },
      audioVideoDidStop() {
        // `disposeSession` already tore this one down on purpose (leaving,
        // going solo, reconnecting) -- its caller owns whatever status
        // follows, so this fires for a genuine, unexpected drop only.
        if (disposedSessions.current.has(session)) return;
        sessionRef.current = null;
        void disposeSession(session);
        setStatus("ended");
      },
    });

    // Moderation aimed at this browser. Everything else on the topic is
    // somebody else's business, including the copy of our own broadcast.
    audioVideo.realtimeSubscribeToReceiveDataMessage(
      MODERATION_TOPIC,
      (message) => {
        if (message.throttled) return;
        let payload;
        try {
          payload = message.json();
        } catch {
          return; // Not ours, or not JSON.
        }
        if (payload?.target !== me) return;

        const who = shortLabel(message.senderExternalUserId);
        if (payload.action === "mute") {
          audioVideo.realtimeMuteLocalAudio();
          setMuted(true);
          showNotice(`${who} muted you`);
        } else if (payload.action === "camera-off") {
          void stopCamera(audioVideo);
          showNotice(`${who} turned your camera off`);
        }
      },
    );

    audioVideo.addContentShareObserver({
      contentShareDidStart: () => setSharing(true),
      contentShareDidStop: () => setSharing(false),
    });

    return session;
  }

  /// Start audio and the connection. Video stays off until asked for.
  async function startMedia(session) {
    const audioVideo = session.audioVideo;
    const mics = await audioVideo.listAudioInputDevices();
    if (mics.length > 0) await audioVideo.startAudioInput(mics[0].deviceId);
    await audioVideo.bindAudioElement(audioRef.current);
    audioVideo.start();
  }

  /// Actually join Chime: the old mount-time join, now called only once
  /// `viewerCount` says somebody else is here. Every await checks
  /// `cancelledRef`: StrictMode mounts twice, and a stray session would be
  /// a second attendee nobody can see.
  async function connect() {
    if (connectingRef.current || sessionRef.current) return;
    connectingRef.current = true;
    // A fresh session gets a fresh Chime attendee id even for the same
    // person reconnecting -- leftover roster/tile state from whichever
    // session came before would otherwise just sit alongside the new
    // entries as ghost tiles rather than being replaced by them.
    volumeSubscriptions.current.clear();
    tileOwners.current.clear();
    boundElements.current.clear();
    setRoster([]);
    setFocused(null);
    setStatus("joining");
    setError("");

    const response = await callApi({
      method: "POST",
      path: `/private/meetings/${encodeURIComponent(code)}/attendees`,
      token,
    });
    if (cancelledRef.current) {
      connectingRef.current = false;
      return;
    }
    if (!response.ok) {
      connectingRef.current = false;
      setStatus("error");
      setError(describeFailure(response));
      return;
    }

    let session;
    try {
      session = buildSession(response.data);
      if (cancelledRef.current) {
        await disposeSession(session);
        return;
      }
      sessionRef.current = session;
      connectingRef.current = false;
      await startMedia(session);
      if (cancelledRef.current) return;
      setStatus("live");
    } catch (failure) {
      // Almost always a denied microphone permission or an insecure origin.
      connectingRef.current = false;
      sessionRef.current = null;
      await disposeSession(session);
      if (cancelledRef.current) return;
      setStatus("error");
      setError(String(failure));
    }
  }

  // Leaving the meeting page altogether (not just going solo) always tears
  // the session down, unconditionally -- `disconnectToWaiting` is for
  // staying on the page with Chime dropped.
  useEffect(() => {
    cancelledRef.current = false;
    connectingRef.current = false;
    return () => {
      cancelledRef.current = true;
      clearTimeout(graceTimer.current);
      const session = sessionRef.current;
      sessionRef.current = null;
      void disposeSession(session);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [code, token]);

  // The occupancy policy: connect once someone else shows up, and once
  // alone again, wait out SOLO_GRACE_MS before dropping back to "waiting"
  // -- a quick reconnect (someone's tab briefly reloading) shouldn't cost
  // a fresh Chime session. `viewerCount` starts `null` until the first
  // `meeting_presence` broadcast, which arrives moments after mount as an
  // echo of this tab's own presence report.
  useEffect(() => {
    clearTimeout(graceTimer.current);
    graceTimer.current = null;
    if (viewerCount === null) return undefined;

    if (viewerCount >= 2) {
      if (!sessionRef.current && !connectingRef.current) void connect();
      return undefined;
    }

    if (sessionRef.current) {
      graceTimer.current = setTimeout(() => {
        graceTimer.current = null;
        void disconnectToWaiting();
      }, SOLO_GRACE_MS);
    }
    return () => clearTimeout(graceTimer.current);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [viewerCount]);

  function toggleMute() {
    const audioVideo = sessionRef.current?.audioVideo;
    if (!audioVideo) return;
    if (muted) audioVideo.realtimeUnmuteLocalAudio();
    else audioVideo.realtimeMuteLocalAudio();
    setMuted(!muted);
  }

  async function toggleCamera() {
    const audioVideo = sessionRef.current?.audioVideo;
    if (!audioVideo) return;

    if (cameraOn) {
      await stopCamera(audioVideo);
      return;
    }

    try {
      const cameras = await audioVideo.listVideoInputDevices();
      if (cameras.length === 0) throw new Error("no camera found");
      await audioVideo.startVideoInput(cameras[0].deviceId);
      audioVideo.startLocalVideoTile();
      setCameraOn(true);
    } catch (failure) {
      setError(String(failure));
    }
  }

  async function toggleShare() {
    const audioVideo = sessionRef.current?.audioVideo;
    if (!audioVideo) return;
    try {
      if (sharing) await audioVideo.stopContentShare();
      else await audioVideo.startContentShareFromScreenCapture();
    } catch (failure) {
      // Dismissing the browser's picker lands here too.
      setError(String(failure));
    }
  }

  return {
    status,
    error,
    notice,
    roster,
    muted,
    cameraOn,
    sharing,
    focused,
    setFocused,
    audioRef,
    bindVideo,
    moderate,
    leave,
    toggleMute,
    toggleCamera,
    toggleShare,
  };
}
