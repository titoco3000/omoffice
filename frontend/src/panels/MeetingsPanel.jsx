import { useEffect, useRef, useState } from "react";
import { decodeJwt } from "../lib/jwt";
import { messenger } from "../lib/messenger";
import {
  IDLE_AFTER_SECONDS,
  MEETING_LIST_CHANNEL,
  MEETING_ROOM_CHANNEL,
  createMeeting,
  joinMeeting,
  leaveMeeting,
  listMeetings,
} from "../lib/meetings";
import * as chime from "../lib/chime";

export default function MeetingsPanel({ token, org }) {
  const myId = decodeJwt(token)?.sub;
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
  const [error, setError] = useState(null);
  // Mirrors `myMeetingId` for the unmount cleanup below, which is set up
  // once per mounted org and would otherwise only ever see whatever it was
  // at that moment, not whatever it is by the time the tab actually leaves.
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
  // is set (see the render below) -- wiring this up once on mount would
  // have grabbed `null` forever, since that element hasn't been created
  // yet at that point. Re-running whenever `myMeetingId` changes catches it
  // exactly when it mounts (and clears it when it unmounts).
  useEffect(() => {
    chime.setSelfVideoElement(selfVideoRef.current);
    return () => chime.setSelfVideoElement(null);
  }, [myMeetingId]);

  const reloadMeetings = () =>
    listMeetings(token, org.id)
      .then((data) => setMeetings(data.meetings ?? []))
      .catch((err) => setError(err.message));

  useEffect(() => {
    reloadMeetings();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [token, org.id]);

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

  // Live updates: a meeting appearing or disappearing anywhere in this org
  // lands here -- see `features::meetings::broadcast_list`
  // (backend/lib/features/meetings/mod.rs). If the meeting this tab is in
  // just vanished from under it (terminated elsewhere -- everyone else
  // left, or it sat idle too long), drop out of it locally too.
  useEffect(
    () =>
      messenger.on(MEETING_LIST_CHANNEL, (message) => {
        if (message.org_id !== org.id) return;
        const next = message.meetings ?? [];
        setMeetings(next);
        if (myMeetingId && !next.some((m) => m.id === myMeetingId)) {
          teardownMedia();
          setMyMeetingId(null);
          setRoomStatus(null);
        }
      }),

    [org.id, myMeetingId],
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

  // Leaving this panel (switching org, logging out) shouldn't leave a ghost
  // attendee behind, or the camera running.
  useEffect(() => {
    return () => {
      teardownMedia();
      const meetingId = myMeetingIdRef.current;
      if (meetingId) leaveMeeting(token, meetingId).catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [org.id]);

  const create = async () => {
    setError(null);
    try {
      const { meeting } = await createMeeting(token, org.id);
      // No room yet -- nobody else is in it. The frontend just shows
      // itself as "in" the meeting; see the module doc on
      // `features::meetings::join`.
      setMyMeetingId(meeting.id);
      setRoomStatus("waiting");
    } catch (err) {
      setError(err.message);
    }
  };

  const join = async (meetingId) => {
    setError(null);
    try {
      const data = await joinMeeting(token, meetingId);
      setMyMeetingId(meetingId);
      if (data.connected) {
        await chime.connect(data.Meeting, data.Attendee);
        setRoomStatus("connected");
      } else {
        setRoomStatus("waiting");
      }
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
  useEffect(() => {
    if (roomStatus !== "waiting") return;
    const timer = setTimeout(() => leave(), IDLE_AFTER_SECONDS * 1000);
    return () => clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [roomStatus, myMeetingId]);

  return (
    <section className="rounded-card border bg-surface p-5">
      {/* Chime's audio output -- never rendered as a control, just a sink. */}
      <audio ref={audioRef} autoPlay hidden />

      <div className="flex items-center justify-between">
        <h2 className="text-lg font-semibold text-ink">Meetings</h2>
        <button
          onClick={create}
          disabled={!!myMeetingId}
          className="rounded-card bg-brand px-3 py-1.5 text-sm font-medium text-on-brand hover:bg-brand-strong disabled:opacity-50"
        >
          New meeting
        </button>
      </div>

      {myMeetingId && (
        <div className="mt-3 rounded-card border bg-bg p-3">
          <div className="flex flex-wrap gap-2">
            {/* Own preview -- a chimeless meeting still shows this, see
                `setCameraOn` in lib/chime.js. Mirrored and muted: it's a
                mirror of yourself, not a source of sound. */}
            <video
              ref={selfVideoRef}
              autoPlay
              muted
              playsInline
              className="aspect-video w-40 -scale-x-100 rounded-card bg-black object-cover"
            />
            {remoteTiles.map((tile) => (
              <video
                key={tile.tileId}
                ref={(element) => chime.bindRemoteVideo(tile.tileId, element)}
                autoPlay
                playsInline
                className={`aspect-video rounded-card bg-black object-cover ${
                  tile.isContent ? "w-full sm:w-80" : "w-40"
                }`}
              />
            ))}
          </div>

          <div className="mt-3 flex flex-wrap gap-2 text-sm">
            <button
              onClick={toggleCamera}
              className={`rounded-card border px-3 py-1.5 ${cameraOn ? "text-ink" : "text-ink-muted"}`}
            >
              {cameraOn ? "Turn camera off" : "Turn camera on"}
            </button>
            <button
              onClick={toggleMic}
              className={`rounded-card border px-3 py-1.5 ${micMuted ? "text-ink-muted" : "text-ink"}`}
            >
              {micMuted ? "Unmute" : "Mute"}
            </button>
            <button
              onClick={toggleScreenShare}
              disabled={roomStatus !== "connected"}
              className={`rounded-card border px-3 py-1.5 disabled:opacity-50 ${sharingScreen ? "text-ink" : "text-ink-muted"}`}
            >
              {sharingScreen ? "Stop sharing screen" : "Share screen"}
            </button>
          </div>
        </div>
      )}

      {meetings.length === 0 && (
        <p className="mt-3 text-sm text-ink-muted">No meetings right now.</p>
      )}

      <ul className="mt-3 space-y-1.5 text-sm">
        {meetings.map((meeting) => {
          const mine = meeting.id === myMeetingId;
          return (
            <li
              key={meeting.id}
              className="flex items-center justify-between rounded-card border px-3 py-2"
            >
              <span className="text-ink">
                {mine && (
                  // Debug aid: whether this tab actually has a live Chime
                  // session for its own meeting, or is just sitting in the
                  // app-level "waiting" state with no audio connected.
                  <span
                    className={`mr-1.5 inline-block ${roomStatus === "connected" ? "text-success" : "text-ink-muted/40"}`}
                    title={
                      roomStatus === "connected"
                        ? "connected to chime"
                        : "not connected to chime"
                    }
                  >
                    ●
                  </span>
                )}
                {meeting.created_by === myId ? "Your meeting" : "Meeting"}{" "}
                <span className="text-ink-muted">{meeting.id.slice(0, 8)}</span>
                {mine && (
                  <span className="ml-2 text-xs text-ink-muted">
                    {roomStatus === "connected"
                      ? "-- connected"
                      : "-- waiting for others"}
                  </span>
                )}
              </span>
              {mine ? (
                <button onClick={leave} className="text-danger hover:underline">
                  Leave
                </button>
              ) : (
                <button
                  onClick={() => join(meeting.id)}
                  disabled={!!myMeetingId}
                  className="text-brand hover:underline disabled:opacity-50"
                >
                  Join
                </button>
              )}
            </li>
          );
        })}
      </ul>

      {error && <p className="mt-3 text-sm text-danger">{error}</p>}
    </section>
  );
}
