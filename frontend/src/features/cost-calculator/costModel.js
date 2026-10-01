// Cost model for running Omoffice on AWS: a fictional org's usage in, a
// monthly USD breakdown out. Port of `dev-docs/cost-breakdown.csv`; keep the
// two in step. Prices live in `costResources.csv`, tunables in
// `costAssumptions.js`. Free tiers are deliberately not applied (worst case
// at list price).
//
// The one departure from the sheet: its "share of org online" constant is
// replaced by `second_monitor_share`. People who leave Omoffice open on a
// second monitor are online (websocket + map connection) for all their work
// hours; everyone else only while in meetings. At a share of 1 the result
// matches the sheet's worst case.

import RESOURCES from "./costResources.csv";
import { DEFAULT_ASSUMPTIONS } from "./costAssumptions";

export { RESOURCES };

const PRICES = Object.fromEntries(RESOURCES.map((r) => [r.id, r]));

// Shares are 0..1.
export const DEFAULT_USAGE = {
  members: 50,
  work_hours_per_month: 176,
  meeting_time_share: 0.3,
  avg_meeting_size: 3,
  chat_messages_per_meeting: 10,
  second_monitor_share: 1,
  walking_share: 0.02,
};

const isShare = (v) => v >= 0 && v <= 1;

function validate(u, a) {
  if (!(u.members >= 1)) return "members must be at least 1";
  if (!(u.work_hours_per_month > 0)) return "work hours must be positive";
  if (!(u.avg_meeting_size >= 1)) return "avg meeting size must be at least 1";
  if (!(u.chat_messages_per_meeting >= 0)) {
    return "chat messages per meeting must not be negative";
  }
  if (
    !isShare(u.meeting_time_share) ||
    !isShare(u.second_monitor_share) ||
    !isShare(u.walking_share)
  ) {
    return "percentages must be between 0 and 100";
  }
  if (!(
    a.persist_interval_s > 0 &&
    a.meeting_length_min > 0 &&
    a.token_lifetime_min > 0 &&
    a.payload_kb > 0
  )) {
    return "persist interval, meeting length, token lifetime and payload must be positive";
  }
  return null;
}

