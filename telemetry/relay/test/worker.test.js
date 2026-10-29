// node --test telemetry/relay/test  (Node 20+: fetch/Request/Response are global)
import { test } from "node:test";
import assert from "node:assert/strict";

import worker, { sanitize, MAX_BODY_BYTES } from "../src/worker.js";

const ENV = {
  GRAFANA_OTLP_ENDPOINT: "https://otlp.example.test/otlp/",
  GRAFANA_INSTANCE_ID: "123456",
  GRAFANA_API_TOKEN: "glc_secret",
};

const str = (key, v) => ({ key, value: { stringValue: v } });
const int = (key, n) => ({ key, value: { intValue: String(n) } });

const resource = (over = {}) => ({
  attributes: [
    str("service.name", "am"),
    str("service.version", "2.2.0"),
    str("am.binary", "am"),
    str("os.type", "darwin"),
    str("host.arch", "arm64"),
    str("am.install_method", "homebrew"),
    str("am.install_id", "3f0c1f5e-1d2b-4c1e-9a55-0d7a1c2b3e4f"),
    ...(over.extra ?? []),
  ],
});

const record = (name, attributes) => ({
  timeUnixNano: "1793312213923000000",
  severityNumber: 9,
  severityText: "INFO",
  eventName: name,
  body: { stringValue: name },
  attributes,
});

const payload = (records, res = resource()) => ({
  resourceLogs: [{ resource: res, scopeLogs: [{ scope: { name: "am" }, logRecords: records }] }],
});

function post(body, headers = {}) {
  const text = typeof body === "string" ? body : JSON.stringify(body);
  return new Request("https://relay.example.test/v1/logs", {
    method: "POST",
    headers: { "content-type": "application/json", "cf-connecting-ip": "203.0.113.9", ...headers },
    body: text,
  });
}

/** Run the worker with `fetch` stubbed to answer `status`; returns the response and what was forwarded. */
async function run(request, { status = 200, env = ENV } = {}) {
  const calls = [];
  const real = globalThis.fetch;
  globalThis.fetch = async (url, init) => {
    calls.push({ url, init, body: JSON.parse(init.body) });
    return new Response("", { status });
  };
  try {
    const response = await worker.fetch(request, env);
    return { response, json: await response.json(), calls };
  } finally {
    globalThis.fetch = real;
  }
}

test("forwards valid events with Grafana's credentials and no client IP", async () => {
  const { response, json, calls } = await run(post(payload([record("am.command", [str("command", "worktree.create"), str("via", "cli")])])));
  assert.equal(response.status, 200);
  assert.equal(json.partialSuccess.rejectedLogRecords, "0");
  assert.equal(calls.length, 1);
  assert.equal(calls[0].url, "https://otlp.example.test/otlp/v1/logs");
  assert.equal(calls[0].init.headers.authorization, `Basic ${btoa("123456:glc_secret")}`);
  const sent = JSON.stringify(calls[0].body);
  assert.ok(!sent.includes("203.0.113.9"), "the IP never leaves the relay");
  const rec = calls[0].body.resourceLogs[0].scopeLogs[0].logRecords[0];
  assert.ok(rec.attributes.some((a) => a.key === "event.name" && a.value.stringValue === "am.command"));
});

test("drops records with anything off-schema, keeps the rest", async () => {
  const records = [
    record("am.command", [str("command", "repo.list")]),
    record("am.command", [str("command", "/Users/someone/secret")]),
    record("am.error", [str("code.filepath", "/Users/me/.cargo/registry/x.rs")]),
    record("am.error", [str("code.filepath", "crates/am-core/src/db.rs"), int("code.lineno", 42)]),
    record("am.anything", []),
    record("am.heartbeat", [{ key: "x", value: { arrayValue: { values: [] } } }]),
  ];
  const { json, calls } = await run(post(payload(records)));
  assert.equal(json.partialSuccess.rejectedLogRecords, "4");
  const kept = calls[0].body.resourceLogs[0].scopeLogs[0].logRecords;
  assert.deepEqual(kept.map((r) => r.eventName), ["am.command", "am.error"]);
  assert.ok(!JSON.stringify(calls[0].body).includes("someone"));
});

