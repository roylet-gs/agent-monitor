// Daily GitHub numbers for the am — Usage overview dashboard: release
// downloads per version and platform, repository traffic (clones, views),
// stars, and npm (GitHub Packages) downloads. Pushed to Grafana Cloud as OTLP
// gauges (Prometheus metrics `am_github_*`) by .github/workflows/usage-stats.yml.
// No dependencies (Node 20+).
//
// Environment:
//   GITHUB_REPOSITORY        owner/name (set by Actions)
//   GITHUB_TOKEN             releases and repo counts (the Action's own token)
//   STATS_GITHUB_TOKEN       optional: traffic (clones, views) needs push access,
//                            e.g. a fine-grained token with Administration: read
//   STATS_PACKAGES_TOKEN     optional: npm download counts need `read:packages`
//   GRAFANA_OTLP_ENDPOINT    e.g. https://otlp-gateway-prod-au-southeast-1.grafana.net/otlp
//   GRAFANA_INSTANCE_ID      the stack's instance id
//   GRAFANA_METRICS_TOKEN    an access-policy token with only metrics:write
//   DRY_RUN=1                print what would be sent instead of sending
// Anything a token can't read is skipped with a warning, never a failure.

const API = "https://api.github.com";

/** A GET against the GitHub API; throws with the status on failure. */
export async function gh(path, token, fetchImpl = fetch) {
  const res = await fetchImpl(`${API}${path}`, {
    headers: {
      accept: "application/vnd.github+json",
      "x-github-api-version": "2022-11-28",
      ...(token ? { authorization: `Bearer ${token}` } : {}),
    },
  });
  if (!res.ok) throw new Error(`GET ${path}: HTTP ${res.status}`);
  return res.json();
}

/** `v2.1.70` → `2.1.70`; anything else (e.g. the evidence-store release) → null. */
export function versionOf(tag) {
  const m = /^v(\d+\.\d+\.\d+)$/.exec(tag ?? "");
  return m ? m[1] : null;
}

/** `agent-monitor-v2.1.70-darwin-arm64.tar.gz` → `darwin-arm64`. */
export function platformOf(asset) {
  const m = /^agent-monitor-v[^-]+-(.+)\.tar\.gz$/.exec(asset ?? "");
  return m ? m[1] : null;
}

/** Every number, as `{ name, value, attrs }` gauges, plus warnings. */
export async function collect({ repo, token, trafficToken, packagesToken, fetchImpl = fetch }) {
  const metrics = [];
  const warnings = [];
  const add = (name, value, attrs = {}) => {
    if (Number.isFinite(value)) metrics.push({ name, value, attrs });
  };

  // Releases: downloads per version and platform, and when each was published.
  try {
    for (let page = 1; page <= 10; page++) {
      const releases = await gh(`/repos/${repo}/releases?per_page=100&page=${page}`, token, fetchImpl);
      for (const r of releases) {
        const version = versionOf(r.tag_name);
        if (!version) continue;
        add("am.github.release.published_seconds", Math.floor(Date.parse(r.published_at) / 1000), { version });
        for (const a of r.assets ?? []) {
          const platform = platformOf(a.name);
          if (platform) add("am.github.release.downloads", a.download_count, { version, platform });
        }
      }
      if (releases.length < 100) break;
    }
  } catch (e) {
    warnings.push(`releases: ${e.message}`);
  }

  // Repository counts.
  try {
    const r = await gh(`/repos/${repo}`, token, fetchImpl);
    add("am.github.stars", r.stargazers_count);
    add("am.github.forks", r.forks_count);
    add("am.github.watchers", r.subscribers_count);
  } catch (e) {
    warnings.push(`repo: ${e.message}`);
  }

  // Traffic (push access only): the rolling 14-day totals. GitHub's daily
  // entries lag a day or two, so a "yesterday" would often read 0.
  for (const kind of ["clones", "views"]) {
    if (!trafficToken) {
      warnings.push(`traffic/${kind}: no STATS_GITHUB_TOKEN`);
      continue;
    }
    try {
      const t = await gh(`/repos/${repo}/traffic/${kind}?per=day`, trafficToken, fetchImpl);
      add(`am.github.${kind}.14d`, t.count);
      add(`am.github.${kind}.uniques_14d`, t.uniques);
    } catch (e) {
      warnings.push(`traffic/${kind}: ${e.message}`);
    }
  }

  // npm downloads (GitHub Packages; `read:packages` only).
  if (!packagesToken) {
    warnings.push("npm: no STATS_PACKAGES_TOKEN");
  } else {
    const owner = repo.split("/")[0];
    let found = false;
    for (const kind of ["organization", "user"]) {
      try {
        const res = await fetchImpl(`${API}/graphql`, {
          method: "POST",
          headers: { authorization: `Bearer ${packagesToken}`, "content-type": "application/json" },
          body: JSON.stringify({
            query: `query($o:String!){ ${kind}(login:$o){ packages(first:20, packageType: NPM){ nodes{ name statistics{ downloadsTotalCount } } } } }`,
            variables: { o: owner },
          }),
        });
        const body = await res.json();
        const nodes = body?.data?.[kind]?.packages?.nodes;
        if (!nodes) continue;
        for (const n of nodes) add("am.github.npm.downloads", n.statistics?.downloadsTotalCount, { package: n.name });
        found = true;
        break;
      } catch (e) {
        warnings.push(`npm (${kind}): ${e.message}`);
      }
    }
    if (!found) warnings.push("npm: no packages readable with STATS_PACKAGES_TOKEN");
  }

  return { metrics, warnings };
}

