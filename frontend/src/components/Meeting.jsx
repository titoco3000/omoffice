import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ConsoleLogger,
  DefaultDeviceController,
  DefaultMeetingSession,
  DefaultModality,
  LogLevel,
  MeetingSessionConfiguration,
} from "amazon-chime-sdk-js";
import { callApi, describeFailure } from "../api";
import ControlPanel from "./ControlPanel";
import ParticipantTile from "./ParticipantTile";

/// Moderation travels as a Chime data message, on this topic.
///
/// The message is a broadcast — Chime has no per-attendee delivery — so it
/// carries its target and every client but the target ignores it. The target
/// then mutes itself *at the source*, which is what makes A muting B silent
/// for C too: nothing is being enforced per-listener. There is no "unmute"
/// action for the same reason there is no unmute button on someone else's
/// tile: only B can unmute B.
///
/// A hostile client could simply not listen. Real enforcement would need the
/// backend to hold the roster and use Chime's server-side mute; this is a
/// sample, and every client here is ours.
const MODERATION_TOPIC = "moderation";

/// How long Chime keeps the message for an attendee who is not connected right
/// now. Short: a mute aimed at this moment should not land a minute later.
const MODERATION_LIFETIME_MS = 5000;

/// While one participant is expanded, the rest sit in a strip. `flow` orders
/// the two children (the expanded tile is always first in the DOM), `box`
/// sizes and scrolls the strip, `item` sizes one tile inside it.
const STRIP = {
  top: {
    flow: "flex-col-reverse",
    box: "h-24 flex-row overflow-x-auto sm:h-32",
    item: "h-full aspect-video shrink-0",
  },
  bottom: {
    flow: "flex-col",
    box: "h-24 flex-row overflow-x-auto sm:h-32",
    item: "h-full aspect-video shrink-0",
  },
  left: {
    flow: "flex-row-reverse",
    box: "w-32 flex-col overflow-y-auto sm:w-44",
    item: "w-full aspect-video shrink-0",
  },
  right: {
    flow: "flex-row",
    box: "w-32 flex-col overflow-y-auto sm:w-44",
    item: "w-full aspect-video shrink-0",
  },
};

/**
 * A joined meeting: the video area, the overlay controls, and the SDK session
 * behind them.
 *
 * Joining happens on mount, from the `code` alone — `MeetingPanel` is what
 * decides which code that is. `POST /private/meetings/{code}/attendees` hands
 * back the `Meeting`/`Attendee` pair `MeetingSessionConfiguration` wants.
 *
 * What renders is `roster`, one entry per attendee the SDK reports present,
 * not one per video tile: an attendee with no camera keeps their area and gets
 * a placeholder, so toggling a camera never reshuffles the layout. A screen
 * share arrives as its own attendee (Chime gives it the `#content` modality)
 * and so gets its own area too.
 *
 * The SDK objects are refs: mutable, long-lived, and nothing renders from them.
 */
