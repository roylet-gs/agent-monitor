import { execSync } from "child_process";
import { existsSync, writeFileSync } from "fs";
import { listRoles, getRoleContent, deleteRole, getRolePath, ensureRolesDir } from "../lib/roles.js";
import { outputTable, outputJson } from "../lib/output.js";

export function roleList(opts: { json?: boolean }): void {
  const roles = listRoles();

  if (opts.json) {
    outputJson(roles);
    return;
  }

  if (roles.length === 0) {
    console.log("No roles found. Create one with: am role edit <name>");
    return;
  }

  outputTable(
    roles.map((r) => ({
      name: r.name,
      path: r.path,
    })),
    [
      { key: "name", header: "Name" },
      { key: "path", header: "Path" },
    ],
  );
}

export function roleEdit(name: string): void {
  ensureRolesDir();
  const path = getRolePath(name);
  const editor = process.env.EDITOR || "vim";

  // Create file if it doesn't exist
  if (!existsSync(path)) {
    writeFileSync(path, `# ${name}\n\nDescribe the role for this Claude agent session.\n`, "utf-8");
  }

  try {
    execSync(`${editor} "${path}"`, { stdio: "inherit" });
    console.log(`Role "${name}" saved at ${path}`);
  } catch (err) {
    console.error(`Failed to open editor: ${err}`);
    process.exit(1);
  }
}

export function roleRemove(name: string): void {
  if (deleteRole(name)) {
    console.log(`Role "${name}" removed.`);
  } else {
    console.error(`Role "${name}" not found.`);
    process.exit(1);
  }
}

export function roleShow(name: string, opts: { json?: boolean }): void {
  const content = getRoleContent(name);
  if (content === null) {
    console.error(`Role "${name}" not found.`);
    process.exit(1);
  }

  if (opts.json) {
    outputJson({ name, content });
    return;
  }

  console.log(content);
}
