import React, { useState, useCallback } from "react";
import { Box, useStdout } from "ink";
import { TerminalPane } from "./TerminalPane.js";
import { TerminalTabBar } from "./TerminalTabBar.js";
import { TerminalActionBar } from "./TerminalActionBar.js";
import { RoleSelector } from "./RoleSelector.js";
import { useTerminalInput } from "../hooks/useTerminalInput.js";
import type { PaneState } from "../hooks/useTerminalPanes.js";

interface TerminalViewProps {
  panes: PaneState[];
  onAddPane: (role?: string, roleContent?: string) => void;
  onRemovePane: (id: string) => void;
  onFocusNext: () => void;
  onFocusPrev: () => void;
  onDetach: () => void;
  getFocusedPane: () => PaneState | undefined;
}

export function TerminalView({
  panes,
  onAddPane,
  onRemovePane,
  onFocusNext,
  onFocusPrev,
  onDetach,
  getFocusedPane,
}: TerminalViewProps) {
  const { stdout } = useStdout();
  const totalCols = stdout?.columns ?? 80;
  const totalRows = stdout?.rows ?? 24;
  const [showHelp, setShowHelp] = useState(false);
  const [zoomed, setZoomed] = useState(false);
  const [showRoleSelector, setShowRoleSelector] = useState(false);

  const handleNewPane = useCallback(() => {
    setShowRoleSelector(true);
  }, []);

  const handleClosePane = useCallback(() => {
    const focused = getFocusedPane();
    if (focused) {
      onRemovePane(focused.id);
    }
  }, [getFocusedPane, onRemovePane]);

  const handleToggleZoom = useCallback(() => {
    setZoomed((z) => !z);
  }, []);

  const handleToggleHelp = useCallback(() => {
    setShowHelp((h) => !h);
  }, []);

  const { mode: inputMode } = useTerminalInput({
    active: !showRoleSelector,
    panes,
    onNewPane: handleNewPane,
    onClosePane: handleClosePane,
    onFocusNext,
    onFocusPrev,
    onDetach,
    onToggleZoom: handleToggleZoom,
    onToggleHelp: handleToggleHelp,
  });

  if (showRoleSelector) {
    return (
      <Box flexDirection="column" height={totalRows}>
        <RoleSelector
          onSelect={(roleName, roleContent) => {
            setShowRoleSelector(false);
            onAddPane(roleName ?? undefined, roleContent ?? undefined);
          }}
          onCancel={() => setShowRoleSelector(false)}
        />
      </Box>
    );
  }

  // Calculate pane dimensions
  const actionBarHeight = showHelp ? 9 : 1;
  const tabBarHeight = 1;
  const paneAreaHeight = totalRows - tabBarHeight - actionBarHeight;

  // In zoomed mode, only show focused pane
  const visiblePanes = zoomed
    ? panes.filter((p) => p.focused)
    : panes;

  const paneWidth = visiblePanes.length > 0
    ? Math.floor(totalCols / visiblePanes.length)
    : totalCols;

  return (
    <Box flexDirection="column" height={totalRows}>
      <TerminalTabBar panes={panes} />
      <Box flexDirection="row" height={paneAreaHeight}>
        {visiblePanes.map((pane) => (
          <TerminalPane
            key={pane.id}
            pane={pane}
            width={paneWidth}
            height={paneAreaHeight}
          />
        ))}
      </Box>
      <TerminalActionBar
        commandMode={inputMode === "command"}
        showHelp={showHelp}
      />
    </Box>
  );
}
