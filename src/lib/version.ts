import { createRequire } from "module";
import { readFileSync } from "fs";
import { dirname, join } from "path";
import { fileURLToPath } from "url";
import { execFile } from "child_process";
import type { Settings } from "./types.js";

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

/** Compare two semver strings. Returns 1 if a > b, -1 if a < b, 0 if equal. */
function compareSemver(a: string, b: string): number {
  const pa = a.split(".").map(Number);
  const pb = b.split(".").map(Number);
  for (let i = 0; i < 3; i++) {
    const na = pa[i] ?? 0;
    const nb = pb[i] ?? 0;
    if (na > nb) return 1;
    if (na < nb) return -1;
  }
  return 0;
}

export interface UpdateCheckResult {
  current: string;
  latest: string;
  updateAvailable: boolean;
  settings: Pick<Settings, "lastUpdateCheck" | "latestKnownVersion">;
}

const CHECK_INTERVAL_MS = 5 * 60 * 1000; // 5 minutes

export async function checkForUpdate(
  settings: Pick<Settings, "lastUpdateCheck" | "latestKnownVersion">,
  options?: { forceCheck?: boolean }
): Promise<UpdateCheckResult | null> {
  const current = getVersion();

  // Return cached result if checked recently (unless forced)
  if (
    !options?.forceCheck &&
    settings.lastUpdateCheck &&
    settings.latestKnownVersion &&
    Date.now() - settings.lastUpdateCheck < CHECK_INTERVAL_MS
  ) {
    return {
      current,
      latest: settings.latestKnownVersion,
      updateAvailable: compareSemver(settings.latestKnownVersion, current) > 0,
      settings: {
        lastUpdateCheck: settings.lastUpdateCheck,
        latestKnownVersion: settings.latestKnownVersion,
      },
    };
  }

  // Fetch latest version from registry
  try {
    const latest = await new Promise<string>((resolve, reject) => {
      execFile(
        "npm",
        [
          "view",
          "@roylet-gs/agent-monitor",
          "version",
          "--registry=https://npm.pkg.github.com",
        ],
        { timeout: 5000 },
        (err, stdout) => {
          if (err) return reject(err);
          const version = stdout.trim();
          if (!version) return reject(new Error("Empty version response"));
          resolve(version);
        }
      );
    });

    return {
      current,
      latest,
      updateAvailable: compareSemver(latest, current) > 0,
      settings: {
        lastUpdateCheck: Date.now(),
        latestKnownVersion: latest,
      },
    };
  } catch (err) {
    console.error("Update check failed:", err);
    return null;
  }
}
