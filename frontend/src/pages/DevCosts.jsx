import { CostCalculator, CostResources } from "../features/cost-calculator";

// AWS cost model of the platform: a what-if calculator and the price list it
// runs on. Public (no login): it is client-side only and reads no user data.
export default function DevCosts() {
  return (
    <main className="mx-auto flex max-w-7xl flex-col gap-6 px-6 py-10">
      <h1 className="text-2xl font-semibold text-ink">Dev costs</h1>
      <CostCalculator />
      <CostResources />
    </main>
  );
}
