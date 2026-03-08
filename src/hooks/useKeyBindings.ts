import { useInput } from "ink";
import { useCallback, useRef, useState } from "react";
import type { AppMode } from "../lib/types.js";

interface KeyBindingActions {
  selectedIndex: number;
  worktreeCount: number;
  mode: AppMode;
  busy: string | null;
  onSelect: (index: number) => void;
  onEnter: () => void;
  onEnterTerminal: () => void;
  onNew: () => void;
  onDelete: () => void;
  onSettings: () => void;
  onRefresh: () => void;
  onOpenPr: () => void;
  onOpenLinear: () => void;
  onToggleLogs: () => void;
  onUpdate?: () => void;
  onClaude: () => void;
  onQuit: () => void;
  onEscHint: (show: boolean) => void;
}

const ESC_DOUBLE_TAP_MS = 500;
const MODIFIER_HELD_MS = 1500;

export function useKeyBindings(actions: KeyBindingActions): { modifierHeld: boolean } {
  const lastEscRef = useRef(0);
  const modTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [modifierHeld, setModifierHeld] = useState(false);

  const flashModifier = useCallback(() => {
    setModifierHeld(true);
    if (modTimerRef.current) clearTimeout(modTimerRef.current);
    modTimerRef.current = setTimeout(() => setModifierHeld(false), MODIFIER_HELD_MS);
  }, []);

  const clearModifier = useCallback(() => {
    setModifierHeld(false);
    if (modTimerRef.current) {
      clearTimeout(modTimerRef.current);
      modTimerRef.current = null;
    }
  }, []);

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

    // Ctrl/Meta+Enter: open in terminal (override)
    if (key.return && (key.meta || key.ctrl)) {
      clearModifier();
      actions.onEnterTerminal();
      return;
    }

    // Flash alternate action bar on any Ctrl keypress
    if (key.ctrl && !key.return) {
      flashModifier();
      // Don't return — fall through so Ctrl+j/k still navigate
    }

    // "z" as modifier key: press z then Enter to open in terminal
    if (input === "z") {
      flashModifier();
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

    // Enter: check if modifier is active (z was pressed) → terminal override
    if (key.return) {
      if (modifierHeld) {
        clearModifier();
        actions.onEnterTerminal();
      } else {
        actions.onEnter();
      }
      return;
    }

    // Any other key clears the modifier state
    if (modifierHeld) {
      clearModifier();
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
    if (input === "w") {
      actions.onToggleLogs();
      return;
    }
    if (input === "u") {
      actions.onUpdate?.();
      return;
    }
    if (input === "q") {
      actions.onQuit();
      return;
    }
  });

  return { modifierHeld };
}
