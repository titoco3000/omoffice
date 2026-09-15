//! Every pending org invite for this account, not just the one riding along
//! on `?invite=org:<id>` -- issued-before-signup and other-tab invites have
//! no such param, only `listMyInvites` covers them. Split out of
//! `components/Inbox.jsx` (now folded into `notifications/NotificationsOverlay.jsx`)
//! so it stays a plain data hook, reusable wherever org invites need
//! surfacing.

import { useEffect, useState } from "react";
import { getInvite, acceptInvite, declineInvite, listMyInvites } from "./rbac";

function inviteIdFromUrl() {
  const raw = new URLSearchParams(window.location.search).get("invite");
  if (!raw) return null;
  const [kind, id] = raw.split(":");
  return kind === "org" ? id : null;
}

function clearInviteParam() {
  const url = new URL(window.location.href);
  url.searchParams.delete("invite");
  window.history.replaceState({}, "", `${url.pathname}${url.search}`);
}

export function useOrgInvites(token) {
  const [urlInviteId] = useState(inviteIdFromUrl);
  const [urlInvite, setUrlInvite] = useState(null);
  const [urlDismissed, setUrlDismissed] = useState(false);
  const [invites, setInvites] = useState([]);
  const [busyId, setBusyId] = useState(null);
  const [error, setError] = useState(null);

  const reload = () =>
    listMyInvites(token)
      .then((data) => setInvites(data.invites ?? []))
      .catch((err) => setError(err.message));

  useEffect(() => {
    if (!urlInviteId) return;
    getInvite(token, urlInviteId)
      .then((data) => setUrlInvite(data.invite))
      .catch((err) => setError(err.message));
  }, [token, urlInviteId]);

  useEffect(() => {
    reload();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [token]);

  const resolve = async (inviteId, orgId, action, onDone) => {
    setBusyId(inviteId);
    try {
      await action(token, inviteId);
      if (inviteId === urlInviteId) {
        clearInviteParam();
        setUrlDismissed(true);
      }
      setInvites((current) => current.filter((i) => i.id !== inviteId));
      onDone(orgId);
    } catch (err) {
      setError(err.message);
    } finally {
      setBusyId(null);
    }
  };

  const accept = (inviteId, orgId, onAccepted) =>
    resolve(inviteId, orgId, acceptInvite, onAccepted);
  const decline = (inviteId, orgId) =>
    resolve(inviteId, orgId, declineInvite, () => {});

  const listHasUrlInvite =
    urlInviteId && invites.some((i) => i.id === urlInviteId);
  const showUrlCard =
    urlInviteId && !urlDismissed && !listHasUrlInvite && urlInvite;

  return {
    invites,
    urlInvite: showUrlCard ? urlInvite : null,
    urlInviteId,
    busyId,
    error,
    accept,
    decline,
  };
}
