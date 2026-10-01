import { useMemo, useState } from "react";
import { FiTrash2 } from "react-icons/fi";
import ValueCompositionGraph from "../../core/ui/ValueCompositionGraph";
import { estimate } from "./costModel";

// Inputs are edited in display units (percent for shares) and converted to
// the model's 0..1 in `toUsage`.
const FIELDS = [
  { key: "members", label: "Members" },
  { key: "work_hours_per_month", label: "Work hours / month" },
  { key: "meeting_time_share", label: "Time in meetings (%)", percent: true },
  { key: "avg_meeting_size", label: "Avg meeting size" },
  { key: "chat_messages_per_meeting", label: "Chat messages / meeting" },
  {
    key: "second_monitor_share",
    label: "Always open on 2nd monitor (%)",
    percent: true,
  },
  { key: "walking_share", label: "Time walking (%)", percent: true },
];

const DEFAULTS = {
  members: 50,
  work_hours_per_month: 176,
  meeting_time_share: 30,
  avg_meeting_size: 3,
  chat_messages_per_meeting: 10,
  second_monitor_share: 100,
  walking_share: 2,
};

const toUsage = (scenario) =>
  Object.fromEntries(
    FIELDS.map(({ key, percent }) => {
      const value = Number(scenario[key]);
      return [key, percent ? value / 100 : value];
    }),
  );

const usd = (value, digits = 2) =>
  value.toLocaleString("en-US", {
    style: "currency",
    currency: "USD",
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  });

const OUTPUTS = [
  { label: "Total / month", get: (r) => usd(r.total), strong: true },
  { label: "Fixed", get: (r) => usd(r.fixed) },
  { label: "Variable", get: (r) => usd(r.variable) },
  { label: "Per person", get: (r) => usd(r.per_person, 3), strong: true },
  { label: "Chime", get: (r) => usd(r.chime) },
  {
    label: "Per person excl. Chime",
    get: (r) => usd(r.per_person_excl_chime, 3),
  },
];

// The graph's parts: the big services get their own fixed colour, the small
// ones fold into "Other" (a categorical palette has eight slots at most).
const GRAPH_SERVICES = [
  "Chime SDK",
  "AppSync Events",
  "API Gateway",
  "Lambda",
  "Aurora DSQL",
  "CloudFront",
  "CloudWatch Logs",
];
const GRAPH_COLORS = {
  ...Object.fromEntries(
    GRAPH_SERVICES.map((service, i) => [service, `var(--om-cat-${i + 1})`]),
  ),
  Other: "var(--om-ink-faint)",
};

const composition = (byService, divisor) => {
  const parts = {};
  for (const { service, usd } of byService) {
    const part = GRAPH_SERVICES.includes(service) ? service : "Other";
    parts[part] = (parts[part] ?? 0) + usd / divisor;
  }
  return parts;
};

const MAX_ORGS = 5;
const orgName = (index) => `Org ${String.fromCharCode(65 + index)}`;

const GRAPH_MODES = [
  ["total", "Total"],
  ["per_user", "Per user"],
];

let nextId = 1;
const newScenario = (base = DEFAULTS) => ({ ...base, id: nextId++ });

