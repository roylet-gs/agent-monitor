"""Generates am's Grafana dashboards (am-usage.json, am-health.json) from the
usage-report schema (crates/am-core/src/telemetry/schema.rs, TELEMETRY.md).
Edit this, not the JSON: `python3 telemetry/grafana/generate.py telemetry/grafana`.

How Grafana Cloud stores the reports (checked against grafana/otel-lgtm):
only `service_name` is an index label; everything else (event_name,
am_install_id, service_version, command, code_filepath, sessions_claude, …)
is structured metadata with dots as underscores, filtered after the selector.
"""
import json, sys, os
out = sys.argv[1]
DS = {"type": "loki", "uid": "${loki}"}
SEL = '{service_name="am"}'
def ev(name): return f'{SEL} | event_name="{name}"'

def panel(pid, title, ptype, targets, x, y, w, h, **extra):
    p = {"id": pid, "title": title, "type": ptype, "datasource": DS,
         "gridPos": {"x": x, "y": y, "w": w, "h": h},
         "targets": [{"refId": chr(65+i), "datasource": DS, "expr": e, "legendFormat": lf,
                      "queryType": qt} for i, (e, lf, qt) in enumerate(targets)]}
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

def dashboard(uid, title, desc, panels):
    return {
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

os.makedirs(out, exist_ok=True)
for name, d in [("am-usage.json", usage), ("am-health.json", health)]:
    with open(os.path.join(out, name), "w") as f:
        json.dump(d, f, indent=2); f.write("\n")
print("ok")
