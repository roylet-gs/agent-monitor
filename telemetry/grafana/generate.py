"""Generates am's Grafana dashboards (am-usage.json, am-health.json,
am-overview.json) from the usage-report schema
(crates/am-core/src/telemetry/schema.rs, TELEMETRY.md) and the daily GitHub
numbers (telemetry/github-stats: Prometheus `am_github_*` gauges).
Edit this, not the JSON: `python3 telemetry/grafana/generate.py telemetry/grafana`.

How Grafana Cloud stores the reports (checked against grafana/otel-lgtm):
only `service_name` is an index label; everything else (event_name,
am_install_id, service_version, command, code_filepath, sessions_claude, …)
is structured metadata with dots as underscores, filtered after the selector.
"""
import json, sys, os
out = sys.argv[1]
DS = {"type": "loki", "uid": "${loki}"}
PROM = {"type": "prometheus", "uid": "${prom}"}
MIXED = {"type": "datasource", "uid": "-- Mixed --"}
SEL = '{service_name="am"}'
def ev(name): return f'{SEL} | event_name="{name}"'

def panel(pid, title, ptype, targets, x, y, w, h, **extra):
    """`targets`: (expr, legend, "instant"|"range"[, datasource]); Loki unless
    given, and a panel mixing data sources gets Grafana's Mixed source."""
    ds_of = lambda t: t[3] if len(t) > 3 else DS
    targets_out = []
    for i, t in enumerate(targets):
        ds = ds_of(t)
        q = {"refId": chr(65+i), "datasource": ds, "expr": t[0], "legendFormat": t[1]}
        if ds is PROM:
            q.update({"instant": t[2] == "instant", "range": t[2] == "range"})
        else:
            q["queryType"] = t[2]
        targets_out.append(q)
    kinds = {id(ds_of(t)) for t in targets}
    p = {"id": pid, "title": title, "type": ptype,
         "datasource": ds_of(targets[0]) if len(kinds) == 1 else MIXED,
         "gridPos": {"x": x, "y": y, "w": w, "h": h}, "targets": targets_out}
    p.update(extra)
    return p

def named(p, names):
    """Instant queries ignore legendFormat: name each query's field by its refId."""
    p.setdefault("fieldConfig", {"defaults": {}, "overrides": []})
    p["fieldConfig"]["overrides"] = [{"matcher": {"id": "byFrameRefID", "options": chr(65+i)},
                                       "properties": [{"id": "displayName", "value": n}]} for i, n in enumerate(names)]
    return p

def summed(p):
    """A range query of per-interval counts, summed over the range: one value per series."""
    p["options"] = {**p.get("options", {}), "reduceOptions": {"values": False, "calcs": ["sum"], "fields": ""}}
    return p

def row(pid, title, y):
    return {"id": pid, "type": "row", "title": title, "collapsed": False, "gridPos": {"x": 0, "y": y, "w": 24, "h": 1}, "panels": []}

def dashboard(uid, title, desc, panels, prom=False):
    d = {
        "uid": uid, "title": title, "description": desc, "tags": ["am", "usage-reports"],
        "schemaVersion": 39, "editable": True, "time": {"from": "now-30d", "to": "now"},
        "refresh": "", "timezone": "utc", "graphTooltip": 1,
        "templating": {"list": [
            # Grafana Cloud stacks also have alert-state-history and
            # usage-insights Loki sources, which sort first: skip them so the
            # stack's own logs are picked by default.
            {"name": "loki", "label": "Logs", "type": "datasource", "query": "loki", "current": {}, "hide": 0,
             "regex": "/^(?!.*(alert-state-history|usage-insights)).*$/"},
            {"name": "version", "label": "Version", "type": "query", "datasource": DS,
             "query": {"label": "service_version", "stream": SEL, "type": 1},
             "definition": f"label_values({SEL}, service_version)",
             "includeAll": True, "allValue": ".+", "multi": True, "current": {}, "refresh": 2, "sort": 4},
        ]},
        "annotations": {"list": []},
        "panels": panels,
    }
    if prom:
        # The stack's own metrics (`grafanacloud-<stack>-prom`), not its usage
        # or ML sources; a local grafana/otel-lgtm calls it Prometheus.
        d["templating"]["list"].insert(1, {"name": "prom", "label": "Metrics", "type": "datasource", "query": "prometheus",
                                           "current": {}, "hide": 0, "regex": "/prom/i"})
    return d

V = '| service_version=~"$version"'
inst = lambda q, r: f"count(sum by (am_install_id) (count_over_time({q} {V} [{r}])))"

