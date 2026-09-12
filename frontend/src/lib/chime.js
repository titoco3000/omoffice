//! One live Amazon Chime SDK session for the whole tab, same as `messenger`
//! is one WebSocket connection for it -- a browser tab is only ever really
//! in one meeting's room at a time. Audio only for now: whoever calls
//! `connect` gets voice, no camera/screen-share wiring yet.

import {
  ConsoleLogger,
  DefaultDeviceController,
  DefaultMeetingSession,
  LogLevel,
  MeetingSessionConfiguration,
} from "amazon-chime-sdk-js";

let session = null;
let audioElement = null;

/// The `<audio>` element `connect` binds Chime's output to. Safe to call
/// before any session exists -- `connect` re-binds against whatever this
/// currently points at.
export function setAudioElement(element) {
  audioElement = element;
}

/// Tear down whatever session is live, then start a fresh one for
/// `meetingInfo`/`attendeeInfo` -- the `Meeting`/`Attendee` shapes
/// `chime::meeting_json`/`chime::attendee_json` send
/// (backend/lib/core/chime.rs). Picks whatever microphone the browser
/// reports first; nothing here presents a device picker yet.
export async function connect(meetingInfo, attendeeInfo) {
  await disconnect();

  const logger = new ConsoleLogger("omoffice", LogLevel.WARN);
  const deviceController = new DefaultDeviceController(logger);
  const configuration = new MeetingSessionConfiguration(
    meetingInfo,
    attendeeInfo,
  );
  const nextSession = new DefaultMeetingSession(
    configuration,
    logger,
    deviceController,
  );

  const audioInputs = await nextSession.audioVideo.listAudioInputDevices();
  if (audioInputs[0]) {
    await nextSession.audioVideo.startAudioInput(audioInputs[0].deviceId);
  }
  if (audioElement) {
    nextSession.audioVideo.bindAudioElement(audioElement);
  }
  nextSession.audioVideo.start();

  session = nextSession;
  return session;
}

/// Stop and drop the current session, if any. Safe to call with none live.
export async function disconnect() {
  if (!session) return;
  try {
    session.audioVideo.stop();
  } catch {
    // Already torn down (the room was cleared out from under it, say) --
    // nothing left to stop.
  }
  session = null;
}

export function isConnected() {
  return session !== null;
}
