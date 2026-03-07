import { useState, useCallback, useEffect, useRef } from "react";
import {
  spawnPty,
  destroyPty,
  writeToTerminal,
  getScreenLines,
  type PtyInstance,
} from "../lib/pty-manager.js";
import { log } from "../lib/logger.js";

export interface PaneState {
  id: string;
  ptyInstance: PtyInstance;
  lines: string[];
  focused: boolean;
  title: string;
  exited: boolean;
  exitCode?: number;
  worktreeId: string;
  role?: string;
}

interface UseTerminalPanesReturn {
  panes: PaneState[];
  addPane: (cwd: string, worktreeId: string, title: string, role?: string, roleContent?: string) => void;
  removePane: (id: string) => void;
  focusPane: (id: string) => void;
  focusPaneByIndex: (index: number) => void;
  focusNext: () => void;
  focusPrev: () => void;
  getFocusedPane: () => PaneState | undefined;
  destroyAll: () => void;
}

export function useTerminalPanes(cols: number, rows: number): UseTerminalPanesReturn {
  const [panes, setPanes] = useState<PaneState[]>([]);
  const panesRef = useRef<PaneState[]>([]);
  const updateTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingUpdateRef = useRef(false);

  // Keep ref in sync
  useEffect(() => {
    panesRef.current = panes;
  }, [panes]);

  // Throttled state update
  const scheduleUpdate = useCallback(() => {
    if (updateTimerRef.current) {
      pendingUpdateRef.current = true;
      return;
    }
    setPanes([...panesRef.current]);
    updateTimerRef.current = setTimeout(() => {
      updateTimerRef.current = null;
      if (pendingUpdateRef.current) {
        pendingUpdateRef.current = false;
        setPanes([...panesRef.current]);
      }
    }, 100);
  }, []);

  const addPane = useCallback(
    (cwd: string, worktreeId: string, title: string, role?: string, roleContent?: string) => {
      if (panesRef.current.length >= 4) {
        log("warn", "terminal", "Maximum 4 panes reached");
        return;
      }

      const paneRows = Math.max(rows - 4, 10); // account for tab bar + action bar
      const paneCols = Math.max(Math.floor(cols / (panesRef.current.length + 1)), 20);

      const instance = spawnPty(cwd, paneCols, paneRows, roleContent);
      if (role) instance.role = role;

      const newPane: PaneState = {
        id: instance.id,
        ptyInstance: instance,
        lines: [],
        focused: true,
        title,
        exited: false,
        worktreeId,
        role,
      };

      // Subscribe to PTY output
      instance.pty.onData((data: string) => {
        writeToTerminal(instance, data);
        const pane = panesRef.current.find((p) => p.id === instance.id);
        if (pane) {
          pane.lines = getScreenLines(instance);
          scheduleUpdate();
        }
      });

      instance.pty.onExit(({ exitCode }) => {
        log("info", "terminal", `PTY ${instance.id} exited with code ${exitCode}`);
        const pane = panesRef.current.find((p) => p.id === instance.id);
        if (pane) {
          pane.exited = true;
          pane.exitCode = exitCode;
          scheduleUpdate();
        }
      });

      // Unfocus all other panes
      for (const p of panesRef.current) {
        p.focused = false;
      }

      panesRef.current = [...panesRef.current, newPane];
      setPanes([...panesRef.current]);
    },
    [cols, rows, scheduleUpdate],
  );

  const removePane = useCallback((id: string) => {
    const pane = panesRef.current.find((p) => p.id === id);
    if (!pane) return;

    destroyPty(pane.ptyInstance);
    const wasFocused = pane.focused;
    panesRef.current = panesRef.current.filter((p) => p.id !== id);

    if (wasFocused && panesRef.current.length > 0) {
      panesRef.current[0].focused = true;
    }

    setPanes([...panesRef.current]);
  }, []);

  const focusPane = useCallback((id: string) => {
    for (const p of panesRef.current) {
      p.focused = p.id === id;
    }
    setPanes([...panesRef.current]);
  }, []);

  const focusPaneByIndex = useCallback((index: number) => {
    if (index >= 0 && index < panesRef.current.length) {
      for (let i = 0; i < panesRef.current.length; i++) {
        panesRef.current[i].focused = i === index;
      }
      setPanes([...panesRef.current]);
    }
  }, []);

  const focusNext = useCallback(() => {
    const currentIdx = panesRef.current.findIndex((p) => p.focused);
    if (currentIdx < 0 || panesRef.current.length <= 1) return;
    const nextIdx = (currentIdx + 1) % panesRef.current.length;
    for (let i = 0; i < panesRef.current.length; i++) {
      panesRef.current[i].focused = i === nextIdx;
    }
    setPanes([...panesRef.current]);
  }, []);

  const focusPrev = useCallback(() => {
    const currentIdx = panesRef.current.findIndex((p) => p.focused);
    if (currentIdx < 0 || panesRef.current.length <= 1) return;
    const prevIdx = (currentIdx - 1 + panesRef.current.length) % panesRef.current.length;
    for (let i = 0; i < panesRef.current.length; i++) {
      panesRef.current[i].focused = i === prevIdx;
    }
    setPanes([...panesRef.current]);
  }, []);

  const getFocusedPane = useCallback(() => {
    return panesRef.current.find((p) => p.focused);
  }, []);

  const destroyAll = useCallback(() => {
    for (const p of panesRef.current) {
      destroyPty(p.ptyInstance);
    }
    panesRef.current = [];
    setPanes([]);
  }, []);

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      if (updateTimerRef.current) {
        clearTimeout(updateTimerRef.current);
      }
    };
  }, []);

  return {
    panes,
    addPane,
    removePane,
    focusPane,
    focusPaneByIndex,
    focusNext,
    focusPrev,
    getFocusedPane,
    destroyAll,
  };
}
