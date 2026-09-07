import { useEffect, useState } from "react";
import { callApi, describeFailure } from "../api";
import { useOrgContext } from "../org/OrgContext";
import Meeting from "./Meeting";
import MeetingHistory from "./MeetingHistory";

/**
 * The lobby in front of `Meeting`: create or type a code, under whichever
 * org `OrgSwitcher` has selected. `joinCode` comes from `PendingInvites`.
 */
export default function MeetingPanel({ token, joinCode, onJoinCodeConsumed }) {
  const { selectedOrgId, selectedOrg } = useOrgContext();
  const [code, setCode] = useState(""); // the code being used, once joined
  const [codeInput, setCodeInput] = useState("");
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState("");
  // Adjusts state during render (react.dev's pattern), not an effect that
  // would set `code`/`codeInput` a render late.
  const [appliedJoinCode, setAppliedJoinCode] = useState(null);

  if (joinCode && joinCode !== appliedJoinCode) {
    setAppliedJoinCode(joinCode);
    setCodeInput(joinCode);
    setCode(joinCode);
  }

  // A real effect (reaches outside this component), keyed on
  // `appliedJoinCode` so it fires exactly once per code.
  useEffect(() => {
    if (appliedJoinCode) onJoinCodeConsumed?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [appliedJoinCode]);

  async function createMeeting() {
    if (!selectedOrgId) return;
    setError("");
    setCreating(true);
    const response = await callApi({
      method: "POST",
      path: "/private/meetings",
      token,
      body: { org_id: selectedOrgId },
    });
    setCreating(false);

    if (!response.ok) {
      setError(describeFailure(response));
      return;
    }
    setCodeInput(response.data.code);
    setCode(response.data.code);
  }

  function joinMeeting(event) {
    event.preventDefault();
    const wanted = codeInput.trim();
    if (!wanted) return;
    setError("");
    setCode(wanted);
  }

  if (code) {
    return (
      <section className="rounded-xl border border-slate-800 bg-slate-900/60 p-4">
        <div className="flex flex-wrap items-center gap-2">
          <h2 className="text-sm font-medium text-slate-300">meeting</h2>
          <span className="rounded bg-emerald-500/15 px-2 py-0.5 text-xs text-emerald-300">
            live
          </span>
          <span className="ml-auto font-mono text-xs break-all text-slate-500">
            {code}
          </span>
        </div>
        <div className="mt-3">
          {/* Keyed by code so a new meeting is a fresh session, not a reused one. */}
          <Meeting
            key={code}
            code={code}
            token={token}
            onLeave={() => setCode("")}
          />
        </div>
        <MeetingInviteForm code={code} token={token} />
      </section>
    );
  }

  return (
    <section className="rounded-xl border border-slate-800 bg-slate-900/60 p-4">
      <div className="flex flex-wrap items-center gap-2">
        <h2 className="text-sm font-medium text-slate-300">meeting</h2>
        <span
          className={`rounded px-2 py-0.5 text-xs ${
            selectedOrg ? "bg-sky-500/15 text-sky-300" : "bg-rose-500/15 text-rose-300"
          }`}
        >
          {selectedOrg ? selectedOrg.name : "no org selected"}
        </span>
      </div>

      <div className="mt-3 flex flex-wrap items-center gap-3">
        <button
          type="button"
          onClick={createMeeting}
          disabled={creating || !token || !selectedOrgId}
          title={selectedOrgId ? undefined : "pick an org above first"}
          className="rounded-lg bg-sky-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-sky-500 disabled:opacity-50"
        >
          {creating ? "..." : "create meeting"}
        </button>

        <form onSubmit={joinMeeting} className="flex flex-1 gap-2">
          <input
            type="text"
            value={codeInput}
            onChange={(event) => setCodeInput(event.target.value)}
            placeholder="meeting code"
            className="h-9 min-w-0 flex-1 rounded-lg border border-slate-700 bg-slate-950 px-2 font-mono text-sm text-slate-100 outline-none focus:border-sky-500"
          />
          <button
            type="submit"
            disabled={!token || !codeInput.trim()}
            className="rounded-lg bg-slate-700 px-4 py-1.5 text-sm font-medium text-white hover:bg-slate-600 disabled:opacity-50"
          >
            join meeting
          </button>
        </form>
      </div>

      {error && (
        <p className="mt-3 rounded-lg border border-rose-800 bg-slate-950 p-3 font-mono text-xs break-all text-rose-300">
          {error}
        </p>
      )}

      {/* Remounted every time the lobby comes back, so leaving a meeting is
          what refreshes the list. */}
      <MeetingHistory
        token={token}
        orgId={selectedOrgId}
        onPick={(picked) => {
          setCodeInput(picked);
          setCode(picked);
        }}
      />
    </section>
  );
}

/**
 * Invite someone by email. The backend gates this on being the meeting's
 * creator; this form just surfaces the 403 inline rather than guessing.
 */
function MeetingInviteForm({ code, token }) {
  const [email, setEmail] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [notice, setNotice] = useState("");

  async function submit(event) {
    event.preventDefault();
    const wanted = email.trim();
    if (!wanted) return;
    setNotice("");
    setSubmitting(true);
    const response = await callApi({
      method: "POST",
      path: `/private/meetings/${encodeURIComponent(code)}/invites`,
      token,
      body: { email: wanted },
    });
    setSubmitting(false);
    if (!response.ok) {
      setNotice(describeFailure(response));
      return;
    }
    setEmail("");
    setNotice(`invited ${wanted}`);
  }

  return (
    <form onSubmit={submit} className="mt-3 flex flex-wrap items-center gap-2">
      <input
        type="email"
        value={email}
        onChange={(event) => setEmail(event.target.value)}
        placeholder="invite@example.com"
        className="h-9 min-w-0 flex-1 rounded-lg border border-slate-700 bg-slate-950 px-2 font-mono text-sm text-slate-100 outline-none focus:border-sky-500"
      />
      <button
        type="submit"
        disabled={submitting || !email.trim()}
        className="shrink-0 rounded-lg bg-sky-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-sky-500 disabled:opacity-50"
      >
        {submitting ? "..." : "invite to meeting"}
      </button>
      {notice && (
        <p className="w-full font-mono text-xs break-all text-slate-400">{notice}</p>
      )}
    </form>
  );
}
