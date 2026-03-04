import { readFileSync } from "fs";
import { createStartupScript, openScriptInEditor, removeStartupScript, hasStartupScript, getScriptPath } from "../lib/scripts.js";
import { loadSettings } from "../lib/settings.js";
import { resolveRepo } from "../lib/resolve.js";
import { outputJson } from "../lib/output.js";

export function scriptEdit(opts: { repo?: string }): void {
  const repo = resolveRepo(opts.repo);
  const settings = loadSettings();
  openScriptInEditor(repo.id, settings.ide);
  console.log(`Opened startup script for ${repo.name} in ${settings.ide}`);
}

export function scriptRemove(opts: { repo?: string }): void {
  const repo = resolveRepo(opts.repo);

  if (!hasStartupScript(repo.id)) {
    console.log(`No startup script for ${repo.name}`);
    return;
  }

  removeStartupScript(repo.id);
  console.log(`Removed startup script for ${repo.name}`);
}

export function scriptShow(opts: { repo?: string; json?: boolean }): void {
  const repo = resolveRepo(opts.repo);

  if (!hasStartupScript(repo.id)) {
    console.log(`No startup script for ${repo.name}`);
    return;
  }

  const scriptPath = getScriptPath(repo.id);
  const content = readFileSync(scriptPath, "utf-8");

  if (opts.json) {
    outputJson({ repo: repo.name, path: scriptPath, content });
    return;
  }

  console.log(`# Startup script for ${repo.name}`);
  console.log(`# Path: ${scriptPath}`);
  console.log();
  console.log(content);
}
