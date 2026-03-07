import { homedir } from "os";
import { join } from "path";

export const APP_DIR = join(homedir(), ".agent-monitor");
export const DB_PATH = join(APP_DIR, "agent-monitor.db");
export const SETTINGS_PATH = join(APP_DIR, "settings.json");
export const LOG_PATH = join(APP_DIR, "debug.log");
export const SOCKET_PATH = join(APP_DIR, "am.sock");
export const RULES_PATH = join(APP_DIR, "rules.json");
export const AM_MANAGED_PERMISSIONS_PATH = join(APP_DIR, "am-managed-permissions.json");
export const ROLES_DIR = join(APP_DIR, "roles");
