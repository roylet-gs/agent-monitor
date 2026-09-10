import { describe, it, expect, vi, beforeEach } from "vitest";
import type { FetchResult, LinearInfo, PrInfo } from "../../src/lib/types.js";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(), initLogger: vi.fn(), setLogLevel: vi.fn(),
}));

const mockFetchLinearResult = vi.fn();
const mockFetchAllPrResults = vi.fn();

vi.mock("../../src/lib/linear.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../src/lib/linear.js")>();
  return { ...actual, fetchLinearResult: (...a: unknown[]) => mockFetchLinearResult(...a) };
});

const mockClearPrBackoff = vi.fn();

vi.mock("../../src/lib/github.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../src/lib/github.js")>();
  return {
    ...actual,
    fetchAllPrResults: (...a: unknown[]) => mockFetchAllPrResults(...a),
    clearPrBackoff: (...a: unknown[]) => mockClearPrBackoff(...a),
  };
});

const REPO = { repoPath: "/repo", repoId: "r1", branches: ["feature/x"] };

function makeLinear(identifier: string): LinearInfo {
  return {
    identifier,
    title: `Ticket ${identifier}`,
    url: `https://linear.app/team/issue/${identifier}`,
    state: { name: "In Progress", color: "#0ea5e9", type: "started" },
    priorityLabel: "High",
    assignee: null,
    project: { id: "proj-1", name: "Dashboard Revamp" },
    prAttachment: null,
  };
}

function makePr(overrides: Partial<PrInfo> = {}): PrInfo {
  return {
    number: 42,
    title: "Test PR",
    url: "https://github.com/test/test/pull/42",
    state: "OPEN",
    isDraft: false,
    reviewDecision: "",
    hasFeedback: false,
    checksStatus: "passing",
    activeCheckUrl: null,
    activeCheckName: null,
    checksWaiting: false,
    ...overrides,
  };
}

const FAILURE: FetchResult<never> = { ok: false, error: "getaddrinfo ENOTFOUND api.linear.app" };

