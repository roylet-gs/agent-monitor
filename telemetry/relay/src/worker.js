// The usage-report relay: a Cloudflare Worker between `am` and Grafana Cloud.
//
// `am` posts OTLP/JSON logs (built by crates/am-core/src/telemetry/schema.rs)
// to POST /v1/logs. The relay never trusts them: every record is rebuilt here
// from the same allowlist the client uses, and anything else is dropped. It
// adds Grafana Cloud's credentials (Worker secrets, never in the binary),
// never forwards the sender's IP, caps the request size, and rate-limits per
// IP when the RATE_LIMITER binding is configured.
//
// Answers follow what the client does with them (upload.rs keeps a batch on
// any non-2xx and retries it): records that don't fit the schema are
// accepted and dropped (200 with OTLP `partialSuccess`), so a bad batch is
// never retried forever; only a failure worth retrying (rate limit, Grafana
// down) is 429/503.
//
// Timestamps: all of am's records land in one Loki stream
// ({service_name="am"}), and Loki refuses entries far older than the newest
// in a stream (about an hour). A client sends what it spooled while offline or
// backing off, so each record's time is clamped to [arrival − 30 min,
// arrival + 5 min]; the time the client recorded is kept as `recorded_ms`.
//
// Environment: GRAFANA_OTLP_ENDPOINT (var, e.g.
// https://otlp-gateway-prod-eu-west-2.grafana.net/otlp), GRAFANA_INSTANCE_ID
// and GRAFANA_API_TOKEN (secrets; the token needs only `logs:write`).

export const MAX_BODY_BYTES = 256 * 1024;
export const MAX_RECORDS = 1000;
export const MAX_AGE_NS = 30n * 60n * 1_000_000_000n;
export const MAX_AHEAD_NS = 5n * 60n * 1_000_000_000n;

const EVENTS = new Set(["am.error", "am.crash", "am.command", "am.features", "am.heartbeat", "am.perf"]);
const RESOURCE_KEYS = new Set([
  "service.name",
  "service.version",
  "am.binary",
  "os.type",
  "host.arch",
  "am.install_method",
  "am.install_id",
]);
const BINARIES = new Set(["am", "am-tui", "am-hook"]);
const SEVERITIES = new Map([
  [9, "INFO"],
  [13, "WARN"],
  [17, "ERROR"],
  [21, "FATAL"],
]);

// The client's rules (schema.rs `token`, `valid_key`, `source_path`).
const TOKEN = /^[A-Za-z0-9_.:-]{1,64}$/;
const KEY = /^[A-Za-z][A-Za-z0-9_.]{0,63}$/;
const SOURCE_PATH = /^crates\/[A-Za-z0-9_.\/-]{1,110}\.rs$/;

export function validValue(key, value) {
  if (typeof value !== "string") return false;
  if (key === "code.filepath") return SOURCE_PATH.test(value) && !value.includes("..");
  return TOKEN.test(value);
}

/** One OTLP/JSON attribute, rebuilt; `null` when it doesn't fit. */
export function cleanAttribute(attr) {
  if (!attr || typeof attr.key !== "string" || !KEY.test(attr.key) || !attr.value) return null;
  const v = attr.value;
  if ("stringValue" in v) {
    return validValue(attr.key, v.stringValue) ? { key: attr.key, value: { stringValue: v.stringValue } } : null;
  }
  if ("intValue" in v) {
    const n = String(v.intValue);
    return /^[0-9]{1,20}$/.test(n) ? { key: attr.key, value: { intValue: n } } : null;
  }
  if ("boolValue" in v) {
    return typeof v.boolValue === "boolean" ? { key: attr.key, value: { boolValue: v.boolValue } } : null;
  }
  return null;
}

function cleanResource(resource) {
  const attrs = [];
  for (const a of resource?.attributes ?? []) {
    if (!RESOURCE_KEYS.has(a?.key)) continue;
    const clean = cleanAttribute(a);
    if (!clean) return null; // a resource we can't vouch for drops its records
    attrs.push(clean);
  }
  const get = (k) => attrs.find((a) => a.key === k)?.value.stringValue;
  if (get("service.name") !== "am" || !BINARIES.has(get("am.binary")) || !get("service.version")) return null;
  return { attributes: attrs };
}

