//! Walking into someone on the office map starts or joins a meeting with
//! them; walking away from everyone in it leaves. Only this tab's own moves
//! count -- someone else walking up is their tab's to act on, not this
//! one's, so two tabs never both react to the same encounter.
//!
//! "Nearby" is anyone online within `PROXIMITY_RANGE` cells in both
//! directions (a square around this tab's cell). Worked out here rather
//! than on the backend, since the live positions are here: the stored ones
//! can lag up to a second behind (see `useMapSession`'s `persist`).
//!
//! Part of `useMapSession`, which is what calls it. What being near
//! someone *means* -- a new meeting, pulling them into this tab's, or
//! moving this tab's whole meeting into theirs -- is the backend's call
//! (`features::map::proximity`), made against the
//! meetings as they actually stand rather than this tab's copy of the list.
//! The local list decides whether asking is worth a request at all, and
//! what the answer will most likely be: that guess is shown straight away
//! (`expectJoin`) rather than after the few seconds of room/Chime setup
//! the real answer takes. Whoever this tab is about to pull in gets told
//! just as early by the backend (`MAP_MEETING_PENDING_CHANNEL`).
//!
//! Leaving is the one rule decided here: it needs to know who was nearby a
//! step ago, which only this tab does. A meeting joined from far away (the
//! roster's join button) isn't left just because its attendees are out of
//! range -- only by walking away from an attendee who was in range.

import { useEffect, useRef } from "react";
import { messenger } from "./messenger";
import {
  MAP_MEETING_PENDING_CHANNEL,
  PROXIMITY_RANGE,
  gatherNearby,
} from "./map";

// Where `features::map::proximity::gather` will most likely put this tab,
// from the local meeting list: the unlocked meeting (not this tab's own)
// the most `near` users are in, ties to the smallest id, same as the
// backend -- else this tab's own meeting (`mine`), else `null` for a new
// one.
function predictTarget(meetings, near, mine) {
  let best = null;
  let bestCount = 0;
  for (const meeting of meetings) {
    if (meeting.invite_only || meeting.id === mine) continue;
    const count = meeting.attendees.filter((id) => near.has(id)).length;
    if (
      count > bestCount ||
      (count > 0 && count === bestCount && meeting.id < best)
    ) {
      best = meeting.id;
      bestCount = count;
    }
  }
  return best ?? mine;
}

export function useMapProximity(
  token,
  orgId,
  positions,
  loaded,
  members,
  myId,
  session,
) {
  const me = positions[myId];

  // Read by `evaluate` below, which runs later than the render that
  // scheduled it (after an in-flight request settles) and should see
  // whatever is current by then.
  const latestRef = useRef(null);
  useEffect(() => {
    latestRef.current = { token, orgId, positions, members, myId, session };
  });

  // Who was nearby as of the last evaluation -- `null` until the first
  // one, which only records it: loading the map next to someone isn't
  // walking up to them.
  const previousNearRef = useRef(null);
  const busyRef = useRef(false);
  const pendingRef = useRef(false);

  useEffect(() => {
    if (!loaded || !me) return;

    const evaluate = async () => {
      if (busyRef.current) {
        pendingRef.current = true;
        return;
      }
      const { token, orgId, positions, members, myId, session } =
        latestRef.current;
      const here = positions[myId];
      const near = new Set(
        members
          .filter((member) => member.id !== myId)
          .filter((member) => {
            const there = positions[member.id];
            return (
              there &&
              Math.max(
                Math.abs(there.x - here.x),
                Math.abs(there.y - here.y),
              ) <= PROXIMITY_RANGE
            );
          })
          .map((member) => member.id),
      );
      const previous = previousNearRef.current;
      previousNearRef.current = near;
      if (!previous) return;

      const { meetings, myMeetingId, leave, expectJoin, followJoin } = session;
      const mine = meetings.find((m) => m.id === myMeetingId);
      const companions = (mine?.attendees ?? []).filter((id) => id !== myId);
      const walkedAway =
        companions.some((id) => previous.has(id)) &&
        !companions.some((id) => near.has(id));

      // Someone nearby who isn't already with this tab, and who could
      // actually be joined -- a locked meeting of someone else's is never
      // a target, see `features::map::proximity`.
      const lockedOthers = new Set(
        meetings
          .filter((m) => m.invite_only && m.id !== myMeetingId)
          .flatMap((m) => m.attendees),
      );
      const newcomers = [...near].filter(
        (id) => !companions.includes(id) && !lockedOthers.has(id),
      );
      if (!walkedAway && newcomers.length === 0) return;

      busyRef.current = true;
      // `leave` drops this tab out locally before its request even goes
      // out, so the expected meeting below can be shown right after it --
      // but the backend has to have actually processed the leave before
      // `gatherNearby`, or it would drag the old meeting along.
      const leaving = walkedAway ? leave() : null;
      const proposedId = crypto.randomUUID();
      if (newcomers.length > 0) {
        const target = predictTarget(
          meetings,
          near,
          walkedAway ? null : myMeetingId,
        );
        expectJoin(target ?? proposedId);
      }
      try {
        await leaving;
        if (newcomers.length > 0)
          await followJoin(
            await gatherNearby(token, orgId, [...near], proposedId),
          );
      } catch {
        // Same as a failed move: nothing to show, the next step asks again.
        await followJoin(null);
      } finally {
        busyRef.current = false;
      }
      if (pendingRef.current) {
        pendingRef.current = false;
        evaluate();
      }
    };

    evaluate();
    // Only this tab's own cell changing is a move -- see the module doc.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loaded, me?.x, me?.y]);

  // Someone else walked up to this user -- show the meeting they're about
  // to be pulled into now, not once the forced join itself lands.
  useEffect(() => {
    if (!orgId) return;
    return messenger.on(MAP_MEETING_PENDING_CHANNEL, (message) => {
      if (message.org_id !== orgId) return;
      latestRef.current?.session.expectJoin(message.meeting_id);
    });
  }, [orgId]);

  // A different org's map is a different set of people to have been near.
  useEffect(() => {
    if (!loaded) previousNearRef.current = null;
  }, [loaded]);
}
