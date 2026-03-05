import React from "react";
import { describe, it, expect, vi } from "vitest";
import { render } from "ink-testing-library";
import { StatusBar } from "../../src/components/StatusBar.js";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

describe("StatusBar", () => {
  it("renders repo name and worktree count", () => {
    const { lastFrame } = render(
      <StatusBar repoName="my-repo" worktreeCount={3} />
    );
    const frame = lastFrame()!;
    expect(frame).toContain("Agent Monitor");
    expect(frame).toContain("my-repo");
    expect(frame).toContain("3 worktrees");
  });

  it("shows singular worktree", () => {
    const { lastFrame } = render(
      <StatusBar repoName="my-repo" worktreeCount={1} />
    );
    expect(lastFrame()!).toContain("1 worktree");
    expect(lastFrame()!).not.toContain("1 worktrees");
  });

  it("shows repo count when multiple repos", () => {
    const { lastFrame } = render(
      <StatusBar repoName="my-repo" worktreeCount={5} repoCount={3} />
    );
    expect(lastFrame()!).toContain("3 repos");
  });

  it("shows version when provided", () => {
    const { lastFrame } = render(
      <StatusBar repoName="my-repo" worktreeCount={1} version="1.2.3" />
    );
    expect(lastFrame()!).toContain("v1.2.3");
  });

  it("shows update available notification", () => {
    const { lastFrame } = render(
      <StatusBar
        repoName="my-repo"
        worktreeCount={1}
        updateInfo={{ current: "1.0.0", latest: "2.0.0", updateAvailable: true }}
      />
    );
    const frame = lastFrame()!;
    expect(frame).toContain("v2.0.0");
    expect(frame).toContain("available");
  });

  it("does not show update when not available", () => {
    const { lastFrame } = render(
      <StatusBar
        repoName="my-repo"
        worktreeCount={1}
        updateInfo={{ current: "2.0.0", latest: "2.0.0", updateAvailable: false }}
      />
    );
    expect(lastFrame()!).not.toContain("available");
  });
});
