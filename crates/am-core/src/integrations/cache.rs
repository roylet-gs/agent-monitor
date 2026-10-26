//! GitHub PR / Linear ticket cache (port of `src/lib/integration-cache.ts`).
//!
//! Shares the `integration_cache(kind, cache_key, payload, fetched_at)` table
//! with the TS app in place: `kind` is `"pr"` / `"linear"`, `cache_key` is
//! `"<repo_id>:<branch>"`, and `payload` is the camelCase `PrInfo` /
//! `LinearInfo` JSON, or NULL for "fetched successfully, there is none".
//!
//! Rules carried over from the TS app:
//! 1. Only a successful fetch writes. A failure leaves the old value alone, so
//!    PR badges and Linear ticket/project grouping survive going offline.
//! 2. Entries never expire; they are replaced by a later success or pruned
//!    when their worktree disappears. An empty worktree list never prunes.

use std::collections::HashMap;

use rusqlite::{Connection, params, params_from_iter};
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::FetchResult;
use super::linear::{attachment_matches_branch, attachment_to_pr_info};
use crate::model::{LinearInfo, PrInfo};
use crate::{debug, info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheKind {
    Pr,
    Linear,
    /// The review-request list (`integrations::reviews`), one row.
    Review,
    /// Who can author PRs in the tracked repos (review autocomplete), one row.
    People,
}

impl CacheKind {
    pub fn as_str(self) -> &'static str {
        match self {
            CacheKind::Pr => "pr",
            CacheKind::Linear => "linear",
            CacheKind::Review => "review",
            CacheKind::People => "people",
        }
    }
}

/// Repo-scoped key, so a branch name common to two repos can't collide.
pub fn cache_key(repo_id: &str, branch: &str) -> String {
    format!("{repo_id}:{branch}")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheRow {
    pub cache_key: String,
    /// JSON payload, or None for a successful fetch that found nothing.
    pub payload: Option<String>,
    pub fetched_at: String,
}

// --- raw table access (same SQL as src/lib/db.ts) ---

pub fn load_rows(conn: &Connection, kind: CacheKind) -> rusqlite::Result<Vec<CacheRow>> {
    let mut stmt = conn.prepare("SELECT cache_key, payload, fetched_at FROM integration_cache WHERE kind = ?1")?;
    let rows = stmt.query_map([kind.as_str()], |r| {
        Ok(CacheRow { cache_key: r.get(0)?, payload: r.get(1)?, fetched_at: r.get(2)? })
    })?;
    rows.collect()
}

/// Record a *successful* fetch. A failed fetch must not call this.
pub fn set_entry(conn: &Connection, kind: CacheKind, key: &str, payload: Option<&str>) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO integration_cache (kind, cache_key, payload) VALUES (?1, ?2, ?3)
         ON CONFLICT(kind, cache_key) DO UPDATE SET payload = excluded.payload, fetched_at = datetime('now')",
        params![kind.as_str(), key, payload],
    )?;
    Ok(())
}

/// Delete rows of `kind` whose key isn't in `valid_keys`; an empty list
/// deletes all of that kind (used by [`IntegrationCache::clear_linear`]).
pub fn prune_rows(conn: &Connection, kind: CacheKind, valid_keys: &[String]) -> rusqlite::Result<usize> {
    if valid_keys.is_empty() {
        return conn.execute("DELETE FROM integration_cache WHERE kind = ?1", [kind.as_str()]);
    }
    let placeholders = vec!["?"; valid_keys.len()].join(",");
    let sql = format!("DELETE FROM integration_cache WHERE kind = ? AND cache_key NOT IN ({placeholders})");
    let params = std::iter::once(kind.as_str()).chain(valid_keys.iter().map(String::as_str));
    conn.execute(&sql, params_from_iter(params))
}

// --- in-memory mirror ---

/// In-memory mirror of `integration_cache`, written through to SQLite.
#[derive(Debug, Default)]
pub struct IntegrationCache {
    prs: HashMap<String, Option<PrInfo>>,
    linear: HashMap<String, Option<LinearInfo>>,
    hydrated: bool,
}

fn hydrate_kind<T: DeserializeOwned>(
    conn: &Connection,
    kind: CacheKind,
    target: &mut HashMap<String, Option<T>>,
) -> rusqlite::Result<usize> {
    let mut restored = 0;
    for row in load_rows(conn, kind)? {
        match row.payload {
            None => {
                target.insert(row.cache_key, None);
                restored += 1;
            }
            Some(p) => match serde_json::from_str::<T>(&p) {
                Ok(v) => {
                    target.insert(row.cache_key, Some(v));
                    restored += 1;
                }
                Err(e) => warn!(
                    "integration-cache",
                    "Dropping unparseable {} cache row {}: {e}",
                    kind.as_str(),
                    row.cache_key
                ),
            },
        }
    }
    Ok(restored)
}

