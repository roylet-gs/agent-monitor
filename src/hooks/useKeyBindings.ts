import { useInput } from "ink";
import { useRef } from "react";
import type { AppMode } from "../lib/types.js";

interface KeyBindingActions {
  selectedIndex: number;
  worktreeCount: number;
  mode: AppMode;
  busy: string | null;
  onSelect: (index: number) => void;
  onEnter: () => void;
  onNew: () => void;
  onDelete: () => void;
  onSettings: () => void;
  onRefresh: () => void;
  onOpenPr: () => void;
  onOpenLinear: () => void;
  onOpenTerminal: () => void;
  onToggleLogs: () => void;
  onUpdate?: () => void;
  onClaude: () => void;
  onQuit: () => void;
  onEscHint: (show: boolean) => void;
}

const ESC_DOUBLE_TAP_MS = 500;

export function useKeyBindings(actions: KeyBindingActions): void {
  const lastEscRef = useRef(0);

  useInput((input, key) => {
    // If busy or in overlay mode, don't handle dashboard keys
    if (actions.busy) return;
    if (actions.mode !== "dashboard") return;

    // Double-escape to quit
    if (key.escape) {
      const now = Date.now();
      if (now - lastEscRef.current < ESC_DOUBLE_TAP_MS) {
        actions.onEscHint(false);
        actions.onQuit();
        return;
      }
      lastEscRef.current = now;
      actions.onEscHint(true);
      setTimeout(() => actions.onEscHint(false), ESC_DOUBLE_TAP_MS);
      return;
    }

    // Navigation
    if (input === "j" || key.downArrow) {
      const next = Math.min(actions.selectedIndex + 1, actions.worktreeCount - 1);
      actions.onSelect(next);
      return;
    }
    if (input === "k" || key.upArrow) {
      const prev = Math.max(actions.selectedIndex - 1, 0);
      actions.onSelect(prev);
      return;
    }

    // Actions
    if (key.return) {
      actions.onEnter();
      return;
    }
    if (input === "n") {
      actions.onNew();
      return;
    }
    if (input === "d") {
      actions.onDelete();
      return;
    }
    if (input === "s") {
      actions.onSettings();
      return;
    }
    if (input === "r") {
      actions.onRefresh();
      return;
    }
    if (input === "g") {
      actions.onOpenPr();
      return;
    }
    if (input === "l") {
      actions.onOpenLinear();
      return;
    }
    if (input === "t") {
      actions.onOpenTerminal();
      return;
    }
    if (input === "w") {
      actions.onToggleLogs();
      return;
    }
    if (input === "u") {
      actions.onUpdate?.();
      return;
    }
    if (input === "t") {
      actions.onClaude();
      return;
    }
    if (input === "q") {
      actions.onQuit();
      return;
    }
  });
}
