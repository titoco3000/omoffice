import { ASSUMPTIONS } from "./costAssumptions";
import { RESOURCES } from "./costModel";

const formatPrice = (usd) =>
  usd >= 0.01 ? `$${usd}` : `$${Number(usd.toPrecision(4))}`;

// Every AWS price the cost model bills (`costResources.csv`) and the
// assumptions it runs on (`costAssumptions.js`).
export default function CostResources() {
  return (
    <section className="rounded-card border bg-surface p-5">
      <h2 className="mb-2 text-sm text-ink-muted">AWS resources</h2>
      {
        <>
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="text-left text-ink-muted">
                  <th className="py-1 pr-3 font-medium">Service</th>
                  <th className="py-1 pr-3 font-medium">Resource</th>
                  <th className="py-1 pr-3 font-medium">Unit</th>
                  <th className="py-1 pr-3 text-right font-medium">Price</th>
                  <th className="py-1 font-medium">Notes</th>
                </tr>
              </thead>
              <tbody>
                {RESOURCES.map((r) => (
                  <tr key={r.id} className="border-t align-top">
                    <td className="py-1.5 pr-3 text-ink">{r.service}</td>
                    <td className="py-1.5 pr-3 text-ink">
                      <a
                        href={r.source_url}
                        target="_blank"
                        rel="noreferrer"
                        className="text-brand hover:underline"
                      >
                        {r.name}
                      </a>
                      {r.fixed && (
                        <span className="ml-1.5 text-xs text-ink-muted">
                          fixed
                        </span>
                      )}
                    </td>
                    <td className="py-1.5 pr-3 text-ink-muted">{r.unit}</td>
                    <td className="py-1.5 pr-3 text-right text-ink">
                      {formatPrice(r.price_usd)}
                    </td>
                    <td className="py-1.5 text-ink-muted">{r.note}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <details className="mt-4">
            <summary className="cursor-pointer text-sm text-ink-muted">
              Model assumptions
            </summary>
            <table className="mt-2 w-full text-sm">
              <tbody>
                {ASSUMPTIONS.map((a) => (
                  <tr key={a.id} className="border-t align-top">
                    <td className="py-1.5 pr-3 text-ink">{a.label}</td>
                    <td className="py-1.5 pr-3 text-right text-ink">
                      {a.value} {a.unit}
                    </td>
                    <td className="py-1.5 text-ink-muted">{a.note}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </details>
        </>
      }
    </section>
  );
}
