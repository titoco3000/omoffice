import { useEffect, useState } from "react";
import { useAuth } from "../auth/AuthContext";
import Meeting from "../components/Meeting";
import { useNavigate, usePath } from "../router";
import { useActiveMeeting } from "./MeetingContext";

/**
 * The joined meeting, mounted once for the whole app.
 *
 * It is position-fixed and never unmounts, which is the point: navigating
 * away from `/meeting/{code}` must not tear the Chime session down. When
 * `MeetingPage` registers its stage the dock is measured onto it; with no
 * stage on screen it shrinks to a corner rectangle instead.
 */
export default function MeetingDock() {
  const { token } = useAuth();
  const { code, leave, slot } = useActiveMeeting();
  const rect = useElementRect(slot);
  const navigate = useNavigate();
  const path = usePath();

  if (!code) return null;

  // The address bar names the meeting; leaving it must clear the bar too,
  // or `/meeting/{code}` reads as "landed here, join" the moment the next
  // effect runs. Only steer away when leave was actually called from that
  // page — the mini dock can be left from anywhere else without moving you.
  function onLeave() {
    if (path === `/meeting/${encodeURIComponent(code)}`) navigate("/users");
    leave();
  }

  // Measured rather than reusing the layout's width classes: the stage is
  // whatever is left after the nav and the chat sidebar, and only the
  // element itself knows that once both have had their say.
  const docked = Boolean(rect);
  const style = docked
    ? { top: rect.top, left: rect.left, width: rect.width, height: rect.height }
    : undefined;

  return (
    <div
      className={`fixed z-30 ${
        docked
          ? ""
          : "right-3 bottom-20 aspect-video w-56 sm:w-72 md:bottom-3 md:left-auto"
      }`}
      style={style}
    >
      {/* Keyed by code so a different meeting is a fresh session. */}
      <Meeting
        key={code}
        code={code}
        token={token}
        onLeave={onLeave}
        mini={!docked}
        onExpand={() => navigate(`/meeting/${encodeURIComponent(code)}`)}
      />
    </div>
  );
}

/// The element's viewport rectangle, kept current while it is on screen.
/// `null` when there is no element — the dock's "no stage" state.
function useElementRect(element) {
  const [measured, setMeasured] = useState({ element: null, rect: null });

  useEffect(() => {
    if (!element) return undefined;

    const update = () =>
      setMeasured({ element, rect: element.getBoundingClientRect() });

    // The stage resizes when the sidebar collapses or the chat opens; the
    // root is watched too, so a layout change that only moves the stage
    // sideways is caught as well. Observing also delivers the first
    // measurement, which is why nothing measures the element by hand.
    const observer = new ResizeObserver(update);
    observer.observe(element);
    observer.observe(document.documentElement);
    window.addEventListener("resize", update);
    window.addEventListener("scroll", update, true);

    return () => {
      observer.disconnect();
      window.removeEventListener("resize", update);
      window.removeEventListener("scroll", update, true);
    };
  }, [element]);

  // A measurement belongs to the element it was taken from: a page that has
  // unregistered its stage must not keep the last rectangle of it.
  return measured.element === element ? measured.rect : null;
}
