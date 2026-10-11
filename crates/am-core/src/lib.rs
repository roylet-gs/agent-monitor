//! Shared core for the native `am` binaries: data paths, logging, the SQLite
//! schema, and the agent-status rules. The TS app shares the same DB in place
//! during the migration, so every schema change here must stay readable by it.

pub mod db;
pub mod gitinfo;
pub mod grouping;
pub mod hooks_installer;
pub mod integrations;
pub mod launch;
pub mod log;
pub mod model;
pub mod paths;
pub mod procscan;
pub mod settings;
pub mod status;
pub mod timefmt;
pub mod worktrees;
