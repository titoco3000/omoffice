import { AdminGate, SysAdminsTable, OrgsTable } from "../features/admin";

export default function Admin() {
  return (
    <AdminGate>
      <h1 className="text-2xl font-semibold text-ink">Admin</h1>
      <SysAdminsTable />
      <OrgsTable />
    </AdminGate>
  );
}
