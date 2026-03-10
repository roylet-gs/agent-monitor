import React, { useState, useEffect } from "react";
import { Box, Text, useInput } from "ink";
import { readdirSync, statSync, existsSync } from "fs";
import { join, resolve, basename, dirname } from "path";
import { homedir } from "os";
import { isGitRepo } from "../lib/git.js";
import { log } from "../lib/logger.js";

interface FolderBrowserProps {
  onSelect: (path: string) => void;
  onCancel: () => void;
}

export function FolderBrowser({ onSelect, onCancel }: FolderBrowserProps) {
  const [currentPath, setCurrentPath] = useState(
    resolve(process.cwd() === "/" ? homedir() : process.cwd(), "..")
  );
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [filter, setFilter] = useState("");
  const [scrollOffset, setScrollOffset] = useState(0);
  const [entries, setEntries] = useState<
    { name: string; isDir: boolean; isRepo: boolean; fullPath: string }[]
  >([]);

  useEffect(() => {
    try {
      const items = readdirSync(currentPath)
        .filter((name) => !name.startsWith("."))
        .filter((name) => {
          if (!filter) return true;
          return name.toLowerCase().includes(filter.toLowerCase());
        })
        .map((name) => {
          const fullPath = join(currentPath, name);
          try {
            const stat = statSync(fullPath);
            if (!stat.isDirectory()) return null;
            return {
              name,
              isDir: true,
              isRepo: isGitRepo(fullPath),
              fullPath,
            };
          } catch {
            return null;
          }
        })
        .filter((e): e is NonNullable<typeof e> => e !== null)
        .sort((a, b) => {
          // Repos first, then alphabetical
          if (a.isRepo && !b.isRepo) return -1;
          if (!a.isRepo && b.isRepo) return 1;
          return a.name.localeCompare(b.name);
        });

      // Add parent directory entry
      const parent = dirname(currentPath);
      if (parent !== currentPath) {
        items.unshift({
          name: "..",
          isDir: true,
          isRepo: false,
          fullPath: parent,
        });
      }

      setEntries(items);
      setSelectedIndex(0);
      setScrollOffset(0);
    } catch (err) {
      log("warn", "FolderBrowser", `Failed to read directory ${currentPath}: ${err}`);
      setEntries([]);
    }
  }, [currentPath, filter]);

  useInput((input, key) => {
    if (key.escape) {
      onCancel();
      return;
    }

    if (key.upArrow) {
      setSelectedIndex((prev) => {
        const next = Math.max(0, prev - 1);
        setScrollOffset((offset) => (next < offset ? next : offset));
        return next;
      });
      return;
    }

    if (key.downArrow) {
      setSelectedIndex((prev) => {
        const next = Math.min(entries.length - 1, prev + 1);
        setScrollOffset((offset) =>
          next >= offset + 15 ? next - 14 : offset
        );
        return next;
      });
      return;
    }

    if (key.return) {
      const entry = entries[selectedIndex];
      if (!entry) return;

      if (entry.isRepo) {
        onSelect(entry.fullPath);
        return;
      }

      // Navigate into directory
      setCurrentPath(entry.fullPath);
      setFilter("");
      return;
    }

    if (key.backspace || key.delete) {
      if (filter.length > 0) {
        setFilter((f) => f.slice(0, -1));
      } else {
        // Go up
        const parent = dirname(currentPath);
        if (parent !== currentPath) {
          setCurrentPath(parent);
        }
      }
      return;
    }

    // Type to filter
    if (input && !key.ctrl && !key.meta) {
      setFilter((f) => f + input);
    }
  });

  const displayPath = currentPath.replace(homedir(), "~");
  const maxVisible = 15;
  const visibleEntries = entries.slice(scrollOffset, scrollOffset + maxVisible);

  return (
    <Box flexDirection="column" borderStyle="single" paddingX={1}>
      <Box justifyContent="space-between">
        <Text bold color="cyan">
          Add Repository
        </Text>
      </Box>

      <Box marginTop={1} flexDirection="column">
        <Text>Navigate to a git repository:</Text>
        <Text bold>{displayPath}/</Text>
        <Text dimColor>──────────────────────</Text>
      </Box>

      <Box flexDirection="column" marginTop={1}>
        {scrollOffset > 0 && (
          <Text dimColor>{scrollOffset} above ...</Text>
        )}
        {visibleEntries.map((entry, i) => {
          const absoluteIndex = i + scrollOffset;
          return (
            <Box key={entry.fullPath} gap={1}>
              <Text>{absoluteIndex === selectedIndex ? "▸" : " "}</Text>
              <Text>{entry.name === ".." ? "📁" : "📁"}</Text>
              <Text
                bold={absoluteIndex === selectedIndex}
                color={absoluteIndex === selectedIndex ? "cyan" : undefined}
              >
                {entry.name}
              </Text>
              {entry.isRepo && (
                <Text color="green">(git repo ✓)</Text>
              )}
            </Box>
          );
        })}
        {entries.length > scrollOffset + maxVisible && (
          <Text dimColor>... {entries.length - scrollOffset - maxVisible} more</Text>
        )}
        {entries.length === 0 && (
          <Text dimColor>No directories found</Text>
        )}
      </Box>

      {filter && (
        <Box marginTop={1}>
          <Text dimColor>Filter: </Text>
          <Text>{filter}</Text>
        </Box>
      )}

      <Box
        marginTop={1}
        borderStyle="single"
        borderTop
        borderBottom={false}
        borderLeft={false}
        borderRight={false}
        paddingX={0}
      >
        <Text>
          <Text color="yellow">[Enter]</Text> Select{" "}
          <Text color="yellow">[↑↓]</Text> Navigate{" "}
          <Text color="yellow">[Backspace]</Text> Go up{" "}
          <Text color="yellow">[Esc]</Text> Cancel
        </Text>
      </Box>
    </Box>
  );
}
