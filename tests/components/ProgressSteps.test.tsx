import React from "react";
import { describe, it, expect, vi } from "vitest";
import { render } from "ink-testing-library";
import { Text } from "ink";
import { ProgressSteps } from "../../src/components/ProgressSteps.js";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

describe("ProgressSteps", () => {
  it("renders title, subtitle, and steps", () => {
    const { lastFrame } = render(
      <ProgressSteps
        title="Test Title"
        subtitle="Test subtitle"
        steps={[
          { label: "Step one", status: "done" },
          { label: "Step two", status: "active" },
          { label: "Step three", status: "pending" },
        ]}
      />
    );
    const frame = lastFrame()!;
    expect(frame).toContain("Test Title");
    expect(frame).toContain("Test subtitle");
    expect(frame).toContain("Step one");
    expect(frame).toContain("Step two");
    expect(frame).toContain("Step three");
  });

  it("renders error message when provided", () => {
    const { lastFrame } = render(
      <ProgressSteps
        title="Delete"
        subtitle="Deleting..."
        steps={[{ label: "Remove", status: "error" }]}
        error="Something went wrong"
      />
    );
    expect(lastFrame()!).toContain("Something went wrong");
  });

  it("does not render prompt when not provided", () => {
    const { lastFrame } = render(
      <ProgressSteps
        title="Delete"
        subtitle="Deleting..."
        steps={[{ label: "Remove", status: "done" }]}
      />
    );
    expect(lastFrame()!).not.toContain("Clean up");
  });

  it("renders prompt when provided", () => {
    const { lastFrame } = render(
      <ProgressSteps
        title="Delete"
        subtitle="Deleting..."
        steps={[{ label: "Remove", status: "error" }]}
        error="Failed"
        prompt={<Text>Clean up database entry? [y/n]</Text>}
      />
    );
    const frame = lastFrame()!;
    expect(frame).toContain("Failed");
    expect(frame).toContain("Clean up database entry?");
  });
});