# ---------------- Adoption & usage ----------------
p = []
p.append(row(1, "Adoption", 0))
p.append(panel(2, "Active installs (last 24 h)", "stat", [(inst(ev("am.heartbeat"), "24h"), "", "instant")], 0, 1, 4, 4,
               description="Installs that sent a daily heartbeat in the last day (distinct random install ids)."))
p.append(panel(3, "Active installs (last 7 d)", "stat", [(inst(ev("am.heartbeat"), "7d"), "", "instant")], 4, 1, 4, 4))
p.append(panel(4, "Active installs (last 30 d)", "stat", [(inst(ev("am.heartbeat"), "30d"), "", "instant")], 8, 1, 4, 4))
p.append(panel(5, "Active installs (rolling 24 h)", "timeseries", [(inst(ev("am.heartbeat"), "1d"), "installs", "range")], 12, 1, 12, 8,
               description="Distinct installs with a heartbeat in the 24 hours before each point."))
by_attr = lambda attr: f'sum by ({attr}) (count_over_time({ev("am.heartbeat")} {V} [$__interval]))'
p.append(summed(panel(6, "Versions", "piechart", [(f'sum by (service_version) (count_over_time({ev("am.heartbeat")} [$__interval]))', "{{service_version}}", "range")], 0, 5, 4, 8)))
p.append(summed(panel(7, "OS / CPU", "piechart", [(f'sum by (os_type, host_arch) (count_over_time({ev("am.heartbeat")} {V} [$__interval]))', "{{os_type}} {{host_arch}}", "range")], 4, 5, 4, 8)))
p.append(summed(panel(8, "Installed with", "piechart", [(by_attr("am_install_method"), "{{am_install_method}}", "range")], 8, 5, 4, 8)))
p.append(row(9, "Setup", 13))
p.append(summed(panel(10, "Mode", "piechart", [(by_attr("mode"), "{{mode}}", "range")], 0, 14, 4, 8)))
p.append(summed(panel(11, "Opens worktrees in", "piechart", [(by_attr("ide"), "{{ide}}", "range")], 4, 14, 4, 8)))
p.append(summed(panel(12, "Terminal", "piechart", [(f'sum by (terminal) (count_over_time({ev("am.heartbeat")} {V} | terminal!="" [$__interval]))', "{{terminal}}", "range")], 8, 14, 4, 8)))
flags = ["github", "linear", "reviews", "compact", "cmux", "builtin_theme"]
p.append(named(panel(13, "Integrations and options on (share of heartbeats)", "bargauge",
               [(f'(sum(count_over_time({ev("am.heartbeat")} {V} | {f}="true" [$__range])) or vector(0)) / sum(count_over_time({ev("am.heartbeat")} {V} [$__range]))', f, "instant") for f in flags],
               12, 14, 12, 8, fieldConfig={"defaults": {"unit": "percentunit", "min": 0, "max": 1}, "overrides": []},
               options={"orientation": "horizontal", "displayMode": "basic"}), flags))
harnesses = ["claude", "codex", "gemini", "cursor", "copilot", "opencode", "amp", "qwen", "droid", "kiro", "pi", "custom"]
p.append(named(panel(14, "Agent sessions seen in heartbeats (by harness)", "bargauge",
               [(f'sum(sum_over_time({ev("am.heartbeat")} {V} | sessions_{h}!="" | unwrap sessions_{h} [$__range]))', h, "instant") for h in harnesses],
               0, 22, 12, 9, options={"orientation": "horizontal", "displayMode": "basic"},
               description="Sum of open sessions per built-in harness across daily heartbeats; custom manifest plugins count as custom."), harnesses))
p.append(named(panel(15, "Repositories and worktrees per install (median)", "stat",
               [(f'quantile_over_time(0.5, {ev("am.heartbeat")} {V} | unwrap repos [$__range])', "repos", "instant"),
                (f'quantile_over_time(0.5, {ev("am.heartbeat")} {V} | unwrap worktrees [$__range])', "worktrees", "instant")],
               12, 22, 12, 9), ["repositories", "worktrees"]))
p.append(row(16, "Features", 31))
p.append(summed(panel(17, "Dashboard commands", "bargauge",
               [(f'sum by (command) (sum_over_time({ev("am.features")} {V} | unwrap count [$__interval]))', "{{command}}", "range")],
               0, 32, 12, 12, options={"orientation": "horizontal", "displayMode": "basic"},
               description="How often each key-bar command ran (Command::id), summed over the range.")))
