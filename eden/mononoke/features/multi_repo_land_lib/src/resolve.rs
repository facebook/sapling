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

    read BulkBonsaiByGitCrossRepo(
        >tuple_list values: (repo_id: RepositoryId, git_sha1: GitSha1)
    ) -> (RepositoryId, GitSha1, ChangesetId) {
        "SELECT repo_id, git_sha1, bcs_id
         FROM bonsai_git_mapping
         WHERE (repo_id, git_sha1) IN {values}"
    }
}

/// Bookmark values for exact `(repo_id, name)` pairs, read with chunked
/// tuple-IN queries on `conn_repo`'s shard connection. No per-repo facet is
/// involved, so N distinct repos still cost `ceil(pairs / 500)` queries, not
/// N. Pairs without a row — the bookmark does not exist — are absent from the
/// returned map.
///
/// # Precondition
///
/// Every `repo_id` MUST live on `conn_repo`'s MySQL metadata shard; a
/// cross-shard pair silently reads as absent — there is no runtime check.
pub async fn bulk_read_bookmarks<R>(
    ctx: &CoreContext,
    conn_repo: &R,
    freshness: Freshness,
    pairs: &[(RepositoryId, BookmarkName)],
) -> Result<HashMap<(RepositoryId, BookmarkName), ChangesetId>>
where
    R: SqlBookmarksRef + Send + Sync,
{
    let mut seen: HashSet<&(RepositoryId, BookmarkName)> = HashSet::new();
    let unique: Vec<(RepositoryId, BookmarkName)> = pairs
        .iter()
        .filter(|pair| seen.insert(*pair))
        .cloned()
        .collect();

    let conn = conn_repo.sql_bookmarks().connection(ctx, freshness);
    let mut values = HashMap::new();
    for chunk in unique.chunks(BATCH_SIZE) {
        let rows = BulkResolveBookmarksCrossRepo::query(conn, ctx.sql_query_telemetry(), chunk)
            .await
            .with_context(|| {
                format!(
                    "Failed to query bookmarks for chunk of {} pairs",
                    chunk.len()
                )
            })?;
        for (repo_id, name, cs_id) in rows {
            values.insert((repo_id, name), cs_id);
        }
    }
    Ok(values)
}

/// Git identities for exact `(repo_id, changeset)` pairs, read with chunked
/// tuple-IN queries on `conn_repo`'s shard connection. Pairs with no mapping
/// row are absent from the returned map.
///
/// # Precondition
///
/// Same single-shard requirement as [`bulk_read_bookmarks`].
pub async fn bulk_read_git_sha1s<R>(
    ctx: &CoreContext,
    conn_repo: &R,
    freshness: Freshness,
    pairs: &[(RepositoryId, ChangesetId)],
) -> Result<HashMap<(RepositoryId, ChangesetId), GitSha1>>
where
    R: SqlBookmarksRef + Send + Sync,
{
    let unique: Vec<(RepositoryId, ChangesetId)> = pairs
        .iter()
        .copied()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    let conn = conn_repo.sql_bookmarks().connection(ctx, freshness);
    let mut values = HashMap::new();
    for chunk in unique.chunks(BATCH_SIZE) {
        let rows = BulkGitMappingCrossRepo::query(conn, ctx.sql_query_telemetry(), chunk)
            .await
            .with_context(|| {
                format!(
                    "Failed to query git mappings for chunk of {} pairs",
                    chunk.len()
                )
            })?;
        for (repo_id, bcs_id, git_sha1) in rows {
            values.insert((repo_id, bcs_id), git_sha1);
        }
    }
    Ok(values)
}

/// Bonsai ids for exact `(repo_id, git_sha1)` pairs — the reverse of
/// [`bulk_read_git_sha1s`], for callers handed git ids. Pairs with no mapping
/// row are absent from the returned map.
///
/// # Precondition
///
/// Same single-shard requirement as [`bulk_read_bookmarks`].
pub async fn bulk_read_bonsais_by_git_sha1<R>(
    ctx: &CoreContext,
    conn_repo: &R,
    freshness: Freshness,
    pairs: &[(RepositoryId, GitSha1)],
) -> Result<HashMap<(RepositoryId, GitSha1), ChangesetId>>
where
    R: SqlBookmarksRef + Send + Sync,
{
    let unique: Vec<(RepositoryId, GitSha1)> = pairs
        .iter()
        .copied()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    let conn = conn_repo.sql_bookmarks().connection(ctx, freshness);
    let mut values = HashMap::new();
    for chunk in unique.chunks(BATCH_SIZE) {
        let rows = BulkBonsaiByGitCrossRepo::query(conn, ctx.sql_query_telemetry(), chunk)
            .await
            .with_context(|| {
                format!(
                    "Failed to query bonsais by git sha for chunk of {} pairs",
                    chunk.len()
                )
            })?;
        for (repo_id, git_sha1, bcs_id) in rows {
            values.insert((repo_id, git_sha1), bcs_id);
        }
    }
    Ok(values)
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

    let bookmark_map: HashMap<(RepositoryId, String), ChangesetId> = bulk_read_bookmarks(
        ctx,
        any_repo.as_ref(),
        Freshness::MostRecent,
        &bookmark_pairs,
    )
    .await?
    .into_iter()
    .map(|((repo_id, name), cs_id)| ((repo_id, name.to_string()), cs_id))
    .collect();

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
    let git_pairs: Vec<(RepositoryId, ChangesetId)> = cs_id_pairs.iter().copied().collect();
    let git_map =
        bulk_read_git_sha1s(ctx, any_repo.as_ref(), Freshness::MostRecent, &git_pairs).await?;

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
