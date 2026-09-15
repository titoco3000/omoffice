//! The dock's content: one tile per attendee, a chat column/view, and the
//! controls row (leave/camera/mic/share on the left, the chat toggle on the
//! right). `MeetingDock` decides where and how big this sits on screen;
//! this component only knows how to fill whatever space it's given.

import { useState } from "react";
import { FiMessageCircle } from "react-icons/fi";
import * as chime from "../lib/chime";
import { preferredName } from "../lib/names";
import Spinner from "../components/Spinner";
import MeetingChat from "./MeetingChat";
import MeetingControls from "./MeetingControls";
import MeetingTile from "./MeetingTile";

const STATUS_LABEL = {
  connecting: "Connecting…",
  waiting: "Waiting for others",
  connected: "Connected",
};

export default function MeetingPanel({
  session,
  members,
  myId,
  isMobile,
  compact,
}) {
  const [chatOpen, setChatOpen] = useState(false);
  const {
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
    chatMessages,
    sendChatMessage,
    muteAttendee,
    turnOffAttendeeCamera,
    leave,
    toggleCamera,
    toggleMic,
    toggleScreenShare,
  } = session;

  const others = (
    meetings.find((m) => m.id === myMeetingId)?.attendees ?? []
  ).filter((userId) => userId !== myId);

  const cameraTileByUser = new Map();
  const contentTiles = [];
  for (const tile of remoteTiles) {
    if (tile.isContent) contentTiles.push(tile);
    else
      cameraTileByUser.set(chime.externalToUserId(tile.externalUserId), tile);
  }

  const showTiles = !isMobile || !chatOpen;
  const showChat = chatOpen;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <audio ref={audioRef} autoPlay hidden />

      <div className="flex min-h-0 flex-1">
        {showTiles && (
          <div
            className={`min-w-0 flex-1 overflow-y-auto p-3 ${compact ? "max-h-[70vh]" : ""}`}
          >
            <div className="flex flex-wrap gap-2">
              <MeetingTile
                name="You"
                isSelf
                videoRef={selfVideoRef}
                hasVideo={cameraOn}
                speaking={speakingUserIds.has(myId)}
                muted={micMuted}
              />
              {others.map((userId) => {
                const tile = cameraTileByUser.get(userId);
                return (
                  <MeetingTile
                    key={userId}
                    name={preferredName(userId, members)}
                    videoRef={
                      tile
                        ? (el) => chime.bindRemoteVideo(tile.tileId, el)
                        : undefined
                    }
                    hasVideo={!!tile}
                    speaking={speakingUserIds.has(userId)}
                    muted={mutedUserIds.has(userId)}
                    onMute={() => muteAttendee(userId)}
                    onCameraOff={() => turnOffAttendeeCamera(userId)}
                  />
                );
              })}
              {contentTiles.map((tile) => (
                <MeetingTile
                  key={tile.tileId}
                  wide
                  name="Screen share"
                  videoRef={(el) => chime.bindRemoteVideo(tile.tileId, el)}
                  hasVideo
                  speaking={false}
                />
              ))}
            </div>
            <p className="mt-3 flex items-center gap-1.5 text-xs text-ink-muted">
              {(creating || roomStatus === "connecting") && <Spinner />}
              {creating && !myMeetingId
                ? "Starting meeting…"
                : (STATUS_LABEL[roomStatus] ?? "Waiting for others")}
            </p>
          </div>
        )}

        {showChat && (
          <MeetingChat
            messages={chatMessages}
            members={members}
            onSend={sendChatMessage}
            className={
              isMobile
                ? "flex-1"
                : `shrink-0 border-l ${compact ? "w-40" : "w-chat"}`
            }
          />
        )}
      </div>

      <div className="flex shrink-0 items-center justify-between gap-2 border-t bg-bg p-3">
        <MeetingControls
          cameraOn={cameraOn}
          micMuted={micMuted}
          sharingScreen={sharingScreen}
          canShareScreen={roomStatus === "connected"}
          onLeave={leave}
          onToggleCamera={toggleCamera}
          onToggleMic={toggleMic}
          onToggleScreenShare={toggleScreenShare}
        />
        <button
          onClick={() => setChatOpen((v) => !v)}
          title={chatOpen ? "Hide chat" : "Show chat"}
          className={`flex items-center rounded-card border p-2 text-lg leading-none ${chatOpen ? "text-brand" : "text-ink-muted hover:text-ink"}`}
        >
          <FiMessageCircle />
        </button>
      </div>
    </div>
  );
}
