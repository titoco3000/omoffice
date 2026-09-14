// Lives in the normal page flow (unlike `MeetingsPanel`'s fixed dock, which
// only shows up once this tab is actually in a meeting) -- just the "start
// one" trigger. Seeing/joining everyone else's meeting happens in
// `PeoplePanel` instead, which already lists online members grouped by
// whichever meeting they're in.
export default function MeetingsListPanel({ session }) {
  const { myMeetingId, error, create } = session;

  return (
    <section className="rounded-card border bg-surface p-5">
      <div className="flex items-center justify-between">
        <h2 className="text-lg font-semibold text-ink">Meetings</h2>
        <button
          onClick={() => create()}
          disabled={!!myMeetingId}
          className="rounded-card bg-brand px-3 py-1.5 text-sm font-medium text-on-brand hover:bg-brand-strong disabled:opacity-50"
        >
          New meeting
        </button>
      </div>

      {error && <p className="mt-3 text-sm text-danger">{error}</p>}
    </section>
  );
}