p.append(summed(panel(18, "CLI commands (you vs. agents through MCP)", "bargauge",
               [(f'sum by (command, via) (count_over_time({ev("am.command")} {V} [$__interval]))', "{{command}} ({{via}})", "range")],
               12, 32, 12, 12, options={"orientation": "horizontal", "displayMode": "basic"})))
p.append(panel(19, "Commands over time", "timeseries",
               [(f'sum by (via) (count_over_time({ev("am.command")} {V} [$__interval]))', "CLI ({{via}})", "range"),
                (f'sum(sum_over_time({ev("am.features")} {V} | unwrap count [$__interval]))', "dashboard", "range")],
               0, 44, 24, 8))
usage = dashboard("am-usage", "am — Adoption & usage", "Anonymous usage reports from am (TELEMETRY.md): installs, versions, setup and features.", p)

# ---------------- Errors & performance ----------------
p = []
p.append(row(1, "Errors and crashes", 0))
p.append(panel(2, "Installs hitting an error (24 h)", "stat", [(inst(ev("am.error"), "24h"), "", "instant")], 0, 1, 4, 4))
p.append(panel(3, "Crashes (range)", "stat", [(f'sum(count_over_time({ev("am.crash")} {V} [$__range]))', "", "instant")], 4, 1, 4, 4,
               fieldConfig={"defaults": {"thresholds": {"mode": "absolute", "steps": [{"color": "green", "value": None}, {"color": "red", "value": 1}]}}, "overrides": []}))
p.append(panel(4, "Errors and warnings by version", "timeseries",
               [(f'sum by (service_version, detected_level) (count_over_time({ev("am.error")} {V} [$__interval]))', "{{service_version}} {{detected_level}}", "range")],
               8, 1, 16, 8))
p.append(panel(5, "Where errors happen (count and installs affected)", "table",
               [(f'sum by (level, module, code_filepath, code_lineno, error_kind) (count_over_time({ev("am.error")} {V} [$__range]))', "", "instant"),
                (f'count by (level, module, code_filepath, code_lineno, error_kind) (sum by (level, module, code_filepath, code_lineno, error_kind, am_install_id) (count_over_time({ev("am.error")} {V} [$__range])))', "", "instant")],
               0, 9, 24, 10,
               transformations=[{"id": "merge", "options": {}},
                                {"id": "organize", "options": {"excludeByName": {"Time": True}, "renameByName": {"Value #A": "events", "Value #B": "installs"}}},
                                {"id": "sortBy", "options": {"sort": [{"field": "installs", "desc": True}]}}],
               description="code_filepath:code_lineno is the WARN/ERROR call in am's source; the message itself is never sent."))
p.append(panel(6, "Crashes by location and version", "table",
               [(f'sum by (service_version, am_binary, code_filepath, code_lineno) (count_over_time({ev("am.crash")} [$__range]))', "", "instant")],
               0, 19, 24, 8, transformations=[{"id": "organize", "options": {"excludeByName": {"Time": True}, "renameByName": {"Value #A": "crashes"}}}]))
p.append(row(7, "Performance", 27))
budgets = {"ui.frame": 16, "ui.input": 50, "collector.cycle": 500, "collector.sync": 300, "collector.git": 3000,
           "collector.context": 100, "collector.procscan": 50, "collector.usage": 100, "collector.stacks": 1000,
           "integrations.github": 10000, "integrations.linear": 10000, "integrations.reviews": 10000}
p.append(panel(8, "Stage p95 across installs (ms)", "timeseries",
               [(f'quantile_over_time(0.95, {ev("am.perf")} {V} | unwrap p95_us [$__interval]) by (stage) / 1000', "{{stage}}", "range")],
               0, 28, 24, 10, fieldConfig={"defaults": {"unit": "ms"}, "overrides": []},
               description="The 95th percentile, across installs, of each install's 5-minute p95. Budgets (perf::BUDGETS): " + ", ".join(f"{k} {v}ms" for k, v in budgets.items())))
p.append(named(panel(9, "Stages over budget (share of 5-minute reports)", "bargauge",
               [(f'(sum(count_over_time({ev("am.perf")} {V} | stage="{s}" | p95_us > {b*1000} [$__range])) or vector(0)) / sum(count_over_time({ev("am.perf")} {V} | stage="{s}" [$__range]))', s, "instant") for s, b in budgets.items()],
               0, 38, 24, 10, fieldConfig={"defaults": {"unit": "percentunit", "min": 0, "max": 1,
                   "thresholds": {"mode": "absolute", "steps": [{"color": "green", "value": None}, {"color": "yellow", "value": 0.05}, {"color": "red", "value": 0.2}]}}, "overrides": []},
               options={"orientation": "horizontal", "displayMode": "gradient"}), list(budgets)))
