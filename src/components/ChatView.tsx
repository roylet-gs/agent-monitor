import React, { useMemo, useState } from "react";
import { Box, Text, useInput, useStdout } from "ink";
import TextInput from "ink-text-input";
import { Spinner } from "./Spinner.js";
import { useChatTranscript } from "../hooks/useChatTranscript.js";
import { startTurn } from "../lib/claude-session.js";
import { openClaudeInTerminal } from "../lib/ide-launcher.js";
import { log } from "../lib/logger.js";
import type { ChatMessage, Settings, WorktreeWithStatus } from "../lib/types.js";

interface ChatViewProps {
  worktree: WorktreeWithStatus;
  settings: Settings;
  onBack: () => void;
}

interface DisplayLine {
  text: string;
  color?: string;
  dim?: boolean;
  bold?: boolean;
}

function wrapText(text: string, width: number): string[] {
  const lines: string[] = [];
  for (const raw of text.split("\n")) {
    if (raw.length <= width) {
      lines.push(raw);
      continue;
    }
    let remaining = raw;
    while (remaining.length > width) {
      let cut = remaining.lastIndexOf(" ", width);
      if (cut <= 0) cut = width;
      lines.push(remaining.slice(0, cut));
      remaining = remaining.slice(cut).trimStart();
    }
    if (remaining) lines.push(remaining);
  }
  return lines;
}

function toDisplayLines(transcript: ChatMessage[], width: number): DisplayLine[] {
  const lines: DisplayLine[] = [];
  for (const msg of transcript) {
    const style: Omit<DisplayLine, "text"> =
      msg.role === "user"
        ? { color: "cyan", bold: true }
        : msg.role === "tool" || msg.role === "system"
          ? { dim: true }
          : msg.role === "error"
            ? { color: "red" }
            : {};
    const prefix =
      msg.role === "user" ? "❯ " : msg.role === "tool" ? "⚒ " : msg.role === "system" ? "· " : msg.role === "error" ? "✗ " : "";

    // Blank line before user prompts and assistant messages to break up turns
    if (lines.length > 0 && (msg.role === "user" || msg.role === "assistant")) {
      lines.push({ text: "" });
    }
    const wrapped = wrapText(msg.text, Math.max(20, width - prefix.length));
    wrapped.forEach((text, i) => {
      lines.push({ ...style, text: i === 0 ? prefix + text : " ".repeat(prefix.length) + text });
    });
  }
  return lines;
}

export function ChatView({ worktree, settings, onBack }: ChatViewProps) {
  const { stdout } = useStdout();
  const { session, transcript, turnRunning } = useChatTranscript(worktree.id);
  const [draft, setDraft] = useState("");
  const [scrollFromBottom, setScrollFromBottom] = useState(0);
  const [error, setError] = useState<string | null>(null);

  const columns = stdout?.columns ?? 80;
  const rows = stdout?.rows ?? 24;
  // header (2) + borders/hints/input (5) leave the rest for the transcript
  const visibleLines = Math.max(4, rows - 7);
  const contentWidth = Math.max(20, columns - 4);

  const lines = useMemo(() => toDisplayLines(transcript, contentWidth), [transcript, contentWidth]);
  const maxScroll = Math.max(0, lines.length - visibleLines);
  const offset = Math.min(scrollFromBottom, maxScroll);
  const start = Math.max(0, lines.length - visibleLines - offset);
  const visible = lines.slice(start, start + visibleLines);

  const displayName = worktree.custom_name ?? worktree.branch;

  const handleSubmit = () => {
    const prompt = draft.trim();
    if (!prompt || turnRunning) return;
    try {
      startTurn(worktree, prompt, settings);
      setDraft("");
      setError(null);
      setScrollFromBottom(0);
    } catch (err) {
      const message = err instanceof Error ? err.message : `${err}`;
      log("warn", "chat", `Failed to start turn for ${worktree.branch}: ${message}`);
      setError(message);
    }
  };

  const handleAttach = () => {
    try {
      openClaudeInTerminal(worktree.path, !!worktree.agent_status?.session_id, displayName, session?.id);
    } catch (err) {
      setError(err instanceof Error ? err.message : `${err}`);
    }
  };

  useInput((input, key) => {
    if (key.escape) {
      onBack();
      return;
    }
    if (key.ctrl && input === "t") {
      handleAttach();
      return;
    }
    if (key.upArrow) {
      setScrollFromBottom((v) => Math.min(v + 1, maxScroll));
      return;
    }
    if (key.downArrow) {
      setScrollFromBottom((v) => Math.max(v - 1, 0));
      return;
    }
    if (key.pageUp) {
      setScrollFromBottom((v) => Math.min(v + visibleLines, maxScroll));
      return;
    }
    if (key.pageDown) {
      setScrollFromBottom((v) => Math.max(v - visibleLines, 0));
      return;
    }
  });

  return (
    <Box flexDirection="column" height={rows} paddingX={1}>
      <Box justifyContent="space-between">
        <Text bold color="cyan">
          Chat — {displayName}
        </Text>
        <Text dimColor>{session ? `session ${session.id.slice(0, 8)} · ${session.turn_count} turn${session.turn_count === 1 ? "" : "s"}` : "no session yet"}</Text>
      </Box>

      <Box flexDirection="column" flexGrow={1} marginTop={1}>
        {start > 0 && <Text dimColor>… {start} more above …</Text>}
        {lines.length === 0 ? (
          <Text dimColor>
            {session
              ? "No messages yet."
              : "No session yet — type a prompt below to start a Claude session in this worktree."}
          </Text>
        ) : (
          visible.map((line, i) => (
            <Text key={start + i} color={line.color} dimColor={line.dim} bold={line.bold} wrap="truncate-end">
              {line.text || " "}
            </Text>
          ))
        )}
        {offset > 0 && <Text dimColor>… {offset} more below …</Text>}
      </Box>

      {error && (
        <Box>
          <Text color="red">Error: {error}</Text>
        </Box>
      )}

      <Box borderStyle="single" borderBottom={false} borderLeft={false} borderRight={false} flexDirection="column">
        {turnRunning ? (
          <Spinner label="Claude is working… (Ctrl+T to open in terminal)" />
        ) : (
          <Box>
            <Text color="cyan">❯ </Text>
            <TextInput value={draft} onChange={setDraft} onSubmit={handleSubmit} placeholder="Send a prompt…" />
          </Box>
        )}
        <Text dimColor>
          [Enter] Send  [↑↓] Scroll  [Ctrl+T] Terminal  [Esc] Back
        </Text>
      </Box>
    </Box>
  );
}