export default function Meeting({ code, token, onLeave }) {
  const [status, setStatus] = useState("joining"); // joining | live | error | ended
  const [error, setError] = useState("");
  const [notice, setNotice] = useState(""); // "X muted you", and the like
  const [roster, setRoster] = useState([]);
  const [muted, setMuted] = useState(false);
  const [cameraOn, setCameraOn] = useState(false);
  const [sharing, setSharing] = useState(false);
  const [focused, setFocused] = useState(null); // attendee id, or null for the grid
  const [settings, setSettings] = useState({
    panel: "bottom left",
    participants: "right",
  });

  const sessionRef = useRef(null);
  const audioRef = useRef(null);
  const tileOwners = useRef(new Map()); // tileId -> attendee id
  const boundElements = useRef(new Map()); // tileId -> the element bound to it
  const volumeSubscriptions = useRef(new Set()); // attendee ids already watched
  const meRef = useRef(null); // this browser's attendee id
  const noticeTimer = useRef(null);

  /// Add or patch one attendee, keeping the order they arrived in.
  ///
  /// It upserts because the two sources disagree about order: presence usually
  /// arrives first, but a video tile for an attendee nobody has announced yet
  /// would otherwise be dropped. Whichever comes first creates the entry —
  /// except when `insert` is false, which is how a late event about somebody
  /// who already left avoids resurrecting them.
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

  /// Bind a tile to the <video> now showing it, or forget one that unmounted.
  /// The layout moves elements between containers, so React remounts them and
  /// this runs again; the map is what keeps a plain re-render from rebinding.
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
    const session = sessionRef.current;
    sessionRef.current = null;
    await disposeSession(session);
    onLeave?.();
  }, [disposeSession, onLeave]);

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
        // The meeting ended, or the connection gave up.
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

  // Join on mount, and again if the code or token changes. Everything after an
  // await checks `cancelled`, because StrictMode runs this effect twice and a
  // stray session would be a second attendee nobody can see.
  useEffect(() => {
    let cancelled = false;
    volumeSubscriptions.current.clear();
    tileOwners.current.clear();

    (async () => {
      const response = await callApi({
        method: "POST",
        path: `/private/meetings/${encodeURIComponent(code)}/attendees`,
        token,
      });
      if (cancelled) return;
      if (!response.ok) {
        setStatus("error");
        setError(describeFailure(response));
        return;
      }

      let session;
      try {
        session = buildSession(response.data);
        if (cancelled) {
          await disposeSession(session);
          return;
        }
        sessionRef.current = session;
        await startMedia(session);
        if (cancelled) return;
        setStatus("live");
      } catch (failure) {
        // Almost always a denied microphone permission or an insecure origin.
        sessionRef.current = null;
        await disposeSession(session);
        if (cancelled) return;
        setStatus("error");
        setError(String(failure));
      }
    })();

    return () => {
      cancelled = true;
      const session = sessionRef.current;
      sessionRef.current = null;
      void disposeSession(session);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [code, token]);

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

  const cells = useMemo(
    () =>
      roster.map((entry) => ({
        key: entry.attendeeId,
        label: cellLabel(entry),
        local: entry.local,
        content: entry.content,
        tileId: entry.tileId,
        muted: entry.muted,
        active: entry.active,
      })),
    [roster],
  );

  // An expanded participant who left is no longer expanded.
  const expanded = cells.find((cell) => cell.key === focused) ?? null;
  const others = expanded ? cells.filter((cell) => cell !== expanded) : cells;
  const strip = STRIP[settings.participants];

  function tileFor(cell, isExpanded) {
    return (
      <ParticipantTile
        cell={cell}
        expanded={isExpanded}
        onToggle={() => setFocused(isExpanded ? null : cell.key)}
        onBindVideo={bindVideo}
        onMute={() => moderate(cell, "mute")}
        onCameraOff={() => moderate(cell, "camera-off")}
      />
    );
  }

  return (
    <div className="relative h-[70vh] min-h-[380px] w-full overflow-hidden rounded-xl border border-slate-800 bg-slate-950">
      {expanded ? (
        <div className={`flex h-full w-full gap-2 p-2 ${strip.flow}`}>
          <div className="min-h-0 min-w-0 flex-1">{tileFor(expanded, true)}</div>
          {others.length > 0 && (
            <div className={`flex shrink-0 gap-2 ${strip.box}`}>
              {others.map((cell) => (
                <div key={cell.key} className={strip.item}>
                  {tileFor(cell, false)}
                </div>
              ))}
            </div>
          )}
        </div>
      ) : (
        <div className="grid h-full w-full auto-rows-fr grid-cols-[repeat(auto-fit,minmax(220px,1fr))] gap-2 overflow-y-auto p-2">
          {cells.map((cell) => (
            <div key={cell.key} className="min-h-0 min-w-0">
              {tileFor(cell, false)}
            </div>
          ))}
        </div>
      )}

      {status !== "live" && (
        <div className="absolute inset-0 z-20 flex items-center justify-center bg-slate-950/85 p-6">
          <div className="max-w-md text-center">
            <p className="font-mono text-sm text-slate-300">
              {status === "joining" && `joining ${code}...`}
              {status === "ended" && "the meeting ended"}
              {status === "error" && "could not join"}
            </p>
            {error && (
              <p className="mt-3 rounded-lg border border-rose-800 bg-slate-950 p-3 font-mono text-xs break-all text-rose-300">
                {error}
              </p>
            )}
            {status !== "joining" && (
              <button
                type="button"
                onClick={leave}
                className="mt-4 rounded-lg bg-slate-700 px-4 py-1.5 text-sm font-medium text-white hover:bg-slate-600"
              >
                back
              </button>
            )}
          </div>
        </div>
      )}

      {status === "live" && (
        <>
          <ControlPanel
            settings={settings}
            onSettings={setSettings}
            muted={muted}
            onToggleMute={toggleMute}
            cameraOn={cameraOn}
            onToggleCamera={toggleCamera}
            sharing={sharing}
            onToggleShare={toggleShare}
            onLeave={leave}
          />
          {(error || notice) && (
            <p
              className={`absolute top-3 left-1/2 z-10 max-w-[80%] -translate-x-1/2 rounded-lg border bg-slate-950/90 px-3 py-1.5 font-mono text-xs break-all ${
                error
                  ? "border-rose-800 text-rose-300"
                  : "border-slate-700 text-slate-300"
              }`}
            >
              {error || notice}
            </p>
          )}
        </>
      )}

      {/* The SDK plays every remote attendee through this one element. */}
      <audio ref={audioRef} className="hidden" />
    </div>
  );
}

/// The backend appends a per-join suffix to keep ExternalUserIds unique, and
/// Chime appends `#content` to a screen share's; show the email in front.
function cellLabel({ externalUserId, local, content }) {
  const name = shortLabel(externalUserId);
  if (content) return `${name} — screen`;
  return local ? `${name} (you)` : name;
}

function shortLabel(externalUserId) {
  return String(externalUserId ?? "").split("#")[0] || "attendee";
}
