import { useEffect, useRef, useState } from "react";
import { useAuth } from "../auth/AuthContext";
import ChatPanel from "../components/ChatPanel";
import MeetingInviteForm from "../components/MeetingInviteForm";
import PageHeader from "../layout/PageHeader";
import { useActiveMeeting } from "../meeting/MeetingContext";
import Button from "../ui/Button";
import Dropdown from "../ui/Dropdown";
import { IconChat, IconClose } from "../ui/icons";

/**
 * `/meeting/{code}` — the stage and the chat beside it.
 *
 * The meeting itself is not rendered here: `MeetingDock` owns that session
 * for the whole app, and this page only hands it the rectangle to fill
 * (`setSlot`). That is what lets you walk to another page mid-call.
 */
export default function MeetingPage({ code }) {
  const { token } = useAuth();
  const { code: activeCode, join, setSlot } = useActiveMeeting();
  const [chatOpen, setChatOpen] = useState(false);

  // The URL is the instruction: landing here on a code we are not in — a
  // shared link, a reload, a rejoin — joins it. Read through a ref rather
  // than depending on `activeCode` directly: leaving the meeting on this
  // very page clears it too, and that must not read as "landed here, join".
  const activeCodeRef = useRef(activeCode);
  activeCodeRef.current = activeCode;

  useEffect(() => {
    if (code && code !== activeCodeRef.current) join(code);
  }, [code, join]);

  // Handing the dock `null` on the way out is what shrinks it to the corner.
  useEffect(() => () => setSlot(null), [setSlot]);

  return (
    <>
      <PageHeader
        title="meeting"
        subtitle={code}
        actions={
          <>
            <Dropdown
              align="right"
              width="w-72"
              trigger={({ toggle }) => (
                <Button variant="outline" size="sm" onClick={toggle}>
                  invite
                </Button>
              )}
            >
              {(close) => (
                <div className="p-2">
                  <MeetingInviteForm code={code} token={token} onDone={close} />
                </div>
              )}
            </Dropdown>
            <Button
              variant="ghost"
              size="icon-sm"
              className="md:hidden"
              aria-label={chatOpen ? "close chat" : "open chat"}
              onClick={() => setChatOpen((current) => !current)}
            >
              {chatOpen ? <IconClose className="h-5 w-5" /> : <IconChat className="h-5 w-5" />}
            </Button>
          </>
        }
      />

      <div className="relative flex min-h-0 flex-1">
        {/* The dock measures this element and covers it. It stays empty. */}
        <div ref={setSlot} className="min-h-0 min-w-0 flex-1 p-2" />

        {/* Desktop: a second sidebar. Phone: the same panel, over the video. */}
        <aside className="hidden w-chat shrink-0 border-l border-line bg-surface md:flex md:flex-col">
          <ChatPanel />
        </aside>

        {chatOpen && (
          <aside className="absolute inset-0 z-40 flex flex-col bg-surface md:hidden">
            <ChatPanel onClose={() => setChatOpen(false)} />
          </aside>
        )}
      </div>
    </>
  );
}
