/**
 * Test helper: create temporary git repositories for integration tests.
 */
import { mkdtempSync, mkdirSync } from "fs";
import { tmpdir } from "os";
import { join } from "path";
import { execSync } from "child_process";

export function createTempGitRepo(name = "test-repo"): string {
  const dir = mkdtempSync(join(tmpdir(), `am-git-${name}-`));
  execSync("git init", { cwd: dir, stdio: "ignore" });
  execSync("git config user.email test@test.com", { cwd: dir, stdio: "ignore" });
  execSync("git config user.name Test", { cwd: dir, stdio: "ignore" });
  execSync("git commit --allow-empty -m 'initial commit'", { cwd: dir, stdio: "ignore" });
  return dir;
}

export function createTempGitRepoWithWorktrees(
  name = "test-repo",
  branches: string[] = []
): { repoPath: string; worktreePaths: Map<string, string> } {
  const repoPath = createTempGitRepo(name);
  const worktreePaths = new Map<string, string>();

  const worktreeDir = join(repoPath, ".worktrees");
  mkdirSync(worktreeDir, { recursive: true });

  for (const branch of branches) {
    const wtPath = join(worktreeDir, branch.replace(/\//g, "-"));
    execSync(`git worktree add -b ${branch} "${wtPath}"`, { cwd: repoPath, stdio: "ignore" });
    worktreePaths.set(branch, wtPath);
  }

  return { repoPath, worktreePaths };
}
