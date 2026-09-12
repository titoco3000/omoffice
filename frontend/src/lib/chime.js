//! One live Amazon Chime SDK session for the whole tab, same as `messenger`
//! is one WebSocket connection for it -- a browser tab is only ever really
//! in one meeting's room at a time.
//!
//! The device controller is created once and reused across every session
//! `connect` makes, rather than one per session: that is what lets the
//! camera preview work with no session at all -- `startVideoPreviewForVideoInput`
//! is a `DeviceControllerFacade` method, not an `AudioVideoFacade` one, so
//! it works before a meeting exists and keeps working after `disconnect`
//! drops one, not only while a room is actually live. `connect`/`disconnect`
//! only ever start and stop the Chime session itself; camera and mic state
//! (`cameraOn`/`micMuted`) is this module's own, set by `setCameraOn`/
//! `setMicMuted` independently of whether a session exists, and reapplied
//! automatically whenever one starts.

import {
  ConsoleLogger,
  DefaultDeviceController,
  DefaultMeetingSession,
  LogLevel,
  MeetingSessionConfiguration,
} from "amazon-chime-sdk-js";

const logger = new ConsoleLogger("omoffice", LogLevel.WARN);

// One device controller for the whole tab -- see the module doc.
const deviceController = new DefaultDeviceController(logger);

let session = null;
let audioElement = null;
let selfVideoElement = null;

let cameraOn = false;
let micMuted = false;
let sharingScreen = false;

let audioInputReady = null; // promise, so the mic is only ever grabbed once
const remoteTiles = new Map(); // tileId -> { attendeeId, isContent }
let onRemoteTilesChange = () => {};
let onScreenShareEnded = () => {};

function notifyRemoteTiles() {
  onRemoteTilesChange(
    Array.from(remoteTiles, ([tileId, tile]) => ({ tileId, ...tile })),
  );
}

/// The `<audio>` element Chime's remote audio is bound to.
export function setAudioElement(element) {
  audioElement = element;
  if (session && element) session.audioVideo.bindAudioElement(element);
}

/// The `<video>` element this tab's own camera is bound to -- a chimeless
/// preview if no session is live, the local tile if one is.
export function setSelfVideoElement(element) {
  selfVideoElement = element;
  if (!cameraOn || !element) return;
  if (session) {
    const tile = session.audioVideo.getLocalVideoTile();
    if (tile) session.audioVideo.bindVideoElement(tile.id(), element);
  } else {
    deviceController.startVideoPreviewForVideoInput(element);
  }
}

/// Called with the current list of `{tileId, attendeeId, isContent}` for
/// every video tile *other* than this tab's own, whenever one appears,
/// changes or disappears. `isContent` marks a screen share rather than a
/// camera.
export function onRemoteTiles(callback) {
  onRemoteTilesChange = callback;
}

/// Called when a screen share this tab started stops from outside its own
/// `stopScreenShare` call -- the browser's own "Stop sharing" control, most
/// often.
export function onScreenShareStopped(callback) {
  onScreenShareEnded = callback;
}

async function ensureAudioInput() {
  if (!audioInputReady) {
    audioInputReady = (async () => {
      const mics = await deviceController.listAudioInputDevices();
      if (mics[0]) await deviceController.startAudioInput(mics[0].deviceId);
    })();
  }
  return audioInputReady;
}

/// Turn this tab's own camera on or off -- works the same whether or not a
/// Chime session is currently live, which is what keeps a chimeless
/// meeting's preview and a connected meeting's local tile behaving
/// identically from the caller's point of view.
export async function setCameraOn(on) {
  cameraOn = on;
  if (!on) {
    if (session) session.audioVideo.stopLocalVideoTile();
    if (selfVideoElement)
      deviceController.stopVideoPreviewForVideoInput(selfVideoElement);
    await deviceController.stopVideoInput();
    return;
  }

  const cameras = await deviceController.listVideoInputDevices();
  if (cameras[0]) await deviceController.startVideoInput(cameras[0].deviceId);

  if (session) {
    session.audioVideo.startLocalVideoTile();
    // The element gets bound once `videoTileDidUpdate` reports the local
    // tile's id, in `connect`'s observer.
  } else if (selfVideoElement) {
    deviceController.startVideoPreviewForVideoInput(selfVideoElement);
  }
}

