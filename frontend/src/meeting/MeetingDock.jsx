//! Persistent across pages -- floats free and draggable on desktop (as
//! before), toggles between a half-screen dock and fullscreen on mobile
//! (toggled from `NavContainer`'s bottom bar, see `MeetingLayoutContext`).
//! Renders nothing while this tab isn't in a meeting.

import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useAuth } from "../auth/AuthContext";
import { decodeJwt } from "../lib/jwt";
import { useIsMobile, useIsPortrait } from "../layout/useViewport";
import { useOrgSession } from "../org/OrgSessionContext";
import { nearestSlot, slotPosition } from "./dockGeometry";
import { useMeetingLayout } from "./MeetingLayoutContext";
import MeetingPanel from "./MeetingPanel";

export default function MeetingDock() {
  const { token } = useAuth();
  const myId = decodeJwt(token)?.sub;
  const { session, roster } = useOrgSession();
  const { myMeetingId } = session;
  const isMobile = useIsMobile();
  const isPortrait = useIsPortrait();
  const { mobileExpanded } = useMeetingLayout();

  const [fullScreen, setFullScreen] = useState(false);
  const [position, setPosition] = useState("bot-right"); // last dropped slot, mini mode only
  const [drag, setDrag] = useState(null); // {pointerId, grabX, grabY, x, y} while dragging
  const [size, setSize] = useState({ width: 0, height: 0 });
  const panelRef = useRef(null);

  // See the equivalent effect in the pre-split `panels/MeetingsPanel.jsx`:
  // keyed on `myMeetingId` since `panelRef` is only attached once a meeting
  // starts.
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
  }, [myMeetingId, isMobile]);

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

  if (!myMeetingId) return null;

  if (isMobile) {
    // Both sizes leave room for `NavContainer`'s bottom bar (portrait) or
    // side bar (landscape) rather than covering it -- that bar is where the
    // toggle back out of fullscreen lives.
    const barGutter = isPortrait ? "bottom-14" : "right-14";
    const placement = mobileExpanded
      ? `inset-0 ${barGutter}`
      : isPortrait
        ? `top-0 ${barGutter} right-0 w-1/2`
        : `top-0 left-0 ${barGutter} h-1/2`;
    return (
      <section
        className={`fixed z-40 flex flex-col border bg-surface shadow-xl ${placement}`}
      >
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
          ? "fixed inset-4 z-50 flex flex-col rounded-card border bg-surface shadow-xl"
          : `fixed z-50 flex max-w-sm flex-col rounded-card border bg-surface shadow-xl ${
              drag ? "" : "transition-[top,left] duration-200"
            }`
      }
    >
      <header
        onPointerDown={startDrag}
        onPointerMove={dragMove}
        onPointerUp={dragEnd}
        className={`flex shrink-0 items-center justify-between rounded-t-card border-b bg-bg px-3 py-2 ${
          fullScreen ? "" : "cursor-move touch-none"
        }`}
      >
        <h2 className="text-sm font-semibold text-ink">Meeting</h2>
        <button
          onPointerDown={(event) => event.stopPropagation()}
          onClick={() => setFullScreen((v) => !v)}
          className="rounded-card border px-2 py-1 text-xs text-ink-muted hover:text-ink"
        >
          {fullScreen ? "Minimize" : "Full screen"}
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
