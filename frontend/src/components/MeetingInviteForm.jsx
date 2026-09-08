import { useState } from "react";
import { callApi, describeFailure } from "../api";
import Button from "../ui/Button";
import Input from "../ui/Input";
import Notice from "../ui/Notice";

/**
 * Invite someone into a meeting by email. The backend gates this on being
 * the meeting's creator; this form just surfaces the 403 inline rather than
 * guessing who is allowed.
 */
export default function MeetingInviteForm({ code, token, onDone }) {
  const [email, setEmail] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [notice, setNotice] = useState("");
  const [failed, setFailed] = useState(false);

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
    setFailed(!response.ok);
    if (!response.ok) {
      setNotice(describeFailure(response));
      return;
    }
    setEmail("");
    setNotice(`invited ${wanted}`);
    onDone?.();
  }

  return (
    <form onSubmit={submit} className="grid gap-2">
      <div className="flex items-center gap-2">
        <Input
          type="email"
          mono
          value={email}
          onChange={(event) => setEmail(event.target.value)}
          placeholder="invite@example.com"
          className="flex-1"
        />
        <Button type="submit" size="sm" disabled={submitting || !email.trim()}>
          {submitting ? "..." : "invite"}
        </Button>
      </div>
      {notice && <Notice tone={failed ? "danger" : "info"}>{notice}</Notice>}
    </form>
  );
}
