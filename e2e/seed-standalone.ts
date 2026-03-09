import { getDb, upsertStandaloneSession } from "../src/lib/db.js";

getDb();
upsertStandaloneSession("/home/user/my-project", "executing", "sess-standalone-1", null, "Refactoring auth module", true);
upsertStandaloneSession("/home/user/scripts", "idle", "sess-standalone-2", "Done: updated deploy script", null, true);
console.log("Seeded 2 standalone sessions");
