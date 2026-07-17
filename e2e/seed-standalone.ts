import { getDb, upsertStandaloneSession } from "../src/lib/db.js";

getDb();
upsertStandaloneSession("/home/user/my-project", "executing", "sess-standalone-1", null, "Refactoring auth module", true);
// Long idle last_response containing emoji + a markdown link. This exercises
// normalizeSummary (emoji stripped, link reduced to its label) in the detail
// panel's "Last Response" section. Without the fix the retained emoji make the
// truncated line one column too wide, wrapping it and breaking the panel's
// vertical borders. See tests/e2e/docker/description-borders.spec.ts.
upsertStandaloneSession(
  "/home/user/scripts",
  "idle",
  "sess-standalone-2",
  "CI passed. ✅ Done — PR [#115](https://github.com/roylet-gs/agent-monitor/pull/115) is green 🎉 everything merged and deployed 🚀 to production now and all systems nominal",
  null,
  true
);
console.log("Seeded 2 standalone sessions");
