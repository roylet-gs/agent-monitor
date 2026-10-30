// Push synthetic usage reports through the relay into a local Loki (grafana/otel-lgtm
// with OTLP on :14318). See telemetry/README.md:
// node telemetry/grafana/seed-local.mjs "$PWD/telemetry/relay/src/worker.js" telemetry/relay/test/fixtures/client-body.json
import fs from "node:fs";
const [, , relayPath, fixturePath] = process.argv;
const worker = (await import(relayPath)).default;
const fixture = JSON.parse(fs.readFileSync(fixturePath, "utf8"));
const templates = fixture.resourceLogs.flatMap((r) => r.scopeLogs[0].logRecords.map((rec) => ({ res: r.resource, rec })));
const env = { GRAFANA_OTLP_ENDPOINT: process.env.OTLP_URL ?? "http://localhost:14318", GRAFANA_INSTANCE_ID: "x", GRAFANA_API_TOKEN: "y" };
const now = Date.now();
let sent = 0;
for (let i = 0; i < 12; i++) {
  const id = `00000000-0000-4000-8000-${String(i).padStart(12, "0")}`;
  const version = i % 3 === 0 ? "2.1.67" : "2.2.0";
  const resourceLogs = templates.map(({ res, rec }) => {
    const attrs = res.attributes.map((a) =>
      a.key === "am.install_id" ? { key: a.key, value: { stringValue: id } }
      : a.key === "service.version" ? { key: a.key, value: { stringValue: version } } : a);
    const ts = BigInt(now - Math.floor(Math.random() * 36e5 * 40)) * 1000000n;
    const S = (k, v) => ({ key: k, value: { stringValue: v } });
    const I = (k, v) => ({ key: k, value: { intValue: String(v) } });
    const B = (k, v) => ({ key: k, value: { boolValue: v } });
    let extra = [];
    if (rec.eventName === "am.heartbeat") {
      extra = [B("linear", i % 2 === 0), B("reviews", true), B("compact", i % 4 === 0), B("cmux", i % 5 === 0), B("builtin_theme", i !== 3),
        I("worktrees", 3 + i), I("sessions.codex", i % 3), I("sessions.gemini", 1), S("terminal", ["ghostty", "iTerm.app", "vscode"][i % 3])];
    }
    const recs = [{ ...rec, timeUnixNano: String(ts), attributes: [...rec.attributes, ...extra] }];
    if (rec.eventName === "am.perf") {
      for (const [stage, us] of [["ui.frame", 900], ["ui.input", 20000], ["collector.cycle", 400000 + i * 30000], ["collector.sync", 9000], ["collector.context", 4000], ["collector.procscan", 12000], ["collector.usage", 3000], ["collector.stacks", 50000], ["integrations.github", 3000000], ["integrations.linear", 800000], ["integrations.reviews", 4000000]]) {
        recs.push({ ...rec, timeUnixNano: String(ts), attributes: rec.attributes.map((a) => a.key === "stage" ? S("stage", stage) : a.key === "p95_us" ? I("p95_us", us) : a) });
      }
    }
    return { resource: { attributes: attrs }, scopeLogs: [{ scope: { name: "am" }, logRecords: recs }] };
  });
  const req = new Request("https://relay.test/v1/logs", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ resourceLogs }) });
  const res = await worker.fetch(req, env);
  const body = await res.json();
  if (res.status !== 200 || body.partialSuccess?.rejectedLogRecords !== "0") throw new Error(`${res.status} ${JSON.stringify(body)}`);
  sent += resourceLogs.reduce((n, r) => n + r.scopeLogs[0].logRecords.length, 0);
}
console.log("sent", sent, "records through the relay");
