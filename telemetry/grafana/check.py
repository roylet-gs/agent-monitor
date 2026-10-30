"""Runs every panel query of the given dashboards against a local Grafana
(grafana/otel-lgtm on :13000, admin/admin, Loki uid `loki`) over the last 2
days and flags errors and empty results. See telemetry/README.md.
`python3 telemetry/grafana/check.py telemetry/grafana/*.json`
"""
import json, os, sys, urllib.request, base64
files = sys.argv[1:]
auth = "Basic " + base64.b64encode(b"admin:admin").decode()
def query(expr, qtype, kind="loki"):
    q = {"refId": "A", "datasource": {"type": kind, "uid": kind}, "expr": expr, "maxLines": 100, "intervalMs": 3600000}
    if kind == "prometheus":
        # 1-minute steps: freshly seeded samples are newer than the last hourly
        # step Grafana would align to.
        q.update({"instant": qtype == "instant", "range": qtype == "range", "intervalMs": 60000, "maxDataPoints": 5000})
    else:
        q["queryType"] = qtype
    body = {"from": "now-2d", "to": "now", "queries": [q]}
    req = urllib.request.Request(os.environ.get("GRAFANA_URL", "http://localhost:13000") + "/api/ds/query", data=json.dumps(body).encode(),
                                 headers={"content-type": "application/json", "authorization": auth})
    try:
        r = json.load(urllib.request.urlopen(req))
    except urllib.error.HTTPError as e:
        return "HTTP %s %s" % (e.code, e.read()[:300].decode())
    res = r["results"]["A"]
    if res.get("error"): return "ERROR " + res["error"][:300]
    vals = []
    for f in res.get("frames", []):
        for col in f.get("data", {}).get("values", [])[1:]:
            vals += [v for v in col if v is not None]
    return vals
bad = 0
for path in files:
    d = json.load(open(path))
    print("==", d["title"])
    for p in d["panels"]:
        for t in p.get("targets", []):
            expr = t["expr"].replace('$version', '.+').replace('$__range', '2d').replace('$__auto', '1h')
            kind = t["datasource"]["type"]
            qtype = t.get("queryType") or ("instant" if t.get("instant") else "range")
            out = query(expr, qtype, kind)
            ok = isinstance(out, list) and len(out) > 0
            bad += not ok
            shown = (out[:3] if isinstance(out, list) else out)
            print(("  ok  " if ok else "  !!  ") + f'{p["title"][:48]:48} [{t.get("legendFormat") or t["refId"]}] -> {shown}')
print("panels with problems:", bad)
