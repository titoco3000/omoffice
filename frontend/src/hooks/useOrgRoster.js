import { useEffect, useState, useCallback } from "react";
import { callApi, describeFailure } from "../api";
import { useChannel } from "../messenger/MessengerContext";

/**
 * One org's people and its live meetings, as the sidebar shows them.
 *
 * Two sources, merged: the membership roster (`GET /private/orgs/{id}`),
 * which changes rarely, and open sockets (presence), which change
 * constantly. Someone in the second but not the first is still listed — a
 * socket reporting this org has already passed the backend's membership
 * check.
 *
 * Presence itself is pushed, not polled: `presence::register` broadcasts an
 * org's snapshot on `org_presence` the moment a join, a leave or a
 * disconnect changes it, to everyone already looking at that org (see the
 * backend module doc). The one `GET /private/orgs/{id}/presence` call left
 * here is a tab's own first read -- there is nothing to push to a tab that
 * has not opened its socket yet.
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

    callApi({ path: `/private/orgs/${encodeURIComponent(orgId)}/presence`, token }).then(
      (response) => {
        if (cancelled || !response.ok) return;
        setPresence({ stamp: `${token}:${orgId}`, users: response.data.users ?? [] });
      },
    );

    return () => {
      cancelled = true;
    };
  }, [token, orgId, reloads]);

  // The live half, after that first read: one socket for the whole app, so
  // this filters to the org actually on screen rather than the backend
  // scoping the channel.
  useChannel("org_presence", (message) => {
    if (message?.org_id !== orgId) return;
    setPresence({ stamp: `${token}:${orgId}`, users: message.users ?? [] });
  });

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
    // on every update would be unusable.
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
