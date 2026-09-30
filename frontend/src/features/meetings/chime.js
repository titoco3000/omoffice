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

const remoteTiles = new Map(); // tileId -> { attendeeId, externalUserId, isContent }
const attendeeExternalIds = new Map(); // chime attendeeId -> ExternalUserId
const speakingAttendeeIds = new Set();
const mutedAttendeeIds = new Set();
let onRemoteTilesChange = () => {};
let onScreenShareEnded = () => {};
let onSpeakingIdsChange = () => {};
let onMutedIdsChange = () => {};

function notifyRemoteTiles() {
  onRemoteTilesChange(
    Array.from(remoteTiles, ([tileId, tile]) => ({ tileId, ...tile })),
  );
}

function notifySpeaking() {
  onSpeakingIdsChange(
    new Set([...speakingAttendeeIds].map((id) => userIdFor(id))),
  );
}

function notifyMuted() {
  onMutedIdsChange(new Set([...mutedAttendeeIds].map((id) => userIdFor(id))));
}

// `ExternalUserId` is `${userId}#${joinSuffix}` (see `chime::external_user_id`,
// backend/lib/core/chime.rs) -- this is that mapping run backward, against
// whichever attendeeId presence has reported it for.
function userIdFor(attendeeId) {
  return externalToUserId(attendeeExternalIds.get(attendeeId));
}

/// The app user id a Chime `ExternalUserId` was minted from.
export function externalToUserId(externalUserId) {
  return externalUserId ? externalUserId.split("#")[0] : externalUserId;
}

/// The `<audio>` element Chime's remote audio is bound to.
export function setAudioElement(element) {
  audioElement = element;
  if (session && element) session.audioVideo.bindAudioElement(element);
}

/// The `<video>` element this tab's own camera is bound to -- a chimeless
/// preview if no session is live, the local tile if one is. With no session,
/// the element coming and going is also what acquires and releases the
/// camera itself (see `syncCamera`).
export function setSelfVideoElement(element) {
  selfVideoElement = element;
  if (!cameraOn) return;
  if (session) {
    const tile = session.audioVideo.getLocalVideoTile();
    if (tile && element)
      session.audioVideo.bindVideoElement(tile.id(), element);
  } else {
    syncCamera().catch(() => {});
  }
}

/// Called with the current list of `{tileId, attendeeId, externalUserId,
/// isContent, isSelf}` for every video tile whenever one appears, changes
/// or disappears -- every *remote* one (camera or screen share), plus this
/// tab's own screen share if it's currently presenting one (`isSelf: true`,
/// no `attendeeId`/`externalUserId` reported for it). The camera's own
/// local tile is never in here -- that one binds straight to
/// `setSelfVideoElement` instead, same as ever. `isContent` marks a screen
/// share rather than a camera.
export function onRemoteTiles(callback) {
  onRemoteTilesChange = callback;
}

/// Called when a screen share this tab started stops from outside its own
/// `stopScreenShare` call -- the browser's own "Stop sharing" control, most
/// often.
export function onScreenShareStopped(callback) {
  onScreenShareEnded = callback;
}

// Called with the current set of user ids whose mic is currently producing
// sound, any time it changes.
export function onSpeakingChange(callback) {
  onSpeakingIdsChange = callback;
}

// Called with the current set of remote user ids whose mic is muted, any
// time it changes -- driven by the same volume indicator subscription as
// speaking state, since Chime reports both on the one callback.
export function onMutedChange(callback) {
  onMutedIdsChange = callback;
}

const SPEAKING_THRESHOLD = 0.4;

// Re-grabbed fresh on every `connect`, same as the camera in `setCameraOn`
// -- caching this across the tab's whole life meant one bad first grab (a
// permission prompt still pending, a stream that died with a past session)
// stuck silently for every meeting after, since nothing ever retried it.
async function ensureAudioInput() {
  const mics = await deviceController.listAudioInputDevices();
  // Explicit constraints: B on speakers otherwise re-captures A's voice and
  // sends it back to A as echo.
  if (mics[0]) {
    await deviceController.startAudioInput({
      deviceId: { exact: mics[0].deviceId },
      echoCancellation: true,
      noiseSuppression: true,
      autoGainControl: true,
    });
  }
}

/// Release this tab's microphone hardware -- called on a full leave. Without this the mic device
/// stayed open (browser still shows the tab as using it) even after the
/// meeting ended, since nothing ever called the matching `stopAudioInput`.
/// `micMuted` is left alone, same as `cameraOn` is for the camera, so the
/// next join still respects whatever mute preference was set.
export async function releaseMic() {
  await deviceController.stopAudioInput();
}

// Whether the camera hardware is currently acquired -- separate from
// `cameraOn`, which is only the wish: with no session and no element to
// preview into, nothing would see the camera, so it's released even while
// `cameraOn` stays true.
let cameraInputOn = false;
let cameraQueue = Promise.resolve();

