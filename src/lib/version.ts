import { createRequire } from "module";
import { readFileSync } from "fs";
import { dirname, join } from "path";
import { fileURLToPath } from "url";

const require = createRequire(import.meta.url);

export interface ReleaseNote {
  hash: string;
  message: string;
}

export function getVersion(): string {
  const pkg = require("../../package.json");
  return pkg.version as string;
}

export function getReleaseNotes(): ReleaseNote[] {
  try {
    const __dirname = dirname(fileURLToPath(import.meta.url));
    const notesPath = join(__dirname, "..", "..", "release-notes.json");
    const raw = readFileSync(notesPath, "utf-8");
    return JSON.parse(raw) as ReleaseNote[];
  } catch {
    return [];
  }
}

export function isNewVersion(lastSeen: string | undefined, current: string): boolean {
  if (lastSeen === undefined) return false;
  return lastSeen !== current;
}