/** The OTLP/JSON metrics request: one gauge per metric name. */
export function otlpMetrics(metrics, nowMs) {
  const time = String(BigInt(Math.floor(nowMs)) * 1_000_000n);
  const byName = new Map();
  for (const m of metrics) {
    if (!byName.has(m.name)) byName.set(m.name, []);
    byName.get(m.name).push({
      timeUnixNano: time,
      asInt: String(Math.round(m.value)),
      attributes: Object.entries(m.attrs).map(([key, v]) => ({ key, value: { stringValue: String(v) } })),
    });
  }
  return {
    resourceMetrics: [
      {
        resource: { attributes: [{ key: "service.name", value: { stringValue: "am-github-stats" } }] },
        scopeMetrics: [
          {
            scope: { name: "am-github-stats" },
            // No unit: Grafana's OTLP translation would otherwise add one as a suffix.
            metrics: [...byName].map(([name, dataPoints]) => ({ name, gauge: { dataPoints } })),
          },
        ],
      },
    ],
  };
}

async function main() {
  const env = process.env;
  const repo = env.GITHUB_REPOSITORY ?? "roylet-gs/agent-monitor";
  const { metrics, warnings } = await collect({
    repo,
    token: env.GITHUB_TOKEN,
    trafficToken: env.STATS_GITHUB_TOKEN,
    packagesToken: env.STATS_PACKAGES_TOKEN,
  });
  for (const w of warnings) console.log(`::warning::${w}`);
  const body = otlpMetrics(metrics, Date.now());
  const summary = metrics.filter((m) => m.name !== "am.github.release.downloads" && m.name !== "am.github.release.published_seconds");
  console.log(`${metrics.length} data points (${summary.map((m) => `${m.name}=${m.value}`).join(", ")})`);
  if (env.DRY_RUN) {
    console.log(JSON.stringify(body, null, 2));
    return;
  }
  for (const k of ["GRAFANA_OTLP_ENDPOINT", "GRAFANA_INSTANCE_ID", "GRAFANA_METRICS_TOKEN"]) {
    if (!env[k]) throw new Error(`${k} is not set`);
  }
  const auth = Buffer.from(`${env.GRAFANA_INSTANCE_ID}:${env.GRAFANA_METRICS_TOKEN}`).toString("base64");
  const res = await fetch(`${env.GRAFANA_OTLP_ENDPOINT.replace(/\/$/, "")}/v1/metrics`, {
    method: "POST",
    headers: { "content-type": "application/json", authorization: `Basic ${auth}` },
    body: JSON.stringify(body),
  });
  const text = await res.text();
  if (!res.ok) throw new Error(`Grafana answered HTTP ${res.status}: ${text.slice(0, 300)}`);
  console.log(`Sent to Grafana (HTTP ${res.status})`);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main().catch((e) => {
    console.error(e.message);
    process.exit(1);
  });
}
