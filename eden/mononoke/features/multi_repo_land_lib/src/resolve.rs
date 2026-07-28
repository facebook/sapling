/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Bulk-resolve `(repo_name, bookmark_name)` pairs to git SHA1s across repos
//! that share one MySQL shard, using `IN (...)` queries.

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use bookmarks::BookmarkName;
use bookmarks::Freshness;
use context::CoreContext;
use dbbookmarks::store::SqlBookmarksRef;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use mononoke_types::hash::GitSha1;
use repo_identity::RepoIdentityRef;
use sql_ext::mononoke_queries;

use crate::repo_provider::RepoProvider;

const BATCH_SIZE: usize = 500;

mononoke_queries! {
    read BulkResolveBookmarksCrossRepo(
        >tuple_list values: (repo_id: RepositoryId, name: BookmarkName)
    ) -> (RepositoryId, BookmarkName, ChangesetId) {
        "SELECT repo_id, name, changeset_id
         FROM bookmarks
         WHERE (repo_id, name) IN {values}"
    }

    read BulkGitMappingCrossRepo(
        >tuple_list values: (repo_id: RepositoryId, bcs_id: ChangesetId)
    ) -> (RepositoryId, ChangesetId, GitSha1) {
        "SELECT repo_id, bcs_id, git_sha1
         FROM bonsai_git_mapping
         WHERE (repo_id, bcs_id) IN {values}"
    }
}

/// A `(repo_name, bookmark_name)` pair to resolve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveEntry {
    pub repo_name: String,
    pub bookmark_name: String,
}

/// Outcome of resolving a single entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveOutcome {
    Resolved(GitSha1),
    /// Human-readable reason resolution failed.
    Error(String),
}

/// Per-entry result; results are returned in input order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveResult {
    pub repo_name: String,
    pub bookmark_name: String,
    pub outcome: ResolveOutcome,
}

