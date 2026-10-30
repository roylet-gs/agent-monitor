// node --test telemetry/github-stats/test/*.test.js
import { test } from "node:test";
import assert from "node:assert/strict";

import { collect, otlpMetrics, platformOf, versionOf } from "../collect.mjs";

const NOW = Date.parse("2026-10-30T09:00:00Z");

/** A fetch answering GitHub-shaped JSON per path; `deny` paths get 403. */
function fakeFetch({ deny = [] } = {}) {
  const calls = [];
  const routes = {
    "/repos/o/r/releases?per_page=100&page=1": [
      {
        tag_name: "v2.1.70",
        published_at: "2026-10-30T01:10:01Z",
        assets: [
          { name: "agent-monitor-v2.1.70-darwin-arm64.tar.gz", download_count: 7 },
          { name: "agent-monitor-v2.1.70-linux-x64.tar.gz", download_count: 1 },
          { name: "SHA256SUMS", download_count: 3 },
        ],
      },
      { tag_name: "evidence-store", published_at: "2026-01-01T00:00:00Z", assets: [{ name: "x.png", download_count: 99 }] },
    ],
    "/repos/o/r": { stargazers_count: 2, forks_count: 0, subscribers_count: 1 },
    "/repos/o/r/traffic/clones?per=day": {
      count: 1236,
      uniques: 232,
      clones: [{ timestamp: "2026-10-29T00:00:00Z", count: 90, uniques: 20 }],
    },
    "/repos/o/r/traffic/views?per=day": { count: 18, uniques: 2, views: [] },
  };
  const impl = async (url, init) => {
    const path = url.replace("https://api.github.com", "");
    calls.push({ path, init });
    if (path === "/graphql") {
      return new Response(JSON.stringify({ data: { organization: { packages: { nodes: [{ name: "agent-monitor", statistics: { downloadsTotalCount: 41 } }] } } } }));
    }
    if (deny.some((d) => path.startsWith(d))) return new Response("{}", { status: 403 });
    return new Response(JSON.stringify(routes[path] ?? []), { status: routes[path] ? 200 : 404 });
  };
  return { impl, calls };
}

const value = (metrics, name, attrs = {}) =>
  metrics.find((m) => m.name === name && Object.entries(attrs).every(([k, v]) => m.attrs[k] === v))?.value;

test("names, versions and platforms come from tags and asset names", () => {
  assert.equal(versionOf("v2.1.70"), "2.1.70");
  assert.equal(versionOf("evidence-store"), null);
  assert.equal(platformOf("agent-monitor-v2.1.70-darwin-arm64.tar.gz"), "darwin-arm64");
  assert.equal(platformOf("SHA256SUMS"), null);
});

test("collects downloads per version and platform, traffic, stars and npm", async () => {
  const { impl } = fakeFetch();
  const { metrics, warnings } = await collect({ repo: "o/r", token: "t", trafficToken: "p", packagesToken: "k", fetchImpl: impl });
  assert.deepEqual(warnings, []);
  assert.equal(value(metrics, "am.github.release.downloads", { version: "2.1.70", platform: "darwin-arm64" }), 7);
  assert.equal(metrics.filter((m) => m.name === "am.github.release.downloads").length, 2, "checksums and other releases left out");
  assert.equal(value(metrics, "am.github.release.published_seconds", { version: "2.1.70" }), Date.parse("2026-10-30T01:10:01Z") / 1000);
  assert.equal(value(metrics, "am.github.clones.uniques_14d"), 232);
  assert.equal(value(metrics, "am.github.views.14d"), 18);
  assert.equal(value(metrics, "am.github.stars"), 2);
  assert.equal(value(metrics, "am.github.npm.downloads", { package: "agent-monitor" }), 41);
});

test("what a token can't read is skipped with a warning", async () => {
  const { impl } = fakeFetch({ deny: ["/repos/o/r/traffic"] });
  const { metrics, warnings } = await collect({ repo: "o/r", token: "t", trafficToken: "p", fetchImpl: impl });
  assert.ok(warnings.some((w) => w.includes("traffic/clones") && w.includes("403")), warnings.join("\n"));
  assert.ok(warnings.some((w) => w.startsWith("npm: no STATS_PACKAGES_TOKEN")));
  assert.equal(value(metrics, "am.github.stars"), 2, "the rest is still collected");

  const none = await collect({ repo: "o/r", token: "t", fetchImpl: fakeFetch().impl });
  assert.ok(none.warnings.some((w) => w === "traffic/clones: no STATS_GITHUB_TOKEN"));
});

test("the OTLP body has one gauge per metric, without units", () => {
  const body = otlpMetrics(
    [
      { name: "am.github.release.downloads", value: 7, attrs: { version: "2.1.70", platform: "darwin-arm64" } },
      { name: "am.github.release.downloads", value: 1, attrs: { version: "2.1.70", platform: "linux-x64" } },
      { name: "am.github.stars", value: 2, attrs: {} },
    ],
    NOW,
  );
  const rm = body.resourceMetrics[0];
  assert.deepEqual(rm.resource.attributes, [{ key: "service.name", value: { stringValue: "am-github-stats" } }]);
  const metrics = rm.scopeMetrics[0].metrics;
  assert.deepEqual(metrics.map((m) => m.name), ["am.github.release.downloads", "am.github.stars"]);
  assert.equal(metrics[0].unit, undefined);
  assert.equal(metrics[0].gauge.dataPoints.length, 2);
  assert.deepEqual(metrics[0].gauge.dataPoints[0], {
    timeUnixNano: String(BigInt(NOW) * 1_000_000n),
    asInt: "7",
    attributes: [
      { key: "version", value: { stringValue: "2.1.70" } },
      { key: "platform", value: { stringValue: "darwin-arm64" } },
    ],
  });
});
