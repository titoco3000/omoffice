import { useEffect, useState } from "react";
import { useAuth } from "../../core/auth/AuthContext";
import { me } from "../org";

// Page shell for the secret sys-admin panel. The backend gates every
// `/admin/*` call on its own; this only decides whether to draw anything.
export default function AdminGate({ children }) {
  const { token } = useAuth();
  const [sysAdmin, setSysAdmin] = useState(null);

  useEffect(() => {
    me(token)
      .then((data) => setSysAdmin(Boolean(data.sys_admin)))
      .catch(() => setSysAdmin(false));
  }, [token]);

  if (sysAdmin === null) return null;
  if (!sysAdmin) {
    return <p className="p-10 text-center text-ink-muted">Not found.</p>;
  }
  return (
    <main className="mx-auto flex max-w-5xl flex-col gap-6 px-6 py-10">
      {children}
    </main>
  );
}
