import { useEffect, useState } from "react";
import { callApi, describeFailure } from "../api";

/**
 * The orgs the current token's email belongs to, from `GET /private/orgs` --
 * each with the caller's own roles already folded in, which is what
 * `rbac.hasRole` reads. Same "stamp" pattern as `MeetingHistory`: `loading`
 * is derived from whether the held result matches the current
 * `token:reloads` pair, rather than tracked alongside it.
 */
export function useOrgs(token) {
  const [reloads, setReloads] = useState(0);
  const [result, setResult] = useState({ stamp: null, orgs: [], error: "" });

  const stamp = `${token}:${reloads}`;
  const loading = Boolean(token) && result.stamp !== stamp;

  useEffect(() => {
    if (!token) return undefined;
    let cancelled = false;

    callApi({ path: "/private/orgs", token }).then((response) => {
      if (cancelled) return;
      setResult({
        stamp: `${token}:${reloads}`,
        orgs: response.ok ? (response.data.orgs ?? []) : [],
        error: response.ok ? "" : describeFailure(response),
      });
    });

    return () => {
      cancelled = true;
    };
  }, [token, reloads]);

  const shown = result.stamp === stamp ? result : { orgs: [], error: "" };

  return {
    orgs: shown.orgs,
    error: shown.error,
    loading,
    reload: () => setReloads((count) => count + 1),
  };
}
