import { useEffect, useRef, useState, useCallback } from "react";
import { writeToPty } from "../lib/pty-manager.js";
import { enableMouse, disableMouse, parseMouseEvent } from "./useMouse.js";
import type { PaneState } from "./useTerminalPanes.js";

type InputMode = "normal" | "command";

interface UseTerminalInputOptions {
  active: boolean;
  panes: PaneState[];
  visiblePaneIds: string[];
  totalCols: number;
  tabBarHeight: number;
  onNewPane: () => void;
  onClosePane: () => void;
  onFocusNext: () => void;
  onFocusPrev: () => void;
  onFocusPane: (id: string) => void;
  onFocusPaneByIndex: (index: number) => void;
  onDetach: () => void;
  onToggleZoom: () => void;
  onToggleHelp: () => void;
}

const ESC = "\x1b";
const ESC_TIMEOUT_MS = 50;

export function useTerminalInput({
  active,
  panes,
  visiblePaneIds,
  totalCols,
  tabBarHeight,
  onNewPane,
  onClosePane,
  onFocusNext,
  onFocusPrev,
  onFocusPane,
  onFocusPaneByIndex,
  onDetach,
  onToggleZoom,
  onToggleHelp,
}: UseTerminalInputOptions): { mode: InputMode } {
  const [mode, setMode] = useState<InputMode>("normal");
  const modeRef = useRef<InputMode>("normal");
  const panesRef = useRef(panes);
  const visiblePaneIdsRef = useRef(visiblePaneIds);
  const totalColsRef = useRef(totalCols);
  const tabBarHeightRef = useRef(tabBarHeight);
  const escBufferRef = useRef<string | null>(null);
  const escTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Store all callbacks in refs so handleData never changes
  const onNewPaneRef = useRef(onNewPane);
  const onClosePaneRef = useRef(onClosePane);
  const onFocusNextRef = useRef(onFocusNext);
  const onFocusPrevRef = useRef(onFocusPrev);
  const onFocusPaneRef = useRef(onFocusPane);
  const onFocusPaneByIndexRef = useRef(onFocusPaneByIndex);
  const onDetachRef = useRef(onDetach);
  const onToggleZoomRef = useRef(onToggleZoom);
  const onToggleHelpRef = useRef(onToggleHelp);

  useEffect(() => { panesRef.current = panes; }, [panes]);
  useEffect(() => { visiblePaneIdsRef.current = visiblePaneIds; }, [visiblePaneIds]);
  useEffect(() => { totalColsRef.current = totalCols; }, [totalCols]);
  useEffect(() => { tabBarHeightRef.current = tabBarHeight; }, [tabBarHeight]);
  useEffect(() => { modeRef.current = mode; }, [mode]);

  // Keep callback refs up to date
  useEffect(() => { onNewPaneRef.current = onNewPane; }, [onNewPane]);
  useEffect(() => { onClosePaneRef.current = onClosePane; }, [onClosePane]);
  useEffect(() => { onFocusNextRef.current = onFocusNext; }, [onFocusNext]);
  useEffect(() => { onFocusPrevRef.current = onFocusPrev; }, [onFocusPrev]);
  useEffect(() => { onFocusPaneRef.current = onFocusPane; }, [onFocusPane]);
  useEffect(() => { onFocusPaneByIndexRef.current = onFocusPaneByIndex; }, [onFocusPaneByIndex]);
  useEffect(() => { onDetachRef.current = onDetach; }, [onDetach]);
  useEffect(() => { onToggleZoomRef.current = onToggleZoom; }, [onToggleZoom]);
  useEffect(() => { onToggleHelpRef.current = onToggleHelp; }, [onToggleHelp]);

  const handleBareEscape = useCallback(() => {
    if (modeRef.current === "command") {
      // Double Escape — detach back to dashboard
      setMode("normal");
      modeRef.current = "normal";
      onDetachRef.current();
    } else {
      setMode("command");
      modeRef.current = "command";
    }
  }, []);

  // handleData has NO deps that change — all accessed via refs
  const handleData = useCallback(
    (data: Buffer) => {
      let str = data.toString("utf-8");

      // If we have a pending escape buffer, concatenate and clear timer
      if (escBufferRef.current !== null) {
        if (escTimerRef.current) {
          clearTimeout(escTimerRef.current);
          escTimerRef.current = null;
        }
        str = escBufferRef.current + str;
        escBufferRef.current = null;
      }

      // Check for mouse events first (works in both modes)
      const mouseResult = parseMouseEvent(str);
      if (mouseResult) {
        const { event } = mouseResult;
        if (event.type === "press" && event.button === 0) {
          // Left click — determine which pane was clicked
          const ids = visiblePaneIdsRef.current;
          if (ids.length > 0 && event.y >= tabBarHeightRef.current) {
            const paneWidth = Math.floor(totalColsRef.current / ids.length);
            const paneIndex = Math.min(Math.floor(event.x / paneWidth), ids.length - 1);
            const targetId = ids[paneIndex];
            if (targetId) {
              onFocusPaneRef.current(targetId);
            }
          }
        }
        return;
      }

      // If data is exactly \x1b, buffer it and start timeout
      if (str === ESC) {
        escBufferRef.current = str;
        escTimerRef.current = setTimeout(() => {
          escBufferRef.current = null;
          escTimerRef.current = null;
          // Bare Escape detected — toggle command mode
          handleBareEscape();
        }, ESC_TIMEOUT_MS);
        return;
      }

      // Command mode: handle single-key commands
      if (modeRef.current === "command") {
        setMode("normal");
        modeRef.current = "normal";

        switch (str.toLowerCase()) {
          case "c":
            onNewPaneRef.current();
            return;
          case "w":
            onClosePaneRef.current();
            return;
          case "h":
          case "\x1b[D": // left arrow
            onFocusPrevRef.current();
            return;
          case "l":
          case "\x1b[C": // right arrow
            onFocusNextRef.current();
            return;
          case "d":
            onDetachRef.current();
            return;
          case "z":
            onToggleZoomRef.current();
            return;
          case "?":
            onToggleHelpRef.current();
            return;
          case "1":
          case "2":
          case "3":
          case "4":
            onFocusPaneByIndexRef.current(parseInt(str, 10) - 1);
            return;
          default:
            // Unknown command key — ignore, already back to normal
            return;
        }
      }

      // Normal mode: forward input to focused PTY
      const focused = panesRef.current.find((p) => p.focused);
      if (focused && !focused.exited) {
        writeToPty(focused.ptyInstance, str);
      }
    },
    [handleBareEscape], // stable — handleBareEscape has no deps
  );

  useEffect(() => {
    if (!active) {
      setMode("normal");
      return;
    }

    // Set stdin to raw mode for byte-level input
    if (process.stdin.isTTY) {
      process.stdin.setRawMode(true);
    }
    process.stdin.resume();
    enableMouse();
    process.stdin.on("data", handleData);

    return () => {
      process.stdin.removeListener("data", handleData);
      disableMouse();
      // Clean up any pending escape timer
      if (escTimerRef.current) {
        clearTimeout(escTimerRef.current);
        escTimerRef.current = null;
      }
      escBufferRef.current = null;
    };
  }, [active, handleData]);

  // Reset mode when going inactive
  useEffect(() => {
    if (!active) {
      setMode("normal");
      modeRef.current = "normal";
    }
  }, [active]);

  return { mode };
}
