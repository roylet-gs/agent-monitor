//! Shared core for the native `am` binaries: data paths, logging, the SQLite
//! schema, and the agent-status rules. The TS app shares the same DB in place
//! during the migration, so every schema change here must stay readable by it.

pub mod harness;
pub mod context;
pub mod db;
pub mod gitinfo;
pub mod grouping;
pub mod hooks_installer;
pub mod integrations;
pub mod launch;
pub mod log;
pub mod mcp_installer;
pub mod model;
pub mod paths;
pub mod perf;
pub mod procscan;
pub mod program_status;
pub mod review_groups;
pub mod settings;
pub mod stacks;
pub mod status;
pub mod themes;
pub mod timefmt;
pub mod update_check;
pub mod usage;
pub mod worktrees;
