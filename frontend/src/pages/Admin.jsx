import { AdminGate, SysAdminsTable, OrgsTable } from "../features/admin";
import { CostCalculator, CostResources } from "../features/cost-calculator";

export default function Admin() {
  return (
    <AdminGate>
      <h1 className="text-2xl font-semibold text-ink">Admin</h1>
      <SysAdminsTable />
      <OrgsTable />
      <CostCalculator />
      <CostResources />
    </AdminGate>
  );
}