describe("integration-cache", () => {
  let cache: typeof import("../../src/lib/integration-cache.js");
  let db: typeof import("../../src/lib/db.js");

  beforeEach(async () => {
    mockFetchLinearResult.mockReset();
    mockFetchAllPrResults.mockReset();
    mockClearPrBackoff.mockReset();
    db = await import("../../src/lib/db.js");
    cache = await import("../../src/lib/integration-cache.js");
    cache.resetIntegrationCache();
  });

  const linearReturns = (result: unknown) => mockFetchLinearResult.mockResolvedValue(result);
  const prReturns = (result: unknown) =>
    mockFetchAllPrResults.mockResolvedValue(new Map([["feature/x", result]]));

  describe("Linear", () => {
    it("caches a fetched ticket in memory and on disk", async () => {
      linearReturns({ ok: true, value: makeLinear("ENG-1") });
      await cache.refreshLinearCache([REPO], "key");

      expect(cache.integrationFieldsFor("r1", "feature/x").linear_info?.identifier).toBe("ENG-1");
      const row = db.loadIntegrationCache("linear").get("r1:feature/x");
      expect(JSON.parse(row!.payload!).identifier).toBe("ENG-1");
    });

    // The reported bug: one offline poll used to blank every ticket, which collapsed
    // the dashboard's ticket and project grouping.
    it("keeps the cached ticket when the fetch fails", async () => {
      linearReturns({ ok: true, value: makeLinear("ENG-1") });
      await cache.refreshLinearCache([REPO], "key");

      linearReturns(FAILURE);
      await cache.refreshLinearCache([REPO], "key");

      expect(cache.integrationFieldsFor("r1", "feature/x").linear_info?.identifier).toBe("ENG-1");
      const row = db.loadIntegrationCache("linear").get("r1:feature/x");
      expect(JSON.parse(row!.payload!).identifier).toBe("ENG-1");
    });

    it("replaces the cached ticket on a later success", async () => {
      linearReturns({ ok: true, value: makeLinear("ENG-1") });
      await cache.refreshLinearCache([REPO], "key");

      linearReturns({ ok: true, value: makeLinear("ENG-2") });
      await cache.refreshLinearCache([REPO], "key");

      expect(cache.integrationFieldsFor("r1", "feature/x").linear_info?.identifier).toBe("ENG-2");
    });

    it("clears the cached ticket when the API says there is none", async () => {
      linearReturns({ ok: true, value: makeLinear("ENG-1") });
      await cache.refreshLinearCache([REPO], "key");

      linearReturns({ ok: true, value: null });
      await cache.refreshLinearCache([REPO], "key");

      expect(cache.integrationFieldsFor("r1", "feature/x").linear_info).toBeNull();
      expect(db.loadIntegrationCache("linear").get("r1:feature/x")!.payload).toBeNull();
    });

    it("fetches once per distinct branch across repos", async () => {
      linearReturns({ ok: true, value: makeLinear("ENG-1") });
      await cache.refreshLinearCache(
        [REPO, { repoPath: "/other", repoId: "r2", branches: ["feature/x"] }],
        "key"
      );

      expect(mockFetchLinearResult).toHaveBeenCalledTimes(1);
      // …and both repo-scoped keys get the answer.
      expect(cache.integrationFieldsFor("r1", "feature/x").linear_info?.identifier).toBe("ENG-1");
      expect(cache.integrationFieldsFor("r2", "feature/x").linear_info?.identifier).toBe("ENG-1");
    });

    it("does not let one repo's branch shadow another's", async () => {
      mockFetchLinearResult.mockImplementation((branch: string) =>
        Promise.resolve({ ok: true, value: branch === "main" ? makeLinear("ENG-9") : null })
      );
      await cache.refreshLinearCache(
        [
          { repoPath: "/a", repoId: "r1", branches: ["main"] },
          { repoPath: "/b", repoId: "r2", branches: ["feature/y"] },
        ],
        "key"
      );

      expect(cache.integrationFieldsFor("r1", "main").linear_info?.identifier).toBe("ENG-9");
      expect(cache.integrationFieldsFor("r2", "feature/y").linear_info).toBeNull();
    });
  });

  describe("GitHub", () => {
    it("keeps the cached PR when the fetch fails", async () => {
      prReturns({ ok: true, value: makePr() });
      await cache.refreshPrCache([REPO]);
      expect(cache.integrationFieldsFor("r1", "feature/x").pr_info?.number).toBe(42);

      prReturns({ ok: false, error: "in backoff" });
      await cache.refreshPrCache([REPO]);
      expect(cache.integrationFieldsFor("r1", "feature/x").pr_info?.number).toBe(42);
    });

    it("clears the cached PR when GitHub says the branch has none", async () => {
      prReturns({ ok: true, value: makePr() });
      await cache.refreshPrCache([REPO]);

      prReturns({ ok: true, value: null });
      await cache.refreshPrCache([REPO]);
      expect(cache.integrationFieldsFor("r1", "feature/x").pr_info).toBeNull();
    });

    it("reuses a known PR number, but drops it once the PR is terminal", async () => {
      prReturns({ ok: true, value: makePr({ state: "OPEN" }) });
      await cache.refreshPrCache([REPO]);
      expect(mockFetchAllPrResults.mock.calls[0]![2]).toEqual(new Map());

      // Next cycle passes the known number for a cheaper lookup…
      prReturns({ ok: true, value: makePr({ state: "MERGED" }) });
      await cache.refreshPrCache([REPO]);
      expect(mockFetchAllPrResults.mock.calls[1]![2]).toEqual(new Map([["feature/x", 42]]));

      // …but a merged PR drops it, so the next cycle looks the branch up by name and
      // can discover a new PR opened on it.
      prReturns({ ok: true, value: makePr({ state: "MERGED" }) });
      await cache.refreshPrCache([REPO]);
      expect(mockFetchAllPrResults.mock.calls[2]![2]).toEqual(new Map());
    });

    // Otherwise the user reconnects, presses refresh, and the backoff silently
    // swallows it — leaving the "cached" hint up with no way to clear it.
    it("clears the backoff for a refresh the user asked for", async () => {
      prReturns({ ok: true, value: makePr() });
      await cache.refreshPrCache([REPO], { force: true });
      expect(mockClearPrBackoff).toHaveBeenCalledWith("/repo");
    });

    it("leaves the backoff alone for a background poll", async () => {
      prReturns({ ok: true, value: makePr() });
      await cache.refreshPrCache([REPO]);
      expect(mockClearPrBackoff).not.toHaveBeenCalled();
    });

    it("falls back to the PR attached to the Linear ticket", async () => {
      const withAttachment = makeLinear("ENG-1");
      withAttachment.prAttachment = {
        url: "https://github.com/test/test/pull/7",
        title: "Linear-attached PR",
        metadata: { number: 7, draft: false, mergedAt: null, closedAt: null },
      };
      linearReturns({ ok: true, value: withAttachment });
      await cache.refreshLinearCache([REPO], "key");

      const fields = cache.integrationFieldsFor("r1", "feature/x");
      expect(fields.pr_info?.number).toBe(7);
    });
  });

  describe("health", () => {
    it("flags the failing source and clears it on recovery", async () => {
      linearReturns({ ok: true, value: makeLinear("ENG-1") });
      await cache.refreshLinearCache([REPO], "key");
      expect(cache.getIntegrationHealth().linearFailing).toBe(false);

      linearReturns(FAILURE);
      await cache.refreshLinearCache([REPO], "key");
      expect(cache.getIntegrationHealth()).toMatchObject({
        linearFailing: true,
        githubFailing: false,
      });

      linearReturns({ ok: true, value: makeLinear("ENG-1") });
      await cache.refreshLinearCache([REPO], "key");
      expect(cache.getIntegrationHealth().linearFailing).toBe(false);
    });
  });

  describe("persistence", () => {
    // The TUI relaunches Ink on every worktree-open, so a memory-only cache is gone
    // by the time you come back to the list.
    it("serves persisted data on a cold start with no network at all", async () => {
      linearReturns({ ok: true, value: makeLinear("ENG-1") });
      prReturns({ ok: true, value: makePr() });
      await cache.refreshLinearCache([REPO], "key");
      await cache.refreshPrCache([REPO]);

      cache.resetIntegrationCache();
      mockFetchLinearResult.mockReset();
      mockFetchAllPrResults.mockReset();

      cache.hydrateIntegrationCache();
      const fields = cache.integrationFieldsFor("r1", "feature/x");
      expect(fields.linear_info?.identifier).toBe("ENG-1");
      expect(fields.pr_info?.number).toBe(42);
      expect(mockFetchLinearResult).not.toHaveBeenCalled();
      expect(mockFetchAllPrResults).not.toHaveBeenCalled();
    });

    it("hydrates at most once", () => {
      db.setIntegrationCacheEntry("linear", "r1:feature/x", JSON.stringify(makeLinear("ENG-1")));
      cache.hydrateIntegrationCache();
      cache.hydrateIntegrationCache();
      expect(cache.integrationFieldsFor("r1", "feature/x").linear_info?.identifier).toBe("ENG-1");
    });

    it("survives a corrupt payload instead of crashing startup", () => {
      db.setIntegrationCacheEntry("linear", "r1:feature/x", "{{not json");
      expect(() => cache.hydrateIntegrationCache()).not.toThrow();
      expect(cache.integrationFieldsFor("r1", "feature/x").linear_info).toBeNull();
    });

    it("prunes entries whose worktree is gone", async () => {
      linearReturns({ ok: true, value: makeLinear("ENG-1") });
      await cache.refreshLinearCache(
        [{ repoPath: "/repo", repoId: "r1", branches: ["feature/x", "feature/gone"] }],
        "key"
      );
      expect(db.loadIntegrationCache("linear").size).toBe(2);

      await cache.refreshLinearCache([REPO], "key");
      expect([...db.loadIntegrationCache("linear").keys()]).toEqual(["r1:feature/x"]);
    });

    it("does not wipe the cache when the worktree list comes back empty", async () => {
      linearReturns({ ok: true, value: makeLinear("ENG-1") });
      await cache.refreshLinearCache([REPO], "key");

      await cache.refreshLinearCache([{ repoPath: "/repo", repoId: "r1", branches: [] }], "key");
      expect(db.loadIntegrationCache("linear").size).toBe(1);
      expect(cache.integrationFieldsFor("r1", "feature/x").linear_info?.identifier).toBe("ENG-1");
    });

    it("clearCachedLinear forgets tickets without touching PRs", async () => {
      linearReturns({ ok: true, value: makeLinear("ENG-1") });
      prReturns({ ok: true, value: makePr() });
      await cache.refreshLinearCache([REPO], "key");
      await cache.refreshPrCache([REPO]);

      cache.clearCachedLinear();
      expect(cache.integrationFieldsFor("r1", "feature/x").linear_info).toBeNull();
      expect(cache.integrationFieldsFor("r1", "feature/x").pr_info?.number).toBe(42);
      expect(db.loadIntegrationCache("linear").size).toBe(0);
      expect(db.loadIntegrationCache("pr").size).toBe(1);
    });
  });
});