health = dashboard("am-health", "am — Errors & performance", "Anonymous usage reports from am (TELEMETRY.md): where errors and crashes happen, and stage timings against perf::BUDGETS.", p)

# ---------------- Usage overview ----------------
# Opted-in usage reports (Loki) beside GitHub's own counts (Prometheus, daily
# from telemetry/github-stats), which see every install channel but are noisy.
# The collector pushes once a day and Prometheus only looks back 5 minutes for
# a sample, so every gauge is read as its latest value within a day or so
# (`gh`), and downloads gained over a window as max - min over it (`gained`:
# exact for counts that only grow, where increase() would extrapolate).
G = 'job=~".*am-github-stats"'
raw = lambda metric, extra="": f'am_github_{metric}{{{G}{extra}}}'
gh = lambda metric, extra="": f'last_over_time({raw(metric, extra)}[26h])'
gained = lambda metric, window: f'(max_over_time({raw(metric)}[{window}]) - min_over_time({raw(metric)}[{window}]))'
# Downloads of a release above the least-downloaded platform: crawlers fetch
# every platform alike, so what macOS arm64 (Homebrew's usual) has on top is
# closer to real installs.
beyond = (f'sum by (version) ({gh("release_downloads", ", platform=\"darwin-arm64\"")}) '
          f'- min by (version) ({gh("release_downloads")})')
# Only the 15 most recent releases (by publish time) in the per-version lists.
recent = f'topk(15, {gh("release_published_seconds")})'
# Counts aren't alarms: one colour, and a dash when nothing has arrived yet.
COUNT = {"defaults": {"color": {"mode": "fixed", "fixedColor": "blue"}, "noValue": "—"}, "overrides": []}
VERSION_BARS = {"orientation": "horizontal", "xField": "version", "showValue": "always", "legend": {"showLegend": False},
                "barWidth": 0.8, "xTickLabelSpacing": 0}

def version_bars(p):
    """One labelled bar per version: the Prometheus result as a table (version,
    value), drawn as a horizontal bar chart in the query's order."""
    p["type"] = "barchart"
    for t in p["targets"]:
        t["format"] = "table"
    p["transformations"] = [{"id": "organize", "options": {"excludeByName": {"Time": True}}}]
    return p
installs_per_version = f'count by (service_version) (sum by (service_version, am_install_id) (count_over_time({ev("am.heartbeat")} [1d])))'
p = []
p.append(row(1, "Installs", 0))
p.append(panel(2, "Active installs, opted in (7 d)", "stat", [(inst(ev("am.heartbeat"), "7d"), "", "instant")], 0, 1, 4, 4, fieldConfig=COUNT,
               description="Installs with usage reports on that sent a heartbeat in the last 7 days. Only installs that opted in (2.1.69 and later) count."))
p.append(panel(3, "Active installs, opted in (30 d)", "stat", [(inst(ev("am.heartbeat"), "30d"), "", "instant")], 4, 1, 4, 4, fieldConfig=COUNT))
p.append(panel(4, "Unique cloners (GitHub, 14 d)", "stat", [(f'max({gh("clones_uniques_14d")})', "", "instant", PROM)], 8, 1, 4, 4, fieldConfig=COUNT,
               description="Distinct machines that cloned the repo in GitHub's last 14 days: brew tap/update (the repo is the Homebrew tap) and CI runs, so an upper bound on Homebrew installs."))
p.append(panel(5, "npm downloads (all time)", "stat", [(f'sum({gh("npm_downloads")})', "", "instant", PROM)], 12, 1, 4, 4, fieldConfig=COUNT,
               description="GitHub Packages downloads of the npm packages (empty unless the collector can read packages)."))
p.append(panel(6, "Release downloads (7 d)", "stat", [(f'sum({gained("release_downloads", "7d")})', "", "instant", PROM)], 16, 1, 4, 4, fieldConfig=COUNT,
               description="Release tarballs downloaded in the last 7 days, every platform (crawlers included). Counts come from the daily collector, so this lags up to a day."))
p.append(panel(7, "GitHub stars", "stat", [(f'max({gh("stars")})', "", "instant", PROM)], 20, 1, 4, 4, fieldConfig=COUNT))
p.append(panel(8, "Installs over time", "timeseries",
               [(inst(ev("am.heartbeat"), "1d"), "active installs, opted in (rolling 24 h)", "range"),
                (f'max({gh("clones_uniques_14d")})', "unique cloners (GitHub, 14 d)", "range", PROM)],
               0, 5, 24, 8,
               fieldConfig={"defaults": {}, "overrides": [{"matcher": {"id": "byFrameRefID", "options": "B"},
                                                          "properties": [{"id": "custom.axisPlacement", "value": "right"}]}]},
               description="Opted-in installs (left axis) beside GitHub's unique cloners (right axis), a much larger but noisier count."))
