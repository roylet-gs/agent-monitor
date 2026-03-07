export interface MouseEvent {
  x: number; // 0-based column
  y: number; // 0-based row
  button: number; // 0=left, 1=middle, 2=right
  type: "press" | "release" | "move";
}

// Enable SGR extended mouse mode
export function enableMouse(): void {
  process.stdout.write("\x1b[?1000h"); // Basic mouse tracking
  process.stdout.write("\x1b[?1006h"); // SGR extended mode
}

export function disableMouse(): void {
  process.stdout.write("\x1b[?1006l");
  process.stdout.write("\x1b[?1000l");
}

// Parse SGR mouse sequence: \x1b[<button;col;rowM or \x1b[<button;col;rowm
// Returns the parsed event and the remaining unprocessed data.
export function parseMouseEvent(data: string): { event: MouseEvent; rest: string } | null {
  const match = data.match(/\x1b\[<(\d+);(\d+);(\d+)([Mm])/);
  if (!match) return null;

  const buttonCode = parseInt(match[1], 10);
  const x = parseInt(match[2], 10) - 1; // 1-based → 0-based
  const y = parseInt(match[3], 10) - 1;
  const isRelease = match[4] === "m";

  const button = buttonCode & 3;
  const type: MouseEvent["type"] = isRelease
    ? "release"
    : buttonCode & 32
      ? "move"
      : "press";

  const rest = data.slice(match.index! + match[0].length);
  return { event: { x, y, button, type }, rest };
}