async function applyCamera() {
  const wanted = cameraOn && (session !== null || selfVideoElement !== null);
  if (!wanted) {
    if (session) session.audioVideo.stopLocalVideoTile();
    if (selfVideoElement)
      deviceController.stopVideoPreviewForVideoInput(selfVideoElement);
    if (cameraInputOn) {
      cameraInputOn = false;
      await deviceController.stopVideoInput();
    }
    return;
  }

  if (!cameraInputOn) {
    const cameras = await deviceController.listVideoInputDevices();
    if (cameras[0]) await deviceController.startVideoInput(cameras[0].deviceId);
    cameraInputOn = true;
  }

  if (session) {
    session.audioVideo.startLocalVideoTile();
    // The element gets bound once `videoTileDidUpdate` reports the local
    // tile's id, in `connect`'s observer.
  } else {
    deviceController.startVideoPreviewForVideoInput(selfVideoElement);
  }
}

// Bring the camera hardware and preview in line with `cameraOn`, `session`
// and `selfVideoElement` as they are by the time this runs. Queued, so
// overlapping calls (an element swap mid-`startVideoInput`) settle in
// order rather than racing each other.
function syncCamera() {
  const run = cameraQueue.then(applyCamera);
  cameraQueue = run.catch(() => {});
  return run;
}

/// Turn this tab's own camera on or off -- works the same whether or not a
/// Chime session is currently live, which is what keeps a chimeless
/// meeting's preview and a connected meeting's local tile behaving
/// identically from the caller's point of view.
export async function setCameraOn(on) {
  cameraOn = on;
  await syncCamera();
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
        // A local *content* tile is this tab's own screen share -- surfaced
        // through `remoteTiles` too (`isSelf: true`) so a content-tile
        // overlay (the shared board) can mount over it same as it does over
        // everyone else's, via `bindRemoteVideo`. The local camera tile
        // takes the branch below instead, same as always.
        if (tileState.isContent) {
          remoteTiles.set(tileState.tileId, { isContent: true, isSelf: true });
          notifyRemoteTiles();
          return;
        }
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
        externalUserId: tileState.boundExternalUserId,
        isContent: tileState.isContent,
      });
      notifyRemoteTiles();
    },
    videoTileWasRemoved: (tileId) => {
      nextSession.audioVideo.unbindVideoElement(tileId);
      remoteTiles.delete(tileId);
      notifyRemoteTiles();
    },
    // `stop()` tears the local video tile down asynchronously, well after it
    // returns (a network round-trip to leave, then `CleanStoppedSessionTask`
    // clears the tile's bound video element) -- resuming the preview from
    // `disconnect()` itself races that and reliably loses. `session` is
    // checked because a fast rejoin may already have replaced this session
    // by the time its own stop finishes; a stale resume here would stomp on
    // that new one's already-bound tile instead of the preview.
    audioVideoDidStop: () => {
      if (!session && cameraOn) syncCamera().catch(() => {});
    },
  });
  nextSession.audioVideo.addContentShareObserver({
    contentShareDidStop: () => {
      sharingScreen = false;
      onScreenShareEnded();
    },
  });

  // Presence tracks every attendee's `ExternalUserId` (self included, video
  // tile or not) so a tile-less audio attendee's speaking state can still
  // resolve to a user id; the volume indicator itself is what actually
  // drives `speakingAttendeeIds`.
  nextSession.audioVideo.realtimeSubscribeToAttendeeIdPresence(
    (attendeeId, present, externalUserId) => {
      if (!present) {
        attendeeExternalIds.delete(attendeeId);
        speakingAttendeeIds.delete(attendeeId);
        const wasMuted = mutedAttendeeIds.delete(attendeeId);
        nextSession.audioVideo.realtimeUnsubscribeFromVolumeIndicator(
          attendeeId,
        );
        notifySpeaking();
        if (wasMuted) notifyMuted();
        return;
      }
      attendeeExternalIds.set(attendeeId, externalUserId);
      nextSession.audioVideo.realtimeSubscribeToVolumeIndicator(
        attendeeId,
        (id, volume, muted) => {
          const speaking = !muted && (volume ?? 0) > SPEAKING_THRESHOLD;
          const speakingChanged = speaking
            ? !speakingAttendeeIds.has(id) && speakingAttendeeIds.add(id)
            : speakingAttendeeIds.delete(id);
          if (speakingChanged) notifySpeaking();

          if (muted == null) return;
          const mutedChanged = muted
            ? !mutedAttendeeIds.has(id) && mutedAttendeeIds.add(id)
            : mutedAttendeeIds.delete(id);
          if (mutedChanged) notifyMuted();
        },
      );
    },
  );

  if (audioElement) nextSession.audioVideo.bindAudioElement(audioElement);
  if (selfVideoElement) {
    deviceController.stopVideoPreviewForVideoInput(selfVideoElement);
  }

  session = nextSession;
  session.audioVideo.start();

  if (micMuted) session.audioVideo.realtimeMuteLocalAudio();
  if (cameraOn) await syncCamera();

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
  attendeeExternalIds.clear();
  speakingAttendeeIds.clear();
  notifySpeaking();
  mutedAttendeeIds.clear();
  notifyMuted();
  sharingScreen = false;
  try {
    // Resuming the preview happens in the `audioVideoDidStop` observer
    // (registered in `connect`), not here -- see the comment there.
    previous.audioVideo.stop();
  } catch {
    // Already torn down (the room was cleared out from under it, say) --
    // nothing left to stop, so `audioVideoDidStop` won't fire either;
    // resume the preview ourselves.
    if (cameraOn) syncCamera().catch(() => {});
  }
}

export function isConnected() {
  return session !== null;
}
