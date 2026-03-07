import React, { useState } from "react";
import { Box, Text, useInput } from "ink";
import { listRoles, getRoleContent } from "../lib/roles.js";

interface RoleSelectorProps {
  onSelect: (roleName: string | null, roleContent: string | null) => void;
  onCancel: () => void;
}

export function RoleSelector({ onSelect, onCancel }: RoleSelectorProps) {
  const roles = listRoles();
  const [selectedIndex, setSelectedIndex] = useState(0);

  // Include a "No role" option at the top
  const options = [{ name: "(no role)", path: "" }, ...roles];

  useInput((input, key) => {
    if (key.escape) {
      onCancel();
      return;
    }

    if (key.upArrow) {
      setSelectedIndex((i) => Math.max(0, i - 1));
      return;
    }

    if (key.downArrow) {
      setSelectedIndex((i) => Math.min(options.length - 1, i + 1));
      return;
    }

    if (key.return) {
      if (selectedIndex === 0) {
        onSelect(null, null);
      } else {
        const role = roles[selectedIndex - 1];
        const content = getRoleContent(role.name);
        onSelect(role.name, content);
      }
      return;
    }
  });

  return (
    <Box flexDirection="column" borderStyle="single" borderColor="cyan" paddingX={1}>
      <Text bold color="cyan">Select a Role</Text>
      <Text dimColor>Roles define the initial prompt for the Claude session</Text>
      <Box flexDirection="column" marginTop={1}>
        {options.map((opt, i) => (
          <Text key={opt.name}>
            {i === selectedIndex ? (
              <Text color="cyan">{"\u25B8"} </Text>
            ) : (
              "  "
            )}
            <Text color={i === selectedIndex ? "cyan" : undefined}>
              {opt.name}
            </Text>
          </Text>
        ))}
      </Box>
      {roles.length === 0 && (
        <Box marginTop={1}>
          <Text dimColor>
            No roles found. Create one with: am role edit {"<name>"}
          </Text>
        </Box>
      )}
      <Box marginTop={1}>
        <Text dimColor>
          <Text color="yellow">[Up/Down]</Text> Navigate{" "}
          <Text color="yellow">[Enter]</Text> Select{" "}
          <Text color="yellow">[Esc]</Text> Cancel
        </Text>
      </Box>
    </Box>
  );
}