fn commit<T: Serialize + Clone>(
    conn: &Connection,
    kind: CacheKind,
    key: &str,
    mem: &mut HashMap<String, Option<T>>,
    result: &FetchResult<T>,
) -> bool {
    let FetchResult::Ok(value) = result else { return false };
    if value.is_none() && mem.get(key).is_some_and(Option::is_some) {
        info!("integration-cache", "Clearing cached {} for {key}: upstream reports none", kind.as_str());
    }
    let payload = value.as_ref().and_then(|v| serde_json::to_string(v).ok());
    if let Err(e) = set_entry(conn, kind, key, payload.as_deref()) {
        warn!("integration-cache", "Failed to persist {} cache entry {key}: {e}", kind.as_str());
    }
    mem.insert(key.to_string(), value.clone());
    true
}

fn prune_kind<T>(conn: &Connection, kind: CacheKind, valid_keys: &[String], mem: &mut HashMap<String, Option<T>>) {
    // A momentarily empty worktree list (e.g. before the first sync) must not
    // be read as "everything is gone".
    if valid_keys.is_empty() {
        return;
    }
    match prune_rows(conn, kind, valid_keys) {
        Ok(n) if n > 0 => debug!("integration-cache", "Pruned {n} stale {} cache rows", kind.as_str()),
        Ok(_) => {}
        Err(e) => warn!("integration-cache", "Failed to prune {} cache: {e}", kind.as_str()),
    }
    let valid: std::collections::HashSet<&str> = valid_keys.iter().map(String::as_str).collect();
    mem.retain(|k, _| valid.contains(k.as_str()));
}

impl IntegrationCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load persisted payloads. Only the first call reads the DB; corrupt rows
    /// are skipped (with a WARN) instead of failing startup.
    pub fn hydrate(&mut self, conn: &Connection) {
        if self.hydrated {
            return;
        }
        self.hydrated = true;
        let prs = hydrate_kind(conn, CacheKind::Pr, &mut self.prs);
        let lin = hydrate_kind(conn, CacheKind::Linear, &mut self.linear);
        match (prs, lin) {
            (Ok(p), Ok(l)) => info!("integration-cache", "Hydrated {p} PR and {l} Linear cache entries from disk"),
            (Err(e), _) | (_, Err(e)) => warn!("integration-cache", "Failed to hydrate integration cache: {e}"),
        }
    }

    pub fn is_hydrated(&self) -> bool {
        self.hydrated
    }

    /// Write a PR fetch result; returns false (and changes nothing) for a failure.
    pub fn commit_pr(&mut self, conn: &Connection, key: &str, result: &FetchResult<PrInfo>) -> bool {
        commit(conn, CacheKind::Pr, key, &mut self.prs, result)
    }

    pub fn commit_linear(&mut self, conn: &Connection, key: &str, result: &FetchResult<LinearInfo>) -> bool {
        commit(conn, CacheKind::Linear, key, &mut self.linear, result)
    }

    pub fn prune_prs(&mut self, conn: &Connection, valid_keys: &[String]) {
        prune_kind(conn, CacheKind::Pr, valid_keys, &mut self.prs);
    }

    pub fn prune_linear(&mut self, conn: &Connection, valid_keys: &[String]) {
        prune_kind(conn, CacheKind::Linear, valid_keys, &mut self.linear);
    }

    /// The gh-sourced PR for `repo_id:branch` (no Linear fallback).
    pub fn pr(&self, repo_id: &str, branch: &str) -> Option<&PrInfo> {
        self.prs.get(&cache_key(repo_id, branch)).and_then(Option::as_ref)
    }

    pub fn linear(&self, repo_id: &str, branch: &str) -> Option<&LinearInfo> {
        self.linear.get(&cache_key(repo_id, branch)).and_then(Option::as_ref)
    }

    /// Port of `integrationFieldsFor`: the gh PR, else the PR attached to the
    /// branch's Linear ticket (when it isn't for a different branch).
    pub fn fields_for(&self, repo_id: &str, branch: &str) -> (Option<PrInfo>, Option<LinearInfo>) {
        let linear = self.linear(repo_id, branch).cloned();
        if let Some(pr) = self.pr(repo_id, branch) {
            return (Some(pr.clone()), linear);
        }
        let fallback = linear
            .as_ref()
            .and_then(|l| l.pr_attachment.as_ref())
            .filter(|a| attachment_matches_branch(a, branch))
            .map(attachment_to_pr_info);
        (fallback, linear)
    }

    /// Forget every cached Linear ticket (memory + disk), e.g. Linear disabled.
    pub fn clear_linear(&mut self, conn: &Connection) {
        self.linear.clear();
        if let Err(e) = prune_rows(conn, CacheKind::Linear, &[]) {
            warn!("integration-cache", "Failed to clear Linear cache: {e}");
        }
    }
}