/// Output mirrors `entries` element-for-element.
///
/// # Precondition
///
/// All `provider` repos MUST share one MySQL metadata shard: every SQL read runs
/// against the first resolved repo's connection. Cross-shard callers get
/// silently-wrong per-entry errors (spurious "bookmark not found" / "git mapping
/// not found") — there is no runtime shard check.
pub async fn resolve_bookmarks_cross_repo<R>(
    ctx: &CoreContext,
    provider: &dyn RepoProvider<R>,
    entries: &[ResolveEntry],
) -> Result<Vec<ResolveResult>>
where
    R: RepoIdentityRef + SqlBookmarksRef + Send + Sync,
{
    if entries.is_empty() {
        return Ok(vec![]);
    }

    // resolved: (original_index, repo_id, repo_name, bookmark_name)
    let mut resolved: Vec<(usize, RepositoryId, String, String)> = Vec::new();
    let mut results: Vec<Option<ResolveResult>> = vec![None; entries.len()];
    let mut any_repo: Option<Arc<R>> = None;

    for (idx, entry) in entries.iter().enumerate() {
        match provider.get_by_name(&entry.repo_name) {
            Some(repo) => {
                let repo_id = repo.repo_identity().id();
                if any_repo.is_none() {
                    any_repo = Some(repo);
                }
                resolved.push((
                    idx,
                    repo_id,
                    entry.repo_name.clone(),
                    entry.bookmark_name.clone(),
                ));
            }
            None => {
                if let Some(slot) = results.get_mut(idx) {
                    *slot = Some(ResolveResult {
                        repo_name: entry.repo_name.clone(),
                        bookmark_name: entry.bookmark_name.clone(),
                        outcome: ResolveOutcome::Error(format!(
                            "unknown repo: {}",
                            entry.repo_name
                        )),
                    });
                }
            }
        }
    }

    if resolved.is_empty() {
        return Ok(results.into_iter().flatten().collect());
    }

    // Exact (repo_id, name) tuple IN-clause — no cross-product false positives.
    let mut bookmark_pairs_seen: HashSet<(RepositoryId, BookmarkName)> = HashSet::new();
    let mut bookmark_pairs: Vec<(RepositoryId, BookmarkName)> = Vec::new();

    for (idx, repo_id, repo_name, bm_name) in &resolved {
        match BookmarkName::new(bm_name) {
            Ok(bm) => {
                if bookmark_pairs_seen.insert((*repo_id, bm.clone())) {
                    bookmark_pairs.push((*repo_id, bm));
                }
            }
            Err(_) => {
                if let Some(slot) = results.get_mut(*idx) {
                    *slot = Some(ResolveResult {
                        repo_name: repo_name.clone(),
                        bookmark_name: bm_name.clone(),
                        outcome: ResolveOutcome::Error(format!("invalid bookmark name: {bm_name}")),
                    });
                }
            }
        }
    }

    // AOSP repos share one shard — any repo's connection works.
    let any_repo = any_repo.ok_or_else(|| {
        anyhow::anyhow!("internal error: resolved is non-empty but no repo was found")
    })?;
    let conn = any_repo
        .sql_bookmarks()
        .connection(ctx, Freshness::MostRecent);

    let mut bookmark_map: HashMap<(RepositoryId, String), ChangesetId> = HashMap::new();

    for chunk in bookmark_pairs.chunks(BATCH_SIZE) {
        let rows = BulkResolveBookmarksCrossRepo::query(conn, ctx.sql_query_telemetry(), chunk)
            .await
            .with_context(|| {
                format!(
                    "Failed to query bookmarks for chunk of {} pairs",
                    chunk.len()
                )
            })?;
        for (repo_id, name, cs_id) in rows {
            bookmark_map.insert((repo_id, name.to_string()), cs_id);
        }
    }

    let mut cs_id_pairs: HashSet<(RepositoryId, ChangesetId)> = HashSet::new();

    for (idx, repo_id, repo_name, bookmark_name) in &resolved {
        if results.get(*idx).is_some_and(|r| r.is_some()) {
            // already errored above
            continue;
        }
        let key = (*repo_id, bookmark_name.clone());
        match bookmark_map.get(&key) {
            Some(cs_id) => {
                cs_id_pairs.insert((*repo_id, *cs_id));
            }
            None => {
                if let Some(slot) = results.get_mut(*idx) {
                    *slot = Some(ResolveResult {
                        repo_name: repo_name.clone(),
                        bookmark_name: bookmark_name.clone(),
                        outcome: ResolveOutcome::Error("bookmark not found".to_string()),
                    });
                }
            }
        }
    }

    // Bulk git-mapping lookup, same exact-tuple IN-clause.
    let mut git_map: HashMap<(RepositoryId, ChangesetId), GitSha1> = HashMap::new();

    if !cs_id_pairs.is_empty() {
        let git_pairs: Vec<(RepositoryId, ChangesetId)> = cs_id_pairs.iter().copied().collect();

        for chunk in git_pairs.chunks(BATCH_SIZE) {
            let rows = BulkGitMappingCrossRepo::query(conn, ctx.sql_query_telemetry(), chunk)
                .await
                .with_context(|| {
                    format!(
                        "Failed to query git mappings for chunk of {} pairs",
                        chunk.len()
                    )
                })?;
            for (repo_id, bcs_id, git_sha1) in rows {
                git_map.insert((repo_id, bcs_id), git_sha1);
            }
        }
    }

    // Assemble in original request order.
    for (idx, repo_id, repo_name, bookmark_name) in &resolved {
        if results.get(*idx).is_some_and(|r| r.is_some()) {
            // already errored above
            continue;
        }
        let bm_key = (*repo_id, bookmark_name.clone());
        if let Some(cs_id) = bookmark_map.get(&bm_key) {
            let outcome = match git_map.get(&(*repo_id, *cs_id)) {
                Some(git_sha1) => ResolveOutcome::Resolved(*git_sha1),
                None => ResolveOutcome::Error("git mapping not found".to_string()),
            };
            if let Some(slot) = results.get_mut(*idx) {
                *slot = Some(ResolveResult {
                    repo_name: repo_name.clone(),
                    bookmark_name: bookmark_name.clone(),
                    outcome,
                });
            }
        }
    }

    Ok(results.into_iter().flatten().collect())
}
