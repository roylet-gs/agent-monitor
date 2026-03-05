/**
 * Test helper: mock process.exit to throw instead of killing the test runner.
 */
import { vi, type MockInstance } from "vitest";

export class ProcessExitError extends Error {
  code: number;
  constructor(code: number) {
    super(`process.exit(${code})`);
    this.name = "ProcessExitError";
    this.code = code;
  }
}

export function mockProcessExit(): MockInstance {
  return vi.spyOn(process, "exit").mockImplementation((code?: number | string) => {
    throw new ProcessExitError(typeof code === "number" ? code : 1);
  });
}