test("unknown resource attributes are dropped; a resource that isn't am's drops its records", async () => {
  const extra = [str("host.name", "toms-macbook"), str("user.email", "x")];
  const { calls } = await run(post(payload([record("am.command", [str("command", "doctor")])], resource({ extra }))));
  const sent = JSON.stringify(calls[0].body);
  assert.ok(!sent.includes("toms-macbook") && !sent.includes("user.email"));

  const notAm = { attributes: [str("service.name", "something-else"), str("am.binary", "am"), str("service.version", "1")] };
  const r = await run(post(payload([record("am.command", [str("command", "doctor")])], notAm)));
  assert.equal(r.calls.length, 0);
  assert.equal(r.json.partialSuccess.rejectedLogRecords, "1");
});

test("never makes a client retry what it can't fix", async () => {
  for (const req of [post("not json"), post({ resourceLogs: "nope" })]) {
    const { response, calls } = await run(req);
    assert.equal(response.status, 200);
    assert.equal(calls.length, 0);
  }
  // Grafana refusing the data (4xx) isn't retried either…
  const refused = await run(post(payload([record("am.command", [str("command", "doctor")])])), { status: 400 });
  assert.equal(refused.response.status, 200);
  // …but Grafana throttling or down is.
  for (const status of [429, 500, 502]) {
    const r = await run(post(payload([record("am.command", [str("command", "doctor")])])), { status });
    assert.equal(r.response.status, 503, `upstream ${status}`);
  }
});

test("only POST /v1/logs with JSON, within the size cap", async () => {
  assert.equal((await worker.fetch(new Request("https://r.test/"), ENV)).status, 404);
  assert.equal((await worker.fetch(new Request("https://r.test/v1/logs"), ENV)).status, 405);
  const proto = post("x", { "content-type": "application/x-protobuf" });
  assert.equal((await worker.fetch(proto, ENV)).status, 415);
  const big = post("x".repeat(MAX_BODY_BYTES + 1));
  assert.equal((await worker.fetch(big, ENV)).status, 413);
});

test("rate-limits per IP when the binding is there", async () => {
  const seen = [];
  const env = { ...ENV, RATE_LIMITER: { limit: async ({ key }) => (seen.push(key), { success: false }) } };
  const { response, calls } = await run(post(payload([])), { env });
  assert.equal(response.status, 429);
  assert.deepEqual(seen, ["203.0.113.9"]);
  assert.equal(calls.length, 0);
});

test("times are clamped near arrival (Loki refuses old entries); the client's is kept", () => {
  const now = 1_800_000_000_000n * 1_000_000n;
  const old = { ...record("am.heartbeat", []), timeUnixNano: String(now - 3n * 86_400n * 1_000_000_000n) };
  const fresh = { ...record("am.heartbeat", []), timeUnixNano: String(now - 60n * 1_000_000_000n) };
  const ahead = { ...record("am.heartbeat", []), timeUnixNano: String(now + 3_600n * 1_000_000_000n) };
  const recs = sanitize(payload([old, fresh, ahead]), now).body.resourceLogs[0].scopeLogs[0].logRecords;
  const minutes = recs.map((r) => Number((BigInt(r.timeUnixNano) - now) / 60_000_000_000n));
  assert.deepEqual(minutes, [-30, -1, 5]);
  const recorded = recs[0].attributes.find((a) => a.key === "recorded_ms").value.intValue;
  assert.equal(BigInt(recorded) * 1_000_000n, BigInt(old.timeUnixNano));
});

test("sanitize caps the records per request", () => {
  const many = Array.from({ length: 1200 }, () => record("am.command", [str("command", "doctor")]));
  const { kept, dropped } = sanitize(payload(many));
  assert.equal(kept, 1000);
  assert.equal(dropped, 200);
});

test("everything the Rust client sends passes untouched (fixture from schema.rs)", async () => {
  const fs = await import("node:fs");
  const body = JSON.parse(fs.readFileSync(new URL("./fixtures/client-body.json", import.meta.url), "utf8"));
  const sent = body.resourceLogs.flatMap((r) => r.scopeLogs.flatMap((s) => s.logRecords)).length;
  const { kept, dropped } = sanitize(body);
  assert.equal(sent, 6, "one of each event");
  assert.deepEqual({ kept, dropped }, { kept: sent, dropped: 0 });
});