p.append(row(9, "Versions", 13))
p.append(panel(10, "Opted-in installs per version (latest day)", "piechart",
               [(installs_per_version, "{{service_version}}", "range")], 0, 14, 8, 9,
               options={"reduceOptions": {"values": False, "calcs": ["lastNotNull"], "fields": ""},
                        "legend": {"displayMode": "table", "placement": "right", "values": ["value", "percent"]}},
               description="Distinct installs per am version that sent a heartbeat in the day before the latest point."))
p.append(panel(11, "Opted-in installs per version over time", "timeseries",
               [(installs_per_version, "{{service_version}}", "range")], 8, 14, 16, 9,
               fieldConfig={"defaults": {"custom": {"stacking": {"mode": "normal"}, "fillOpacity": 40, "drawStyle": "bars"}}, "overrides": []},
               description="How fast installs move to a new release, and who stays behind."))
p.append(version_bars(panel(12, "Release downloads per version, above the crawler baseline", "bargauge",
               [(f'sort_desc(({beyond}) and on (version) {recent})', "{{version}}", "instant", PROM)], 0, 23, 12, 12,
               options=VERSION_BARS, fieldConfig=COUNT,
               description="The 15 most recent releases: macOS arm64 downloads of each minus its least-downloaded platform. Crawlers fetch every platform alike, so the rest is mostly Homebrew installs and upgrades.")))
p.append(version_bars(panel(13, "Release downloads per version (all platforms)", "bargauge",
               [(f'sort_desc(sum by (version) ({gh("release_downloads")}) and on (version) {recent})', "{{version}}", "instant", PROM)], 12, 23, 12, 12,
               options=VERSION_BARS, fieldConfig=COUNT,
               description="The 15 most recent releases.")))
p.append(panel(14, "Release downloads per day", "timeseries",
               [(f'sum by (platform) ({gained("release_downloads", "25h")})', "{{platform}}", "range", PROM)], 0, 35, 24, 8,
               interval="1d", fieldConfig={"defaults": {"custom": {"drawStyle": "bars", "fillOpacity": 60, "stacking": {"mode": "normal"}}}, "overrides": []}))
p.append(row(15, "Channels and engagement", 43))
p.append(summed(panel(16, "Installed with (opted in)", "piechart", [(by_attr("am_install_method"), "{{am_install_method}}", "range")], 0, 44, 6, 8)))
p.append(summed(panel(17, "OS / CPU (opted in)", "piechart", [(f'sum by (os_type, host_arch) (count_over_time({ev("am.heartbeat")} {V} [$__interval]))', "{{os_type}} {{host_arch}}", "range")], 6, 44, 6, 8)))
p.append(summed(panel(18, "Top dashboard commands", "bargauge",
               [(f'sum by (command) (sum_over_time({ev("am.features")} {V} | unwrap count [$__interval]))', "{{command}}", "range")],
               12, 44, 12, 8, options={"orientation": "horizontal", "displayMode": "basic"})))
p.append(panel(19, "CLI commands per day (you vs. agents through MCP)", "timeseries",
               [(f'sum by (via) (count_over_time({ev("am.command")} {V} [1d]))', "{{via}}", "range")], 0, 52, 12, 8, interval="1d",
               fieldConfig={"defaults": {"custom": {"drawStyle": "bars", "fillOpacity": 60, "stacking": {"mode": "normal"}}}, "overrides": []}))
p.append(panel(20, "Repo traffic (GitHub, rolling 14 d)", "timeseries",
               [(f'max({gh("clones_14d")})', "clones", "range", PROM), (f'max({gh("clones_uniques_14d")})', "unique cloners", "range", PROM),
                (f'max({gh("views_uniques_14d")})', "unique visitors", "range", PROM)], 12, 52, 12, 8))
overview = dashboard("am-overview", "am — Usage overview",
                     "How many people use am and on which versions: opted-in usage reports (TELEMETRY.md) beside GitHub's own counts (telemetry/github-stats).", p, prom=True)

os.makedirs(out, exist_ok=True)
for name, d in [("am-usage.json", usage), ("am-health.json", health), ("am-overview.json", overview)]:
    with open(os.path.join(out, name), "w") as f:
        json.dump(d, f, indent=2); f.write("\n")
print("ok")
