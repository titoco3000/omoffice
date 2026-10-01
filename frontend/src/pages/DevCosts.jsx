import { AdminGate } from "../features/admin";
import { CostCalculator, CostResources } from "../features/cost-calculator";

// AWS cost model of the platform: a what-if calculator and the price list it
// runs on. Behind the sys-admin gate like the rest of the operator pages.
export default function DevCosts() {
  return (
    <AdminGate wide>
      <h1 className="text-2xl font-semibold text-ink">Dev costs</h1>
      <CostCalculator />
      <CostResources />
    </AdminGate>
  );
}
