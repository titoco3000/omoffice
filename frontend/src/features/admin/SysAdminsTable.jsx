import { useEffect, useRef, useState } from "react";
import { useAuth } from "../../core/auth/AuthContext";
import AsyncButton from "../../core/ui/AsyncButton";
import { addSysAdmin, listSysAdmins, removeSysAdmin } from "./api";

export default function SysAdminsTable() {
  const { token } = useAuth();
  const [admins, setAdmins] = useState([]);
  const [email, setEmail] = useState("");
  const [error, setError] = useState(null);
  // Enter-to-submit routes through the AsyncButton -- same trick as
  // `features/org/UsersManagement.jsx`.
  const addButtonRef = useRef(null);

  useEffect(() => {
    listSysAdmins(token)
      .then((data) => setAdmins(data.sys_admins))
      .catch((err) => setError(err.message));
  }, [token]);

  const run = async (call) => {
    try {
      setError(null);
      const data = await call();
      setAdmins(data.sys_admins);
      return true;
    } catch (err) {
      setError(err.message);
      return false;
    }
  };

  const add = async () => {
    if (await run(() => addSysAdmin(token, email.trim()))) setEmail("");
  };

  const remove = async (admin) => {
    const label = admin.email ?? admin.user_id;
    if (!window.confirm(`Remove ${label} as sys-admin?`)) return;
    await run(() => removeSysAdmin(token, admin.user_id));
  };

  return (
    <section className="rounded-card border bg-surface p-5">
      <h2 className="mb-2 text-sm text-ink-muted">Sys-admins</h2>
      <table className="w-full text-sm">
        <thead>
          <tr className="text-left text-ink-muted">
            <th className="py-1 font-medium">Email</th>
            <th className="py-1 font-medium">Name</th>
            <th className="py-1" />
          </tr>
        </thead>
        <tbody>
          {admins.map((admin) => (
            <tr key={admin.user_id} className="border-t">
              <td className="py-1.5 text-ink">{admin.email ?? "--"}</td>
              <td className="py-1.5 text-ink-muted">{admin.name ?? "--"}</td>
              <td className="py-1.5 text-right">
                <AsyncButton
                  onClick={() => remove(admin)}
                  className="text-danger hover:underline"
                >
                  remove
                </AsyncButton>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          addButtonRef.current?.click();
        }}
        className="mt-4 flex items-center gap-2"
      >
        <input
          type="email"
          required
          value={email}
          onChange={(event) => setEmail(event.target.value)}
          placeholder="admin@example.com"
          className="flex-1 rounded-card border bg-bg px-3 py-1.5 text-ink"
        />
        <AsyncButton
          ref={addButtonRef}
          onClick={add}
          className="rounded-card bg-brand px-3 py-1.5 text-sm font-medium text-on-brand hover:bg-brand-strong"
        >
          Add sys-admin
        </AsyncButton>
      </form>
      {error && <p className="mt-2 text-sm text-danger">{error}</p>}
    </section>
  );
}
