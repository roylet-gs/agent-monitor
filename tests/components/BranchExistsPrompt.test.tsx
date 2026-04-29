import React from "react";
import { describe, it, expect, vi } from "vitest";
import { render } from "ink-testing-library";
import { BranchExistsPrompt } from "../../src/components/BranchExistsPrompt.js";

const ESCAPE = "";
const ENTER = "\r";

function waitForFrame(ms = 150): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

function defaultHandlers() {
  return {
    onReuseLocal: vi.fn(),
    onPullRemote: vi.fn(),
    onCreateDisconnected: vi.fn(),
    onDeleteAndRecreate: vi.fn(),
    onCancel: vi.fn(),
  };
}

describe("BranchExistsPrompt", () => {
  describe("local-only branch", () => {
    it("renders local header and reuse / delete options", () => {
      const h = defaultHandlers();
      const { lastFrame } = render(
        <BranchExistsPrompt
          branchName="feature/test"
          localExists={true}
          remoteExists={false}
          {...h}
        />
      );
      const frame = lastFrame()!;
      expect(frame).toContain('Branch "feature/test" already exists locally');
      expect(frame).toContain("[Enter/r]");
      expect(frame).toContain("Reuse existing local branch");
      expect(frame).toContain("[d]");
      expect(frame).toContain("Delete local branch");
      expect(frame).toContain("[Esc/n]");
      // Remote-specific options should not appear
      expect(frame).not.toContain("Pull remote");
      expect(frame).not.toContain("Create disconnected");
    });

    it("Enter calls onReuseLocal", async () => {
      const h = defaultHandlers();
      const { stdin } = render(
        <BranchExistsPrompt
          branchName="feature/test"
          localExists={true}
          remoteExists={false}
          {...h}
        />
      );
      await waitForFrame(100);
      stdin.write(ENTER);
      await waitForFrame();
      expect(h.onReuseLocal).toHaveBeenCalled();
    });

    it("d calls onDeleteAndRecreate", async () => {
      const h = defaultHandlers();
      const { stdin } = render(
        <BranchExistsPrompt
          branchName="feature/test"
          localExists={true}
          remoteExists={false}
          {...h}
        />
      );
      await waitForFrame(100);
      stdin.write("d");
      await waitForFrame();
      expect(h.onDeleteAndRecreate).toHaveBeenCalled();
    });

    it("Escape calls onCancel", async () => {
      const h = defaultHandlers();
      const { stdin } = render(
        <BranchExistsPrompt
          branchName="feature/test"
          localExists={true}
          remoteExists={false}
          {...h}
        />
      );
      await waitForFrame(100);
      stdin.write(ESCAPE);
      await waitForFrame();
      expect(h.onCancel).toHaveBeenCalled();
    });
  });

  describe("remote-only branch", () => {
    it("renders remote header and pull / disconnected options", () => {
      const h = defaultHandlers();
      const { lastFrame } = render(
        <BranchExistsPrompt
          branchName="feature/test"
          localExists={false}
          remoteExists={true}
          {...h}
        />
      );
      const frame = lastFrame()!;
      expect(frame).toContain('Branch "feature/test" exists on origin');
      expect(frame).toContain("[Enter/p]");
      expect(frame).toContain("Pull remote");
      expect(frame).toContain("track origin/feature/test");
      expect(frame).toContain("[c]");
      expect(frame).toContain("Create disconnected");
      // Should NOT show the destructive delete-and-recreate path here
      expect(frame).not.toContain("Reuse existing local branch");
      expect(frame).not.toContain("Delete local branch");
    });

    it("Enter calls onPullRemote", async () => {
      const h = defaultHandlers();
      const { stdin } = render(
        <BranchExistsPrompt
          branchName="feature/test"
          localExists={false}
          remoteExists={true}
          {...h}
        />
      );
      await waitForFrame(100);
      stdin.write(ENTER);
      await waitForFrame();
      expect(h.onPullRemote).toHaveBeenCalled();
    });

    it("c calls onCreateDisconnected", async () => {
      const h = defaultHandlers();
      const { stdin } = render(
        <BranchExistsPrompt
          branchName="feature/test"
          localExists={false}
          remoteExists={true}
          {...h}
        />
      );
      await waitForFrame(100);
      stdin.write("c");
      await waitForFrame();
      expect(h.onCreateDisconnected).toHaveBeenCalled();
    });
  });

  describe("branch exists locally and remotely", () => {
    it("renders combined header with disconnected warning", () => {
      const h = defaultHandlers();
      const { lastFrame } = render(
        <BranchExistsPrompt
          branchName="feature/test"
          localExists={true}
          remoteExists={true}
          {...h}
        />
      );
      const frame = lastFrame()!;
      expect(frame).toContain('Branch "feature/test" exists locally and on origin');
      expect(frame).toContain("Pull remote (reset local to origin)");
      expect(frame).toContain("Create disconnected (deletes local, no tracking)");
    });

    it("c calls onCreateDisconnected when local also exists", async () => {
      const h = defaultHandlers();
      const { stdin } = render(
        <BranchExistsPrompt
          branchName="feature/test"
          localExists={true}
          remoteExists={true}
          {...h}
        />
      );
      await waitForFrame(100);
      stdin.write("c");
      await waitForFrame();
      expect(h.onCreateDisconnected).toHaveBeenCalled();
    });
  });

  it("ignores input during initial ready delay", () => {
    const h = defaultHandlers();
    const { stdin } = render(
      <BranchExistsPrompt
        branchName="feature/test"
        localExists={true}
        remoteExists={false}
        {...h}
      />
    );
    // Fire immediately without waiting for ready delay
    stdin.write(ENTER);
    stdin.write("d");
    stdin.write(ESCAPE);
    expect(h.onReuseLocal).not.toHaveBeenCalled();
    expect(h.onDeleteAndRecreate).not.toHaveBeenCalled();
    expect(h.onCancel).not.toHaveBeenCalled();
  });
});