// -> {total, fixed, variable, per_person, chime, per_person_excl_chime,
//     by_service: [{service, usd}], lines: [{id, service, name, quantity,
//     unit, usd, fixed}]}, USD per month. Throws on invalid input.
export function estimate(usage, overrides = {}) {
  const u = { ...DEFAULT_USAGE, ...usage };
  const a = { ...DEFAULT_ASSUMPTIONS, ...overrides };
  const invalid = validate(u, a);
  if (invalid) throw new Error(invalid);

  const g = u.members;
  const h = u.work_hours_per_month;
  const size = u.avg_meeting_size;

  // Fraction of work time a member is connected.
  const online =
    u.second_monitor_share +
    (1 - u.second_monitor_share) * u.meeting_time_share;
  const onlineUserHours = g * online * h;
  const onlineMinPerUser = h * 60 * online;
  const viewers = Math.max(0, g * online - 1);
  const walkSeconds = onlineUserHours * 3600 * u.walking_share;

  const finePublished = walkSeconds * a.fine_moves_per_s;
  const fineSubs = viewers * a.fine_audience_share * (1 - a.zoomed_out_share);
  const fineDelivered = finePublished * fineSubs;
  const coarsePublished = walkSeconds * a.coarse_moves_per_s;
  const coarseDelivered = coarsePublished * viewers * a.zoomed_out_share;
  const persisted = walkSeconds / a.persist_interval_s;
  const tokenRefreshes = (g * onlineMinPerUser) / a.token_lifetime_min;

  const attendeeMin = g * h * 60 * u.meeting_time_share;
  const meetings = attendeeMin / size / a.meeting_length_min;
  const chatSent = meetings * u.chat_messages_per_meeting;
  const chatDelivered = chatSent * (size - 1);

  const http =
    persisted * (1 + a.proximity_calls_per_persist) +
    tokenRefreshes +
    onlineUserHours * a.other_requests_per_online_hour +
    meetings;
  const wsConnMin = g * onlineMinPerUser;
  const wsMsgs = chatSent + chatDelivered;
  const wsLambda = chatSent + (2 * g * h) / 8;
  const apiInvocations = http + wsLambda;
  const authInvocations = tokenRefreshes * a.authorizer_calls_per_token;
  const dbRoundTrips = http + chatSent + wsLambda;

  const pageLoads = ((g * h) / 8) * a.page_loads_per_workday;
  const cfGb = (pageLoads * (a.bundle_mb + a.media_mb_per_load)) / 1024;
  const cfRequests = pageLoads * a.cf_requests_per_load;
  const mediaGb = (g * a.media_mb_per_user) / 1024;
  const logGb =
    ((apiInvocations + authInvocations) * a.log_kb_per_invocation +
      http * a.log_kb_per_http_request) /
    1024 /
    1024;

  const appsyncOps =
    ((finePublished * 2 +
      fineDelivered +
      coarsePublished * 2 +
      coarseDelivered) *
      Math.ceil(a.payload_kb / 5)) /
      1e6 +
    (tokenRefreshes * (1 + a.subscriptions_per_connect)) / 1e6;

  // Billable quantity per resource, in that resource's own `unit`.
  const quantities = {
    "secrets-secret": a.secrets_count,
    "secrets-api": a.secrets_api_calls / 10_000,
    "appsync-ops": appsyncOps,
    "appsync-conn": wsConnMin / 1e6,
    "apigw-http": http / 1e6,
    "apigw-ws-msgs": (wsMsgs * Math.ceil(a.payload_kb / 32)) / 1e6,
    "apigw-ws-conn": wsConnMin / 1e6,
    chime: attendeeMin,
    "lambda-api-req": apiInvocations / 1e6,
    "lambda-api-dur":
      ((apiInvocations * a.api_lambda_duration_ms) / 1000) *
      a.api_lambda_memory_gb,
    "lambda-auth-req": authInvocations / 1e6,
    "lambda-auth-dur":
      ((authInvocations * a.authorizer_duration_ms) / 1000) *
      a.authorizer_memory_gb,
    // 4.35 weeks x 4 steps x 2 s x 128 MB
    "lambda-rotator": 4.35 * 4 * 2 * 0.128,
    "dsql-dpu": (dbRoundTrips * a.dsql_dpu_per_round_trip) / 1e6,
    "dsql-storage": a.dsql_storage_gb,
    eventbridge: meetings / 1e6,
    "cf-data": cfGb,
    "cf-requests": cfRequests / 10_000,
    "s3-storage": mediaGb,
    "s3-get": (cfRequests * (1 - a.cf_cache_hit_ratio)) / 1000,
    "logs-ingest": logGb,
    "logs-storage": (logGb * a.log_retention_days) / 30,
  };

  const lines = Object.entries(quantities).map(([id, quantity]) => {
    const r = PRICES[id];
    return {
      id,
      service: r.service,
      name: r.name,
      quantity,
      unit: r.unit,
      usd: r.price_usd * quantity,
      fixed: r.fixed,
    };
  });

  const sum = (ls) => ls.reduce((acc, l) => acc + l.usd, 0);
  const total = sum(lines);
  const fixed = sum(lines.filter((l) => l.fixed));
  const chime = sum(lines.filter((l) => l.id === "chime"));

  const byService = new Map();
  for (const l of lines) {
    byService.set(l.service, (byService.get(l.service) ?? 0) + l.usd);
  }

  return {
    total,
    fixed,
    variable: total - fixed,
    per_person: total / g,
    chime,
    per_person_excl_chime: (total - chime) / g,
    by_service: [...byService].map(([service, usd]) => ({ service, usd })),
    lines,
  };
}
