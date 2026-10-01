import { AdminGate, SysAdminsTable, OrgsTable } from "../features/admin";
import { Link } from "../core/Router";

export default function Admin() {
  return (
    <AdminGate>
      <h1 className="text-2xl font-semibold text-ink">Admin</h1>
      <SysAdminsTable />
      <OrgsTable />
      <Link to="/dev-costs" className="text-sm text-brand hover:underline">
        Dev costs
      </Link>
    </AdminGate>
  );
}
