import { describe, it, expect, beforeEach } from "vitest";
import { captureConsole, type ConsoleSpy } from "../helpers/console-capture.js";

let spy: ConsoleSpy;

beforeEach(() => {
  spy = captureConsole();
});

describe("outputJson", () => {
  it("outputs formatted JSON", async () => {
    const { outputJson } = await import("../../src/lib/output.js");
    outputJson({ foo: "bar", count: 42 });
    expect(spy.getLog()).toBe(JSON.stringify({ foo: "bar", count: 42 }, null, 2));
  });

  it("handles arrays", async () => {
    const { outputJson } = await import("../../src/lib/output.js");
    outputJson([1, 2, 3]);
    expect(JSON.parse(spy.getLog())).toEqual([1, 2, 3]);
  });

  it("handles null", async () => {
    const { outputJson } = await import("../../src/lib/output.js");
    outputJson(null);
    expect(spy.getLog()).toBe("null");
  });
});

describe("outputTable", () => {
  it("renders table with headers and rows", async () => {
    const { outputTable } = await import("../../src/lib/output.js");
    outputTable(
      [{ name: "Alice", age: "30" }, { name: "Bob", age: "25" }],
      [
        { key: "name", header: "Name" },
        { key: "age", header: "Age" },
      ]
    );
    const lines = spy.getLogLines();
    expect(lines[0]).toContain("Name");
    expect(lines[0]).toContain("Age");
    // Separator line
    expect(lines[1]).toMatch(/─+/);
    // Data rows
    expect(lines[2]).toContain("Alice");
    expect(lines[3]).toContain("Bob");
  });

  it("shows 'No results.' for empty rows", async () => {
    const { outputTable } = await import("../../src/lib/output.js");
    outputTable([], [{ key: "name", header: "Name" }]);
    expect(spy.getLog()).toBe("No results.");
  });

  it("respects right alignment", async () => {
    const { outputTable } = await import("../../src/lib/output.js");
    outputTable(
      [{ num: "42" }],
      [{ key: "num", header: "Number", width: 10, align: "right" }]
    );
    const lines = spy.getLogLines();
    // Data should be right-padded
    expect(lines[2]).toMatch(/\s+42/);
  });

  it("truncates long values with ellipsis", async () => {
    const { outputTable } = await import("../../src/lib/output.js");
    outputTable(
      [{ name: "A very long name that exceeds width" }],
      [{ key: "name", header: "Name", width: 10 }]
    );
    const lines = spy.getLogLines();
    expect(lines[2]!.trim().length).toBeLessThanOrEqual(10);
    expect(lines[2]).toContain("…");
  });

  it("handles missing keys with empty string", async () => {
    const { outputTable } = await import("../../src/lib/output.js");
    outputTable(
      [{ name: "Alice" }],
      [
        { key: "name", header: "Name" },
        { key: "missing", header: "Missing" },
      ]
    );
    const lines = spy.getLogLines();
    expect(lines[2]).toContain("Alice");
  });
});

describe("outputKeyValue", () => {
  it("aligns keys and values", async () => {
    const { outputKeyValue } = await import("../../src/lib/output.js");
    outputKeyValue([
      ["name", "Alice"],
      ["age", "30"],
      ["location", "Wonderland"],
    ]);
    const lines = spy.getLogLines();
    expect(lines).toHaveLength(3);
    // All key fields should be same width (padded to "location".length = 8)
    expect(lines[0]).toMatch(/^name\s+Alice$/);
    expect(lines[2]).toMatch(/^location\s+Wonderland$/);
  });
});
