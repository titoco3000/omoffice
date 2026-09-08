import { useCallback, useEffect, useState } from "react";
import { callApi, describeFailure } from "../api";

/// How often the presence half is re-read. Presence is not pushed: the
/// `presence` channel carries a browser's claim *to* the backend, and
/// nothing fans a roster change back out, so this polls the read route
/// (`presence::org_presence_route`) instead.
const PRESENCE_INTERVAL_MS = 10_000;

/**
 * One org's people and its live meetings, as the sidebar shows them.
 *
 * Two sources, merged: the membership roster (`GET /private/orgs/{id}`),
 * which changes rarely, and open sockets (`GET /private/orgs/{id}/presence`),
 * which change constantly. Someone in the second but not the first is still
 * listed — a socket reporting this org has already passed the backend's
 * membership check.
 */
export function useOrgRoster(token, orgId) {
  const [reloads, setReloads] = useState(0);
  // Both halves carry the fetch they answer, the same "stamp" pattern as
  // `useOrgs`: a result from the previous org is simply not the current
  // one, so nothing has to be cleared when the selection changes.
  const [roster, setRoster] = useState({ stamp: null, members: [], error: "" });
  const [presence, setPresence] = useState({ stamp: null, users: [] });

  const stamp = `${token}:${orgId}`;
  const refresh = useCallback(() => setReloads((count) => count + 1), []);

  useEffect(() => {
    if (!token || !orgId) return undefined;
    let cancelled = false;

    callApi({ path: `/private/orgs/${encodeURIComponent(orgId)}`, token }).then(
      (response) => {
        if (cancelled) return;
        setRoster({
          stamp: `${token}:${orgId}`,
          members: response.ok ? (response.data.members ?? []) : [],
          error: response.ok ? "" : describeFailure(response),
        });
      },
    );

    return () => {
      cancelled = true;
    };
  }, [token, orgId, reloads]);

  useEffect(() => {
    if (!token || !orgId) return undefined;
    let cancelled = false;

    async function poll() {
      // A hidden tab is not looking at the roster; skipping keeps a
      // background tab from polling all day.
      if (document.hidden) return;
      const response = await callApi({
        path: `/private/orgs/${encodeURIComponent(orgId)}/presence`,
        token,
      });
      if (cancelled || !response.ok) return;
      setPresence({ stamp: `${token}:${orgId}`, users: response.data.users ?? [] });
    }

    poll();
    const timer = setInterval(poll, PRESENCE_INTERVAL_MS);
    document.addEventListener("visibilitychange", poll);

    return () => {
      cancelled = true;
      clearInterval(timer);
      document.removeEventListener("visibilitychange", poll);
    };
  }, [token, orgId, reloads]);

  const shownRoster = roster.stamp === stamp ? roster : { members: [], error: "" };
  const shownPresence = presence.stamp === stamp ? presence.users : [];

  const online = new Map(shownPresence.map((user) => [user.email, user.meeting ?? null]));

  const emails = [
    ...new Set([...shownRoster.members.map((member) => member.email), ...online.keys()]),
  ];
  const people = emails
    .map((email) => ({
      email,
      online: online.has(email),
      meeting: online.get(email) ?? null,
    }))
    // Whoever is here first, then alphabetically — a roster that reorders
    // on every poll would be unusable.
    .sort((a, b) =>
      a.online === b.online ? a.email.localeCompare(b.email) : a.online ? -1 : 1,
    );

  // A meeting is "live" because somebody is in it, so this is the presence
  // rows grouped by code rather than a second request.
  const byCode = new Map();
  for (const [email, meeting] of online) {
    if (!meeting) continue;
    if (!byCode.has(meeting)) byCode.set(meeting, []);
    byCode.get(meeting).push(email);
  }
  const meetings = [...byCode.entries()]
    .map(([code, participants]) => ({ code, participants: participants.sort() }))
    .sort((a, b) => b.participants.length - a.participants.length);

  return { people, meetings, error: shownRoster.error, refresh };
}
