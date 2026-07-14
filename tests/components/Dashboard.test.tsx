import React from "react";
import { describe, it, expect, vi } from "vitest";
import { render } from "ink-testing-library";
import { Text } from "ink";
import { Dashboard } from "../../src/components/Dashboard.js";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

// Wide terminal so the right pane (detail / chat) is shown
vi.mock("../../src/hooks/useTerminalSize.js", () => ({
  useTerminalSize: () => ({ columns: 120, rows: 40 }),
}));

const BASE_PROPS = {
  repoName: "test-repo",
  groups: [],
  flatWorktrees: [],
  standaloneSessions: [],
  selectedIndex: 0,
  busy: null,
  escHint: false,
  unseenIds: new Set<string>(),
  compactView: false,
  showLogs: false,
  terminalRows: 40,
};

describe("Dashboard chat pane", () => {
  it("shows dashboard keys and no chat pane by default", () => {
    const { lastFrame } = render(<Dashboard {...BASE_PROPS} />);
    const frame = lastFrame()!;
    expect(frame).toContain("[n]");
    expect(frame).not.toContain("CHAT-PANE-CONTENT");
  });

  it("renders the chat pane in place of the detail panel with chat keys", () => {
    const { lastFrame } = render(
      <Dashboard {...BASE_PROPS} chatPane={<Text>CHAT-PANE-CONTENT</Text>} />
    );
    const frame = lastFrame()!;
    expect(frame).toContain("CHAT-PANE-CONTENT");
    expect(frame).toContain("Send");
    expect(frame).toContain("[Esc]");
    // dashboard-only hints are replaced
    expect(frame).not.toContain("[n]ew");
  });
});
