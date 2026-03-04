import { installGlobalHooks, uninstallGlobalHooks, isGlobalHooksInstalled } from "../lib/hooks-installer.js";
import { outputJson } from "../lib/output.js";

export function hooksInstall(): void {
  installGlobalHooks();
  console.log("Hooks installed into ~/.claude/settings.json");
}

export function hooksUninstall(): void {
  uninstallGlobalHooks();
  console.log("Hooks removed from ~/.claude/settings.json");
}

export function hooksStatus(opts: { json?: boolean }): void {
  const installed = isGlobalHooksInstalled();

  if (opts.json) {
    outputJson({ installed });
    return;
  }

  console.log(`Claude hooks: ${installed ? "installed" : "not installed"}`);
}
