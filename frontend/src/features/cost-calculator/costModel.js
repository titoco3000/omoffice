// Cost model for running Omoffice on AWS: a fictional org's usage in, a
// monthly USD breakdown out. Port of `dev-docs/cost-breakdown.csv`; keep the
// two in step. Prices live in `costResources.csv`, tunables in
// `costAssumptions.js`. Free tiers are deliberately not applied (worst case
// at list price).
//
// Usage is per member: work days and hours set the month, meetings per day
// and their duration set meeting time (Chime attendee-minutes), and two
// shares decide who is connected how: "overwatchers" keep the office tab
// visible all day (online for all their work hours; everyone else only
// during meetings), "resource-conscious" members run it without the map
// (no map connection, walking or map traffic, just the messenger socket).
// With 5.0769 days x 8 h (176 h a month), 4.8 meetings of 30 min, 100%
// overwatchers and 0% resource-conscious the result is the sheet's worst case.

import RESOURCES from "./costResources.csv";
import { DEFAULT_ASSUMPTIONS } from "./costAssumptions";

export { RESOURCES };

const PRICES = Object.fromEntries(RESOURCES.map((r) => [r.id, r]));

const WEEKS_PER_MONTH = 52 / 12;

// Shares are 0..1.
export const DEFAULT_USAGE = {
  members: 10,
  work_days_per_week: 5,
  work_hours_per_day: 8,
  meeting_minutes: 30,
  meetings_per_day: 3,
  chat_messages_per_meeting: 10,
  // Members who keep the office tab visible all day.
  overwatcher_share: 0.2,
  walking_share: 0.02,
  // Members who run the office without the map (no map connection, no
  // walking, no map traffic).
  resource_conscious_share: 0,
};

const isShare = (v) => v >= 0 && v <= 1;

function validate(u, a) {
  if (!(u.members >= 1)) return "members must be at least 1";
  if (!(u.work_days_per_week > 0 && u.work_days_per_week <= 7)) {
    return "work days per week must be between 0 and 7";
  }
  if (!(u.work_hours_per_day > 0 && u.work_hours_per_day <= 24)) {
    return "work hours must be between 0 and 24";
  }
  if (!(u.meeting_minutes > 0)) return "meeting duration must be positive";
  if (!(u.meetings_per_day >= 0))
    return "meetings per day must not be negative";
  if ((u.meetings_per_day * u.meeting_minutes) / 60 > u.work_hours_per_day) {
    return "meetings take longer than the work day";
  }
  if (!(u.chat_messages_per_meeting >= 0)) {
    return "chat messages per meeting must not be negative";
  }
  if (
    !isShare(u.overwatcher_share) ||
    !isShare(u.walking_share) ||
    !isShare(u.resource_conscious_share)
  ) {
    return "percentages must be between 0 and 100";
  }
  if (!(
    a.persist_interval_s > 0 &&
    a.avg_meeting_size >= 1 &&
    a.token_lifetime_min > 0 &&
    a.payload_kb > 0
  )) {
    return "persist interval, meeting size, token lifetime and payload must be sensible";
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
  const workdays = u.work_days_per_week * WEEKS_PER_MONTH;
  const h = workdays * u.work_hours_per_day;
  const size = a.avg_meeting_size;
  const meetingShare =
    (u.meetings_per_day * u.meeting_minutes) / 60 / u.work_hours_per_day;

  // Fraction of work time a member is connected: all of it for an
  // overwatcher, only their meetings for everyone else.
  const online = u.overwatcher_share + (1 - u.overwatcher_share) * meetingShare;
  const onlineUserHours = g * online * h;
  const onlineMinPerUser = h * 60 * online;
  // Members with the map open; the rest have only the messenger socket.
  const mapUsers = g * (1 - u.resource_conscious_share);
  const mapMinPerUser = onlineMinPerUser;
  const viewers = Math.max(0, mapUsers * online - 1);
  const walkSeconds = mapUsers * online * h * 3600 * u.walking_share;

  const finePublished = walkSeconds * a.fine_moves_per_s;
  const fineSubs = viewers * a.fine_audience_share * (1 - a.zoomed_out_share);
  const fineDelivered = finePublished * fineSubs;
  const coarsePublished = walkSeconds * a.coarse_moves_per_s;
  const coarseDelivered = coarsePublished * viewers * a.zoomed_out_share;
  const persisted = walkSeconds / a.persist_interval_s;
  const tokenRefreshes = (mapUsers * mapMinPerUser) / a.token_lifetime_min;

  const attendeeMin = g * workdays * u.meetings_per_day * u.meeting_minutes;
  const meetings = attendeeMin / size / u.meeting_minutes;
  const chatSent = meetings * u.chat_messages_per_meeting;
  const chatDelivered = chatSent * (size - 1);

  const http =
    persisted * (1 + a.proximity_calls_per_persist) +
    tokenRefreshes +
    onlineUserHours * a.other_requests_per_online_hour +
    meetings;
  const wsConnMin = g * onlineMinPerUser;
  const appsyncConnMin = mapUsers * mapMinPerUser;
  const wsMsgs = chatSent + chatDelivered;
  const wsLambda = chatSent + 2 * g * workdays;
  const apiInvocations = http + wsLambda;
  const authInvocations = tokenRefreshes * a.authorizer_calls_per_token;
  const dbRoundTrips = http + chatSent + wsLambda;

  const pageLoads = g * workdays * a.page_loads_per_workday;
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
    "appsync-conn": appsyncConnMin / 1e6,
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
