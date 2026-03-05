/**
 * Test helper: capture console.log and console.error output.
 */
import { vi, type MockInstance } from "vitest";

export interface ConsoleSpy {
  log: MockInstance;
  error: MockInstance;
  getLog(): string;
  getError(): string;
  getLogLines(): string[];
  getErrorLines(): string[];
  restore(): void;
}

export function captureConsole(): ConsoleSpy {
  const logSpy = vi.spyOn(console, "log").mockImplementation(() => {});
  const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});

  return {
    log: logSpy,
    error: errorSpy,
    getLog(): string {
      return logSpy.mock.calls.map((args) => args.join(" ")).join("\n");
    },
    getError(): string {
      return errorSpy.mock.calls.map((args) => args.join(" ")).join("\n");
    },
    getLogLines(): string[] {
      return logSpy.mock.calls.map((args) => args.join(" "));
    },
    getErrorLines(): string[] {
      return errorSpy.mock.calls.map((args) => args.join(" "));
    },
    restore(): void {
      logSpy.mockRestore();
      errorSpy.mockRestore();
    },
  };
}
