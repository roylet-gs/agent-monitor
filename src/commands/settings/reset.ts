import { saveSettings, DEFAULT_SETTINGS } from "../../lib/settings.js";

export function settingsReset(): void {
  saveSettings({ ...DEFAULT_SETTINGS });
  console.log("Settings reset to defaults.");
}
