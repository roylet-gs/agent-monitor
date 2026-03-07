import { useEffect, useRef, useState, useCallback } from "react";
import { writeToPty } from "../lib/pty-manager.js";
import type { PaneState } from "./useTerminalPanes.js";

type InputMode = "normal" | "command";

interface UseTerminalInputOptions {
  active: boolean;
  panes: PaneState[];
  onNewPane: () => void;
  onClosePane: () => void;
  onFocusNext: () => void;
  onFocusPrev: () => void;
  onDetach: () => void;
  onToggleZoom: () => void;
  onToggleHelp: () => void;
}

const CTRL_A = "\x01";

export function useTerminalInput({
  active,
  panes,
  onNewPane,
  onClosePane,
  onFocusNext,
  onFocusPrev,
  onDetach,
  onToggleZoom,
  onToggleHelp,
}: UseTerminalInputOptions): { mode: InputMode } {
  const [mode, setMode] = useState<InputMode>("normal");
  const modeRef = useRef<InputMode>("normal");
  const panesRef = useRef(panes);

  useEffect(() => {
    panesRef.current = panes;
  }, [panes]);

  useEffect(() => {
    modeRef.current = mode;
  }, [mode]);

  const handleData = useCallback(
    (data: Buffer) => {
      const str = data.toString("utf-8");

      if (modeRef.current === "command") {
        setMode("normal");
        modeRef.current = "normal";

        switch (str) {
          case "c":
            onNewPane();
            return;
          case "x":
            onClosePane();
            return;
          case "h":
          case "\x1b[D": // left arrow
            onFocusPrev();
            return;
          case "l":
          case "\x1b[C": // right arrow
            onFocusNext();
            return;
          case "d":
            onDetach();
            return;
          case "z":
            onToggleZoom();
            return;
          case "?":
            onToggleHelp();
            return;
          case CTRL_A:
            // Double Ctrl+A sends literal Ctrl+A to the PTY
            const focused = panesRef.current.find((p) => p.focused);
            if (focused && !focused.exited) {
              writeToPty(focused.ptyInstance, CTRL_A);
            }
            return;
          default:
            // Unknown command key - ignore
            return;
        }
      }

      // Normal mode
      if (str === CTRL_A) {
        setMode("command");
        modeRef.current = "command";
        return;
      }

      // Forward input to focused PTY
      const focused = panesRef.current.find((p) => p.focused);
      if (focused && !focused.exited) {
        writeToPty(focused.ptyInstance, str);
      }
    },
    [onNewPane, onClosePane, onFocusNext, onFocusPrev, onDetach, onToggleZoom, onToggleHelp],
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
    process.stdin.on("data", handleData);

    return () => {
      process.stdin.removeListener("data", handleData);
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