export function isCameraOn() {
  return cameraOn;
}

/// Mute or unmute this tab's own mic. Tracked even with no session live, so
/// a preference set before `connect` (or kept from before a `disconnect`)
/// carries into whichever session comes next.
export async function setMicMuted(muted) {
  micMuted = muted;
  if (!session) return;
  if (muted) session.audioVideo.realtimeMuteLocalAudio();
  else session.audioVideo.realtimeUnmuteLocalAudio();
}

export function isMicMuted() {
  return micMuted;
}

/// Share this tab's screen. Only meaningful with a live session -- there is
/// nobody to see it otherwise -- so this is a no-op with none.
export async function startScreenShare() {
  if (!session) return;
  await session.audioVideo.startContentShareFromScreenCapture();
  sharingScreen = true;
}

export function stopScreenShare() {
  if (!session) return;
  session.audioVideo.stopContentShare();
  sharingScreen = false;
}

export function isSharingScreen() {
  return sharingScreen;
}

/// Bind one remote tile (from `onRemoteTiles`) to a `<video>` element.
export function bindRemoteVideo(tileId, element) {
  if (session && element) session.audioVideo.bindVideoElement(tileId, element);
}

/// Tear down whatever session is live, then start a fresh one for
/// `meetingInfo`/`attendeeInfo` -- the `Meeting`/`Attendee` shapes
/// `chime::meeting_json`/`chime::attendee_json` send
/// (backend/lib/core/chime.rs). Reapplies whatever camera/mic state was
/// already set, rather than starting silent and camera-off regardless.
export async function connect(meetingInfo, attendeeInfo) {
  await disconnect();
  await ensureAudioInput();

  const configuration = new MeetingSessionConfiguration(
    meetingInfo,
    attendeeInfo,
  );
  const nextSession = new DefaultMeetingSession(
    configuration,
    logger,
    deviceController,
  );

  nextSession.audioVideo.addObserver({
    videoTileDidUpdate: (tileState) => {
      if (tileState.localTile) {
        if (selfVideoElement) {
          nextSession.audioVideo.bindVideoElement(
            tileState.tileId,
            selfVideoElement,
          );
        }
        return;
      }
      if (tileState.boundAttendeeId == null) return;
      remoteTiles.set(tileState.tileId, {
        attendeeId: tileState.boundAttendeeId,
        isContent: tileState.isContent,
      });
      notifyRemoteTiles();
    },
    videoTileWasRemoved: (tileId) => {
      nextSession.audioVideo.unbindVideoElement(tileId);
      remoteTiles.delete(tileId);
      notifyRemoteTiles();
    },
  });
  nextSession.audioVideo.addContentShareObserver({
    contentShareDidStop: () => {
      sharingScreen = false;
      onScreenShareEnded();
    },
  });

  if (audioElement) nextSession.audioVideo.bindAudioElement(audioElement);
  if (selfVideoElement) {
    deviceController.stopVideoPreviewForVideoInput(selfVideoElement);
  }

  session = nextSession;
  session.audioVideo.start();

  if (micMuted) session.audioVideo.realtimeMuteLocalAudio();
  if (cameraOn) {
    const cameras = await deviceController.listVideoInputDevices();
    if (cameras[0]) await deviceController.startVideoInput(cameras[0].deviceId);
    session.audioVideo.startLocalVideoTile();
  }

  return session;
}

/// Stop and drop the current session, if any -- camera and mic state are
/// left alone (see `setCameraOn`), so a camera that was on keeps previewing
/// locally rather than going dark the moment the room disappears.
export async function disconnect() {
  if (!session) return;
  const previous = session;
  session = null;
  remoteTiles.clear();
  notifyRemoteTiles();
  sharingScreen = false;
  try {
    previous.audioVideo.stop();
  } catch {
    // Already torn down (the room was cleared out from under it, say) --
    // nothing left to stop.
  }
  if (cameraOn && selfVideoElement) {
    deviceController.startVideoPreviewForVideoInput(selfVideoElement);
  }
}

export function isConnected() {
  return session !== null;
}
