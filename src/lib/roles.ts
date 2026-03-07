import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync, unlinkSync } from "fs";
import { join, basename } from "path";
import { ROLES_DIR } from "./paths.js";

export function ensureRolesDir(): void {
  if (!existsSync(ROLES_DIR)) {
    mkdirSync(ROLES_DIR, { recursive: true });
  }
}

export function listRoles(): { name: string; path: string }[] {
  ensureRolesDir();
  try {
    return readdirSync(ROLES_DIR)
      .filter((f) => f.endsWith(".md"))
      .map((f) => ({
        name: basename(f, ".md"),
        path: join(ROLES_DIR, f),
      }))
      .sort((a, b) => a.name.localeCompare(b.name));
  } catch {
    return [];
  }
}

export function getRoleContent(name: string): string | null {
  const filePath = join(ROLES_DIR, `${name}.md`);
  try {
    return readFileSync(filePath, "utf-8");
  } catch {
    return null;
  }
}

export function createRole(name: string, content: string): void {
  ensureRolesDir();
  const filePath = join(ROLES_DIR, `${name}.md`);
  writeFileSync(filePath, content, "utf-8");
}

export function deleteRole(name: string): boolean {
  const filePath = join(ROLES_DIR, `${name}.md`);
  try {
    unlinkSync(filePath);
    return true;
  } catch {
    return false;
  }
}

export function getRolePath(name: string): string {
  return join(ROLES_DIR, `${name}.md`);
}
