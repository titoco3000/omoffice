import { useCallback, useEffect, useRef, useState } from "react";
import {
  ConsoleLogger,
  DefaultDeviceController,
  DefaultMeetingSession,
  LogLevel,
  MeetingSessionConfiguration,
} from "amazon-chime-sdk-js";
import { callApi } from "../api";

/**
 * Create a Chime meeting, or join one by code, and run the media session.
 *
 * Both API calls sit behind `/private`, so this needs the token `App` holds.
 * `POST /private/meetings` hands back the code (the Chime MeetingId);
 * `POST /private/meetings/{code}/attendees` hands back the `Meeting` and
 * `Attendee` objects, which is exactly the pair `MeetingSessionConfiguration`
 * wants — the backend already keys them the way the SDK reads them.
 *
 * The SDK objects are refs, not state: they are mutable and long-lived, and
 * nothing renders from them directly. What renders is `tiles` — the video tile
 * ids the SDK reports — and each `<video>` is bound to its tile once mounted.
 */
export default function MeetingPanel({ token }) {
  const [code, setCode] = useState("");
  const [codeInput, setCodeInput] = useState("");
  const [status, setStatus] = useState("idle"); // idle | working | joined
  const [error, setError] = useState("");
  const [tiles, setTiles] = useState([]);
  const [muted, setMuted] = useState(false);
  const [cameraOn, setCameraOn] = useState(false);

  const sessionRef = useRef(null);
  const audioRef = useRef(null);
  const videoRefs = useRef(new Map());

  const joined = status === "joined";
  const busy = status === "working";

  /// Tear the media session down. Safe to call when there is none.
  const leave = useCallback(async () => {
    const session = sessionRef.current;
    sessionRef.current = null;
    if (!session) return;

    const audioVideo = session.audioVideo;
    try {
      audioVideo.stop();
      await audioVideo.stopAudioInput();
      await audioVideo.stopVideoInput();
    } catch {
      // Already torn down, or the devices went away with the tab.
    }
    setTiles([]);
    setCameraOn(false);
    setMuted(false);
    setStatus("idle");
  }, []);

  // Leaving the page is leaving the meeting.
  useEffect(() => () => void leave(), [leave]);

  // Bind every tile the SDK reported to the <video> that now exists for it.
  useEffect(() => {
    const audioVideo = sessionRef.current?.audioVideo;
    if (!audioVideo) return;
    for (const tile of tiles) {
      const element = videoRefs.current.get(tile.tileId);
      if (element) audioVideo.bindVideoElement(tile.tileId, element);
    }
  }, [tiles]);

  async function createMeeting() {
    setError("");
    setStatus("working");
    const response = await callApi({
      method: "POST",
      path: "/private/meetings",
      token,
    });
    setStatus("idle");

    if (!response.ok) {
      setError(describe(response));
      return;
    }
    setCode(response.data.code);
    setCodeInput(response.data.code);
  }

  async function joinMeeting(event) {
    event.preventDefault();
    const wanted = codeInput.trim();
    if (!wanted || busy || joined) return;

    setError("");
    setStatus("working");

    const response = await callApi({
      method: "POST",
      path: `/private/meetings/${encodeURIComponent(wanted)}/attendees`,
      token,
    });

    if (!response.ok) {
      setStatus("idle");
      setError(describe(response));
      return;
    }

    try {
      await startSession(response.data);
      setCode(wanted);
      setStatus("joined");
    } catch (failure) {
      // Almost always a denied microphone permission or an insecure origin.
      await leave();
      setError(String(failure));
    }
  }

  /// Build the SDK session and start audio. Video stays off until asked for.
  async function startSession({ Meeting, Attendee }) {
    const logger = new ConsoleLogger("chime", LogLevel.WARN);
    const deviceController = new DefaultDeviceController(logger);
    const session = new DefaultMeetingSession(
      new MeetingSessionConfiguration(Meeting, Attendee),
      logger,
      deviceController,
    );
    sessionRef.current = session;

    const audioVideo = session.audioVideo;
    audioVideo.addObserver({
      videoTileDidUpdate(tile) {
        if (!tile.boundAttendeeId) return;
        setTiles((current) => {
          const rest = current.filter((each) => each.tileId !== tile.tileId);
          return [
            ...rest,
            {
              tileId: tile.tileId,
              local: tile.localTile,
              attendee: tile.boundExternalUserId ?? tile.boundAttendeeId,
            },
          ];
        });
      },
      videoTileWasRemoved(tileId) {
        videoRefs.current.delete(tileId);
        setTiles((current) => current.filter((tile) => tile.tileId !== tileId));
      },
      audioVideoDidStop() {
        // The server ended the meeting, or the connection gave up.
        void leave();
      },
    });

    const mics = await audioVideo.listAudioInputDevices();
    if (mics.length > 0) await audioVideo.startAudioInput(mics[0].deviceId);
    await audioVideo.bindAudioElement(audioRef.current);
    audioVideo.start();
  }

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
      audioVideo.stopLocalVideoTile();
      await audioVideo.stopVideoInput();
      setCameraOn(false);
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

  return (
    <section className="rounded-xl border border-slate-800 bg-slate-900/60 p-4">
      <div className="flex flex-wrap items-center gap-2">
        <h2 className="text-sm font-medium text-slate-300">meeting</h2>
        <span
          className={`rounded px-2 py-0.5 text-xs ${
            token ? "bg-sky-500/15 text-sky-300" : "bg-rose-500/15 text-rose-300"
          }`}
        >
          {token ? "token" : "no token"}
        </span>
        {joined && (
          <span className="rounded bg-emerald-500/15 px-2 py-0.5 text-xs text-emerald-300">
            live
          </span>
        )}
      </div>

      <div className="mt-3 flex flex-wrap items-center gap-3">
        <button
          type="button"
          onClick={createMeeting}
          disabled={busy || joined}
          className="rounded-lg bg-sky-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-sky-500 disabled:opacity-50"
        >
          create meeting
        </button>

        <form onSubmit={joinMeeting} className="flex flex-1 gap-2">
          <input
            type="text"
            value={codeInput}
            onChange={(event) => setCodeInput(event.target.value)}
            placeholder="meeting code"
            disabled={joined}
            className="h-9 min-w-0 flex-1 rounded-lg border border-slate-700 bg-slate-950 px-2 font-mono text-sm text-slate-100 outline-none focus:border-sky-500 disabled:opacity-50"
          />
          <button
            type="submit"
            disabled={busy || joined || !codeInput.trim()}
            className="rounded-lg bg-slate-700 px-4 py-1.5 text-sm font-medium text-white hover:bg-slate-600 disabled:opacity-50"
          >
            {busy ? "..." : "join meeting"}
          </button>
        </form>
      </div>

      {code && (
        <div className="mt-3 rounded-lg bg-slate-950 p-3">
          <span className="font-mono text-xs text-slate-500">meeting code</span>
          <p className="mt-1 font-mono text-sm break-all text-emerald-300">
            {code}
          </p>
        </div>
      )}

      {error && (
        <p className="mt-3 rounded-lg border border-rose-800 bg-slate-950 p-3 font-mono text-xs break-all text-rose-300">
          {error}
        </p>
      )}

      {joined && (
        <div className="mt-3 flex flex-wrap items-center gap-2">
          <button
            type="button"
            onClick={toggleMute}
            className="rounded-lg border border-slate-700 px-3 py-1.5 text-sm text-slate-200 hover:bg-slate-800"
          >
            {muted ? "unmute" : "mute"}
          </button>
          <button
            type="button"
            onClick={toggleCamera}
            className="rounded-lg border border-slate-700 px-3 py-1.5 text-sm text-slate-200 hover:bg-slate-800"
          >
            {cameraOn ? "camera off" : "camera on"}
          </button>
          <button
            type="button"
            onClick={leave}
            className="rounded-lg bg-rose-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-rose-500"
          >
            leave
          </button>
          <span className="font-mono text-xs text-slate-500">
            {tiles.length === 0 ? "audio only" : `${tiles.length} video`}
          </span>
        </div>
      )}

      {tiles.length > 0 && (
        <div className="mt-3 grid gap-3 sm:grid-cols-2">
          {tiles.map((tile) => (
            <div
              key={tile.tileId}
              className="overflow-hidden rounded-lg border border-slate-800 bg-black"
            >
              <video
                ref={(element) => {
                  if (element) videoRefs.current.set(tile.tileId, element);
                  else videoRefs.current.delete(tile.tileId);
                }}
                autoPlay
                playsInline
                muted={tile.local}
                className="aspect-video w-full object-cover"
              />
              <p className="truncate px-2 py-1 font-mono text-xs text-slate-400">
                {tile.local ? "you" : label(tile.attendee)}
              </p>
            </div>
          ))}
        </div>
      )}

      {/* The SDK plays every remote attendee through this one element. */}
      <audio ref={audioRef} className="hidden" />
    </section>
  );
}

/// Turn a failed `callApi` result into one line.
function describe({ status, data }) {
  const detail = data?.message ?? data?.error ?? JSON.stringify(data);
  return status === 0 ? `request failed: ${detail}` : `${status}: ${detail}`;
}

/// The backend appends a per-join suffix to keep ExternalUserIds unique; show
/// the email in front of it.
function label(externalUserId) {
  return String(externalUserId ?? "").split("#")[0] || "attendee";
}