// Side-by-side usage variations of a fictional org; each column is priced by
// `costModel.estimate`.
export default function CostCalculator() {
  const [scenarios, setScenarios] = useState(() => [newScenario()]);
  const [graphMode, setGraphMode] = useState("per_user");
  // scenario id -> {data} | {error}
  const results = useMemo(
    () =>
      Object.fromEntries(
        scenarios.map((scenario) => {
          try {
            return [scenario.id, { data: estimate(toUsage(scenario)) }];
          } catch (err) {
            return [scenario.id, { error: err.message }];
          }
        }),
      ),
    [scenarios],
  );

  const perUser = graphMode === "per_user";
  const graphItems = scenarios.flatMap((s, i) =>
    results[s.id].data
      ? [
          {
            label: orgName(i),
            composition: composition(
              results[s.id].data.by_service,
              perUser ? Number(s.members) : 1,
            ),
          },
        ]
      : [],
  );

  const update = (id, key, value) =>
    setScenarios((list) =>
      list.map((s) => (s.id === id ? { ...s, [key]: value } : s)),
    );
  const add = () =>
    setScenarios((list) => [...list, newScenario(list[list.length - 1])]);
  const removeLast = () => setScenarios((list) => list.slice(0, -1));

  const services = [];
  scenarios.forEach((s) =>
    results[s.id]?.data?.by_service.forEach(({ service }) => {
      if (!services.includes(service)) services.push(service);
    }),
  );

  const cell = (scenario, render) => {
    const result = results[scenario.id];
    if (result.error) return <span className="text-danger">--</span>;
    return render(result.data);
  };

  return (
    <section className="rounded-card border bg-surface p-5">
      <div className="mb-2 flex items-center justify-between">
        <h2 className="text-sm text-ink-muted">Cost calculator</h2>
        <button
          onClick={add}
          disabled={scenarios.length >= MAX_ORGS}
          className="rounded-card bg-brand px-3 py-1 text-sm font-medium text-on-brand hover:bg-brand-strong disabled:opacity-50"
        >
          Add org
        </button>
      </div>
      <div className="overflow-x-auto">
        <table className="w-full text-sm">
          <thead>
            <tr className="text-left text-ink-muted">
              <th className="py-1 pr-3 font-medium" />
              {scenarios.map((s, i) => (
                <th key={s.id} className="px-1 py-1 font-medium">
                  <span>{orgName(i)}</span>
                  {scenarios.length > 1 && i === scenarios.length - 1 && (
                    <button
                      onClick={removeLast}
                      aria-label={`Remove ${orgName(i)}`}
                      className="ml-2 align-middle text-danger hover:opacity-70"
                    >
                      <FiTrash2 />
                    </button>
                  )}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {FIELDS.map(({ key, label }) => (
              <tr key={key} className="border-t">
                <td className="py-1 pr-3 text-ink-muted">{label}</td>
                {scenarios.map((s) => (
                  <td key={s.id} className="px-1 py-1">
                    <input
                      type="number"
                      min="0"
                      step="any"
                      value={s[key]}
                      onChange={(event) =>
                        update(s.id, key, event.target.value)
                      }
                      className="w-24 rounded-card border bg-bg px-2 py-1 text-ink"
                    />
                  </td>
                ))}
              </tr>
            ))}
            {OUTPUTS.map(({ label, get, strong }, i) => (
              <tr key={label} className={i === 0 ? "border-t-2" : "border-t"}>
                <td className="py-1.5 pr-3 text-ink-muted">{label}</td>
                {scenarios.map((s) => (
                  <td
                    key={s.id}
                    className={`px-1 py-1.5 ${strong ? "font-medium text-ink" : "text-ink"}`}
                  >
                    {cell(s, get)}
                  </td>
                ))}
              </tr>
            ))}
            {services.map((service, i) => (
              <tr key={service} className={i === 0 ? "border-t-2" : "border-t"}>
                <td className="py-1.5 pr-3 text-ink-muted">{service}</td>
                {scenarios.map((s) => (
                  <td key={s.id} className="px-1 py-1.5 text-ink">
                    {cell(s, (r) =>
                      usd(
                        r.by_service.find((x) => x.service === service)?.usd ??
                          0,
                      ),
                    )}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <div className="mb-1 mt-5 flex items-center justify-between">
        <h3 className="text-sm text-ink-muted">
          Monthly cost by service{perUser ? ", per user in the org" : ""}
        </h3>
        <div className="inline-flex rounded-card border p-0.5">
          {GRAPH_MODES.map(([mode, label]) => (
            <button
              key={mode}
              onClick={() => setGraphMode(mode)}
              className={`rounded-[calc(var(--radius-card)-0.2rem)] px-2.5 py-1 text-xs font-medium ${
                graphMode === mode
                  ? "bg-brand text-on-brand"
                  : "text-ink-muted hover:text-ink"
              }`}
            >
              {label}
            </button>
          ))}
        </div>
      </div>
      <ValueCompositionGraph
        items={graphItems}
        colors={GRAPH_COLORS}
        formatValue={(v) => usd(v, perUser ? 3 : 2)}
      />
      {scenarios.map(
        (s, i) =>
          results[s.id]?.error && (
            <p key={s.id} className="mt-2 text-sm text-danger">
              {orgName(i)}: {results[s.id].error}
            </p>
          ),
      )}
    </section>
  );
}
