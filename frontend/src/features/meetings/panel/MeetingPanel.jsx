//! The dock's content: one tile per attendee, a chat column/view, and the
//! controls row (leave/camera/mic/share on the left, the chat toggle on the
//! right). `MeetingDock` decides where and how big this sits on screen;
//! this component only knows how to fill whatever space it's given.

import { useState } from "react";
import { FiMessageCircle } from "react-icons/fi";
import * as chime from "../chime";
import { preferredName, preferredPicture } from "../../org";
import { avatarUrl } from "../../avatars";
import Spinner from "../../../core/ui/Spinner";
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
  const [pinnedId, setPinnedId] = useState(null);
  // The content tile id last auto-pinned, so a fresh share only defaults
  // to pinned once -- see the render-time update below.
  const [lastAutoContentId, setLastAutoContentId] = useState(null);
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
    toggleLock,
  } = session;

  const locked = !!meetings.find((m) => m.id === myMeetingId)?.invite_only;

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

  // One shape for every tile regardless of kind (self/other/screen share)
  // -- lets pin, the gallery grid and the pinned/sidebar split treat them
  // uniformly instead of duplicating that logic per kind.
  const tiles = [
    {
      id: "self",
      key: "self",
      props: {
        name: "You",
        initial: preferredName(myId, members),
        isSelf: true,
        videoRef: selfVideoRef,
        hasVideo: cameraOn,
        picture: avatarUrl(preferredPicture(myId, members)),
        speaking: speakingUserIds.has(myId),
        muted: micMuted,
      },
    },
    ...others.map((userId) => {
      const tile = cameraTileByUser.get(userId);
      return {
        id: userId,
        key: userId,
        props: {
          name: preferredName(userId, members),
          picture: avatarUrl(preferredPicture(userId, members)),
          videoRef: tile
            ? (el) => chime.bindRemoteVideo(tile.tileId, el)
            : undefined,
          hasVideo: !!tile,
          speaking: speakingUserIds.has(userId),
          muted: mutedUserIds.has(userId),
          onMute: () => muteAttendee(userId),
          onCameraOff: () => turnOffAttendeeCamera(userId),
        },
      };
    }),
    ...contentTiles.map((tile) => ({
      id: `content:${tile.tileId}`,
      key: tile.tileId,
      props: {
        name: "Screen share",
        videoRef: (el) => chime.bindRemoteVideo(tile.tileId, el),
        hasVideo: true,
        speaking: false,
      },
    })),
  ];

  // A new screen share is pinned by default the moment it starts, but only
  // that once -- a click on any tile's pin button afterward can override
  // it, same as it can override any other pin. Comparing against
  // `lastAutoContentId` (rather than reacting to `contentTiles` itself)
  // is what makes this "default", not "forced": it fires once per share
  // starting, so it doesn't keep stomping on a manual pin/unpin made while
  // that same share is still going. Updating state mid-render like this
  // (not in a `useEffect`) is the React-endorsed way to derive state from
  // a prop/computed value -- it bails out before committing, so it's one
  // render, not a cascade.
  const firstContentId = contentTiles[0]?.tileId ?? null;
  if (firstContentId !== lastAutoContentId) {
    setLastAutoContentId(firstContentId);
    if (firstContentId != null) setPinnedId(`content:${firstContentId}`);
  }

  const togglePin = (id) => setPinnedId((prev) => (prev === id ? null : id));

  const pinnedTile = tiles.find((t) => t.id === pinnedId);
  const sidebarTiles = pinnedTile ? tiles.filter((t) => t.id !== pinnedId) : [];
  const galleryCols = Math.max(1, Math.ceil(Math.sqrt(tiles.length)));

  const showTiles = !isMobile || !chatOpen;
  const showChat = chatOpen;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <audio ref={audioRef} autoPlay hidden />

      <div className="flex min-h-0 flex-1">
        {showTiles && (
          <div className="flex min-h-0 min-w-0 flex-1 flex-col p-3">
            <div className="flex min-h-0 flex-1 gap-2">
              {pinnedTile ? (
                <>
                  <div className="min-w-0 flex-1">
                    <MeetingTile
                      key={pinnedTile.key}
                      fill
                      pinned
                      onPin={() => togglePin(pinnedTile.id)}
                      {...pinnedTile.props}
                    />
                  </div>
                  <div className="flex w-40 shrink-0 flex-col gap-2 overflow-y-auto sm:w-48">
                    {sidebarTiles.map((t) => (
                      <MeetingTile
                        key={t.key}
                        onPin={() => togglePin(t.id)}
                        {...t.props}
                      />
                    ))}
                  </div>
                </>
              ) : (
                <div
                  className="grid h-full w-full flex-1 auto-rows-fr gap-2"
                  style={{
                    gridTemplateColumns: `repeat(${galleryCols}, minmax(0, 1fr))`,
                  }}
                >
                  {tiles.map((t) => (
                    <MeetingTile
                      key={t.key}
                      fill
                      onPin={() => togglePin(t.id)}
                      {...t.props}
                    />
                  ))}
                </div>
              )}
            </div>
            <p className="mt-3 flex shrink-0 items-center gap-1.5 text-xs text-ink-muted">
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
          locked={locked}
          canShareScreen={roomStatus === "connected"}
          onLeave={leave}
          onToggleCamera={toggleCamera}
          onToggleMic={toggleMic}
          onToggleScreenShare={toggleScreenShare}
          onToggleLock={toggleLock}
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
