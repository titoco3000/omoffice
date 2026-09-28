//! Persistent across pages -- floats free and draggable on desktop (as
//! before). On mobile it's a plain flex sibling instead (see `NavContainer`,
//! which places and sizes it -- half or fullscreen, per `MeetingLayoutContext`
//! -- by allotting it flex space rather than overlaying it); this component
//! only renders its content there. Renders nothing while this tab isn't in
//! a meeting -- except the instant after "New meeting" is clicked, where it
//! opens on `creating` alone, before there's an id to be "in" yet.

import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { FiMaximize, FiMinimize } from "react-icons/fi";
import { useAuth } from "../../../core/auth/AuthContext";
import { decodeJwt } from "../../../core/jwt";
import { useIsMobile } from "../../../core/hooks/useViewport";
import { useOrgSession } from "../../org";
import { useMeetingSessionContext } from "../MeetingSessionContext";
import { nearestSlot, slotPosition } from "./dockGeometry";
import MeetingPanel from "../panel/MeetingPanel";

export default function MeetingDock() {
  const { token } = useAuth();
  const myId = decodeJwt(token)?.sub;
  const { roster } = useOrgSession();
  const session = useMeetingSessionContext();
  const { myMeetingId, creating } = session;
  const isMobile = useIsMobile();
  // Shows the dock the instant "New meeting" is clicked, before the
  // backend hands back a real id -- `MeetingPanel` renders a "Starting
  // meeting…" placeholder for this window.
  const open = !!myMeetingId || creating;

  const [fullScreen, setFullScreen] = useState(false);
  const [position, setPosition] = useState("bot-right"); // last dropped slot, mini mode only
  const [drag, setDrag] = useState(null); // {pointerId, grabX, grabY, x, y} while dragging
  const [size, setSize] = useState({ width: 0, height: 0 });
  const panelRef = useRef(null);

  // See the equivalent effect in the pre-split `panels/MeetingsPanel.jsx`:
  // keyed on `open` since `panelRef` is only attached once the dock
  // renders (a meeting joined, or the optimistic "starting" placeholder).
  useLayoutEffect(() => {
    if (isMobile) return;
    const element = panelRef.current;
    if (!element) return;
    const observer = new ResizeObserver(([entry]) => {
      const box = entry.contentBoxSize?.[0];
      setSize(
        box
          ? { width: box.inlineSize, height: box.blockSize }
          : {
              width: entry.contentRect.width,
              height: entry.contentRect.height,
            },
      );
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [open, isMobile]);

  const [, forceRecompute] = useState(0);
  useEffect(() => {
    const onResize = () => forceRecompute((n) => n + 1);
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  const startDrag = (event) => {
    if (fullScreen) return;
    const rect = panelRef.current.getBoundingClientRect();
    event.currentTarget.setPointerCapture(event.pointerId);
    setDrag({
      pointerId: event.pointerId,
      grabX: event.clientX - rect.left,
      grabY: event.clientY - rect.top,
      x: rect.left,
      y: rect.top,
    });
  };

  const dragMove = (event) => {
    if (!drag || event.pointerId !== drag.pointerId) return;
    setDrag((d) => ({
      ...d,
      x: event.clientX - d.grabX,
      y: event.clientY - d.grabY,
    }));
  };

  const dragEnd = (event) => {
    if (!drag || event.pointerId !== drag.pointerId) return;
    setPosition(nearestSlot(drag.x + size.width / 2, drag.y + size.height / 2));
    setDrag(null);
  };

  if (!open) return null;

  if (isMobile) {
    return (
      <section className="flex min-h-0 min-w-0 flex-1 flex-col border bg-surface">
        <MeetingPanel
          session={session}
          members={roster.members}
          myId={myId}
          isMobile
        />
      </section>
    );
  }

  const dockedStyle = fullScreen
    ? undefined
    : drag
      ? { top: drag.y, left: drag.x }
      : { ...slotPosition(position, size) };

  return (
    <section
      ref={panelRef}
      style={dockedStyle}
      className={
        fullScreen
          ? "fixed inset-0 z-50 flex flex-col bg-surface shadow-xl"
          : `fixed z-50 flex h-[min(300px,70%)] w-[min(500px,70%)] flex-col rounded-card border bg-surface shadow-xl ${
              drag ? "" : "transition-[top,left] duration-200"
            }`
      }
    >
      <header
        onPointerDown={startDrag}
        onPointerMove={dragMove}
        onPointerUp={dragEnd}
        className={`flex shrink-0 items-center justify-between border-b bg-bg px-3 py-2 ${
          fullScreen ? "" : "cursor-move touch-none rounded-t-card"
        }`}
      >
        <h2 className="text-sm font-semibold text-ink">Meeting</h2>
        <button
          onPointerDown={(event) => event.stopPropagation()}
          onClick={() => setFullScreen((v) => !v)}
          className="rounded-card border px-2 py-1 text-xs text-ink-muted hover:text-ink"
        >
          {fullScreen ? <FiMinimize /> : <FiMaximize />}
        </button>
      </header>
      <MeetingPanel
        session={session}
        members={roster.members}
        myId={myId}
        isMobile={false}
        compact={!fullScreen}
      />
    </section>
  );
}
