import { execFile } from "child_process";
import { log } from "./logger.js";
import type { SystemSound } from "./types.js";

export const SYSTEM_SOUNDS: SystemSound[] = [
  "Basso", "Blow", "Bottle", "Frog", "Funk", "Glass", "Hero",
  "Morse", "Ping", "Pop", "Purr", "Sosumi", "Submarine", "Tink",
];

/**
 * Play a macOS system sound by name.
 * Non-blocking: spawns afplay and does not wait for it to finish.
 * Silently ignores errors (e.g., if afplay is unavailable).
 */
export function playSound(sound: SystemSound): void {
  const soundPath = `/System/Library/Sounds/${sound}.aiff`;

  log("debug", "audio", `Playing sound "${sound}": ${soundPath}`);
  execFile("afplay", [soundPath], (err) => {
    if (err) {
      log("debug", "audio", `Failed to play sound "${sound}": ${err.message}`);
    }
  });
}
