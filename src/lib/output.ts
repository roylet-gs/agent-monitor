/**
 * CLI output formatting utilities.
 * Provides table, key-value, and JSON output for CLI commands.
 */

export interface TableColumn {
  key: string;
  header: string;
  width?: number;
  align?: "left" | "right";
}

export function outputJson(data: unknown): void {
  console.log(JSON.stringify(data, null, 2));
}

export function outputTable(rows: Record<string, string>[], columns: TableColumn[]): void {
  if (rows.length === 0) {
    console.log("No results.");
    return;
  }

  // Calculate column widths
  const widths = columns.map((col) => {
    const maxData = rows.reduce((max, row) => Math.max(max, (row[col.key] ?? "").length), 0);
    return col.width ?? Math.max(col.header.length, maxData);
  });

  // Header
  const header = columns.map((col, i) => padCell(col.header, widths[i]!, col.align)).join("  ");
  console.log(header);
  console.log(columns.map((_, i) => "─".repeat(widths[i]!)).join("  "));

  // Rows
  for (const row of rows) {
    const line = columns.map((col, i) => padCell(row[col.key] ?? "", widths[i]!, col.align)).join("  ");
    console.log(line);
  }
}

export function outputKeyValue(pairs: [string, string][]): void {
  const maxKey = pairs.reduce((max, [k]) => Math.max(max, k.length), 0);
  for (const [key, value] of pairs) {
    console.log(`${key.padEnd(maxKey)}  ${value}`);
  }
}

function padCell(text: string, width: number, align?: "left" | "right"): string {
  if (text.length > width) {
    return text.slice(0, width - 1) + "…";
  }
  return align === "right" ? text.padStart(width) : text.padEnd(width);
}
