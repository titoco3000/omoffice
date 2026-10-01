import { useEffect, useState } from "react";
import { useAuth } from "../../core/auth/AuthContext";
import { Link } from "../../core/Router";
import Spinner from "../../core/ui/Spinner";
import { listAdminOrgs } from "./api";

export default function OrgsTable() {
  const { token } = useAuth();
  const [orgs, setOrgs] = useState(null);
  const [error, setError] = useState(null);

  useEffect(() => {
    listAdminOrgs(token)
      .then((data) => setOrgs(data.orgs))
      .catch((err) => setError(err.message));
  }, [token]);

  return (
    <section className="rounded-card border bg-surface p-5">
      <h2 className="mb-2 text-sm text-ink-muted">Organizations</h2>
      {!orgs && !error && <Spinner className="size-4" />}
      {orgs && (
        <table className="w-full text-sm">
          <thead>
            <tr className="text-left text-ink-muted">
              <th className="py-1 font-medium">Organization</th>
              <th className="py-1 text-right font-medium">Users</th>
              <th className="py-1 text-right font-medium">Online</th>
            </tr>
          </thead>
          <tbody>
            {orgs.map((org) => (
              <tr key={org.id} className="border-t">
                <td className="py-1.5">
                  <Link
                    to={`/admin/org/${org.id}`}
                    className="text-brand hover:underline"
                  >
                    {org.name}
                  </Link>
                </td>
                <td className="py-1.5 text-right text-ink">{org.user_count}</td>
                <td className="py-1.5 text-right text-ink">
                  {org.online_count}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {error && <p className="text-sm text-danger">{error}</p>}
    </section>
  );
}