/** A log record, rebuilt, its time clamped around `nowNs`; `null` when it doesn't fit. */
export function cleanRecord(rec, nowNs = BigInt(Date.now()) * 1_000_000n) {
  const name = rec?.eventName ?? rec?.body?.stringValue;
  if (!EVENTS.has(name)) return null;
  const nanos = String(rec.timeUnixNano ?? "");
  if (!/^[0-9]{1,20}$/.test(nanos)) return null;
  const recorded = BigInt(nanos);
  const lo = nowNs - MAX_AGE_NS;
  const hi = nowNs + MAX_AHEAD_NS;
  const time = recorded < lo ? lo : recorded > hi ? hi : recorded;
  const severityNumber = SEVERITIES.has(rec.severityNumber) ? rec.severityNumber : 9;
  const attributes = [];
  for (const a of rec.attributes ?? []) {
    const clean = cleanAttribute(a);
    if (!clean) return null; // the client never sends a bad one: drop the record
    attributes.push(clean);
  }
  if (attributes.length > 200) return null;
  // `event.name` too, so Loki can filter on it whatever it does with `eventName`.
  attributes.push({ key: "event.name", value: { stringValue: name } });
  attributes.push({ key: "recorded_ms", value: { intValue: String(recorded / 1_000_000n) } });
  return {
    timeUnixNano: String(time),
    observedTimeUnixNano: String(nowNs),
    severityNumber,
    severityText: SEVERITIES.get(severityNumber),
    eventName: name,
    body: { stringValue: name },
    attributes,
  };
}

/** The request body Grafana gets, and how many records were dropped. */
export function sanitize(body, nowNs = BigInt(Date.now()) * 1_000_000n) {
  let kept = 0;
  let dropped = 0;
  const resourceLogs = [];
  for (const rl of Array.isArray(body?.resourceLogs) ? body.resourceLogs : []) {
    const records = (rl?.scopeLogs ?? []).flatMap((s) => (Array.isArray(s?.logRecords) ? s.logRecords : []));
    const resource = cleanResource(rl?.resource);
    if (!resource) {
      dropped += records.length;
      continue;
    }
    const clean = [];
    for (const r of records) {
      const c = kept < MAX_RECORDS ? cleanRecord(r, nowNs) : null;
      if (c) {
        clean.push(c);
        kept++;
      } else {
        dropped++;
      }
    }
    if (clean.length) resourceLogs.push({ resource, scopeLogs: [{ scope: { name: "am" }, logRecords: clean }] });
  }
  return { body: { resourceLogs }, kept, dropped };
}

const json = (status, obj) =>
  new Response(JSON.stringify(obj), { status, headers: { "content-type": "application/json" } });

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (url.pathname !== "/v1/logs") return json(404, { error: "not found" });
    if (request.method !== "POST") return json(405, { error: "POST only" });
    if (!(request.headers.get("content-type") ?? "").startsWith("application/json")) {
      return json(415, { error: "application/json only" });
    }
    if (Number(request.headers.get("content-length") ?? 0) > MAX_BODY_BYTES) return json(413, { error: "too large" });

    // Rate limit by IP; the IP goes no further than this line.
    if (env.RATE_LIMITER) {
      const { success } = await env.RATE_LIMITER.limit({ key: request.headers.get("cf-connecting-ip") ?? "unknown" });
      if (!success) return json(429, { error: "slow down" });
    }

    const text = await request.text();
    if (text.length > MAX_BODY_BYTES) return json(413, { error: "too large" });
    let parsed;
    try {
      parsed = JSON.parse(text);
    } catch {
      // Not ours to retry: accept and drop.
      return json(200, { partialSuccess: { rejectedLogRecords: "0", errorMessage: "not JSON" } });
    }
    const { body, kept, dropped } = sanitize(parsed);
    const partial = { partialSuccess: { rejectedLogRecords: String(dropped) } };
    if (kept === 0) return json(200, partial);

    const auth = btoa(`${env.GRAFANA_INSTANCE_ID}:${env.GRAFANA_API_TOKEN}`);
    let upstream;
    try {
      upstream = await fetch(`${env.GRAFANA_OTLP_ENDPOINT.replace(/\/$/, "")}/v1/logs`, {
        method: "POST",
        headers: { "content-type": "application/json", authorization: `Basic ${auth}` },
        body: JSON.stringify(body),
      });
    } catch {
      return json(503, { error: "upstream unreachable" });
    }
    if (upstream.ok) return json(200, partial);
    // Grafana throttling or down, or refusing the relay's own credentials (a
    // relay misconfiguration, not bad data): the client keeps the batch and retries.
    if ([401, 403, 429].includes(upstream.status) || upstream.status >= 500) {
      return json(503, { error: `upstream ${upstream.status}` });
    }
    // Grafana refused the data itself: retrying won't help.
    return json(200, { partialSuccess: { rejectedLogRecords: String(kept + dropped), errorMessage: `upstream ${upstream.status}` } });
  },
};
