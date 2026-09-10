import React from "react";
import { describe, it, expect, vi } from "vitest";
import { render } from "ink-testing-library";
import { Text } from "ink";
import { Dashboard } from "../../src/components/Dashboard.js";
import type { IntegrationHealth } from "../../src/lib/types.js";

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

describe("Dashboard cached-data hint", () => {
  const health = (over: Partial<IntegrationHealth> = {}): IntegrationHealth => ({
    githubFailing: false,
    linearFailing: false,
    lastGithubError: null,
    lastLinearError: null,
    ...over,
  });

  it("says nothing when both integrations are healthy", () => {
    const { lastFrame } = render(<Dashboard {...BASE_PROPS} integrationHealth={health()} />);
    expect(lastFrame()!).not.toContain("cached");
  });

  it("names Linear when only Linear is failing", () => {
    const { lastFrame } = render(
      <Dashboard {...BASE_PROPS} integrationHealth={health({ linearFailing: true })} />
    );
    const frame = lastFrame()!;
    expect(frame).toContain("Linear offline");
    expect(frame).not.toContain("GitHub offline");
  });

  it("names GitHub when only GitHub is failing", () => {
    const { lastFrame } = render(
      <Dashboard {...BASE_PROPS} integrationHealth={health({ githubFailing: true })} />
    );
    const frame = lastFrame()!;
    expect(frame).toContain("GitHub offline");
    expect(frame).not.toContain("Linear offline");
  });

  it("names neither when both are failing", () => {
    const { lastFrame } = render(
      <Dashboard
        {...BASE_PROPS}
        integrationHealth={health({ githubFailing: true, linearFailing: true })}
      />
    );
    const frame = lastFrame()!;
    expect(frame).toContain("offline");
    expect(frame).toContain("cached");
    expect(frame).not.toContain("GitHub offline");
    expect(frame).not.toContain("Linear offline");
  });

  it("says nothing when health is unknown", () => {
    const { lastFrame } = render(<Dashboard {...BASE_PROPS} integrationHealth={null} />);
    expect(lastFrame()!).not.toContain("cached");
  });
});
