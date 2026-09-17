//! Persistent chrome for every authenticated page except `/` and `/login`:
//! a collapsible sidebar on desktop, a bottom (or side, landscape) bar on
//! mobile. `OrgOnlineView` lives in the desktop sidebar only, and is hidden
//! there while already showing on `/in` itself -- except in graphics mode
//! "map", where `/in` is just a canvas and the sidebar is its only home.

import { useState } from "react";
import { FiHome, FiMenu } from "react-icons/fi";
import { Link, useLocation } from "../router/Router";
import { useIsMobile, useIsPortrait } from "./useViewport";
import { useOrgSession } from "../org/OrgSessionContext";
import { useGraphics } from "../theme/GraphicsContext";
import { useMeetingLayout } from "../meeting/MeetingLayoutContext";
import MeetingDock from "../meeting/MeetingDock";
import OrgOnlineView from "../components/OrgOnlineView";
import Avatar from "../components/Avatar";
import NotificationsOverlay from "../notifications/NotificationsOverlay";
import { avatarUrl } from "../lib/avatars";

const STORAGE_KEY = "omoffice_sidebar_collapsed";

function Logo({ className = "text-lg" }) {
  return (
    <span className={`font-semibold text-ink ${className}`}>omoffice</span>
  );
}

function useMyDisplay() {
  const { myId, currentOrg, roster, profile } = useOrgSession();
  const member = currentOrg ? roster.members.find((m) => m.id === myId) : null;
  return {
    name: member?.name ?? profile?.name ?? null,
    picture: member?.picture ?? profile?.picture ?? null,
  };
}

function DesktopSidebar() {
  const path = useLocation();
  const { currentOrg } = useOrgSession();
  const { mode } = useGraphics();
  const me = useMyDisplay();
  const [collapsed, setCollapsed] = useState(
    () => localStorage.getItem(STORAGE_KEY) === "true",
  );

  const toggle = () => {
    setCollapsed((v) => {
      localStorage.setItem(STORAGE_KEY, String(!v));
      return !v;
    });
  };

  return (
    <aside
      className={`flex h-full shrink-0 flex-col border-r bg-surface transition-[width] duration-150 ${
        collapsed ? "w-rail" : "w-side"
      }`}
    >
      <div className="flex shrink-0 items-center justify-between border-b p-3">
        {!collapsed && <Logo />}
        <button
          onClick={toggle}
          title={collapsed ? "Expand" : "Collapse"}
          className="rounded-card px-2 py-1 text-ink-muted hover:bg-muted hover:text-ink"
        >
          {collapsed ? "»" : "«"}
        </button>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto p-2">
        {currentOrg && (path !== "/in" || mode === "map") && (
          <>
            <div className="flex flex-1 items-center justify-center">
              <Link
                to="/in"
                title="Home"
                className="flex size-10 items-center justify-center rounded-card text-xl text-ink-muted hover:bg-muted hover:text-ink"
              >
                <FiHome />
              </Link>
            </div>
            <OrgOnlineView />
          </>
        )}
      </div>

      <Link
        to="/profile"
        className="flex shrink-0 items-center gap-2 border-t p-3 hover:bg-muted"
      >
        <Avatar
          url={avatarUrl(me.picture)}
          name={me.name || "?"}
          size="size-8"
        />
        {!collapsed && (
          <span className="truncate text-sm text-ink">
            {me.name || "Profile"}
          </span>
        )}
      </Link>
    </aside>
  );
}

function MobileBar() {
  const isPortrait = useIsPortrait();
  const { session } = useOrgSession();
  const me = useMyDisplay();
  const { mobileExpanded, toggleMobileExpanded } = useMeetingLayout();
  const inMeeting = !!session.myMeetingId;

  return (
    <nav
      className={`flex shrink-0 items-center justify-around border bg-surface ${
        isPortrait ? "h-14 flex-row" : "w-14 flex-col"
      }`}
    >
      <Link
        to="/profile"
        title={me.name || "Profile"}
        className="flex flex-col items-center justify-center text-ink-muted"
      >
        <Avatar
          url={avatarUrl(me.picture)}
          name={me.name || "?"}
          size="size-6"
        />
      </Link>
      <button
        onClick={inMeeting ? toggleMobileExpanded : undefined}
        className="flex flex-col items-center justify-center text-ink-muted"
        title={
          inMeeting
            ? mobileExpanded
              ? "Shrink meeting"
              : "Expand meeting"
            : undefined
        }
      >
        <Logo className="text-sm" />
      </button>
      <Link
        to="/in"
        className="flex flex-col items-center justify-center text-ink-muted"
      >
        <FiMenu className="text-lg" />
      </Link>
    </nav>
  );
}

export default function NavContainer({ children }) {
  const isMobile = useIsMobile();
  const isPortrait = useIsPortrait();
  const { session } = useOrgSession();
  const { mobileExpanded } = useMeetingLayout();
  const inMeeting = !!session.myMeetingId;

  if (isMobile) {
    // Hidden (not unmounted) rather than removed, so expanding the dock
    // doesn't lose whatever state the page underneath was holding.
    const main = (
      <main
        className={`min-h-0 min-w-0 flex-1 overflow-y-auto ${
          inMeeting && mobileExpanded ? "hidden" : ""
        }`}
      >
        {children}
      </main>
    );
    const dock = inMeeting ? <MeetingDock /> : null;

    return (
      <>
        <div className={`flex h-full ${isPortrait ? "flex-col" : "flex-row"}`}>
          <div
            className={`flex min-h-0 flex-1 ${isPortrait ? "flex-col" : "flex-row"}`}
          >
            {dock}
            {main}
          </div>
          <MobileBar />
        </div>
        <NotificationsOverlay />
      </>
    );
  }

  return (
    <>
      <div className="flex h-full">
        <DesktopSidebar />
        <main className="min-h-0 flex-1 overflow-y-auto">{children}</main>
        <MeetingDock />
      </div>
      <NotificationsOverlay />
    </>
  );
}
