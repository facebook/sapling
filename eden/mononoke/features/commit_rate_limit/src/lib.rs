/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(test)]
mod tests;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use blobstore::Loadable;
use bookmarks::BookmarkKey;
use bookmarks::BookmarksRef;
use bookmarks::Freshness;
use commit_graph::CommitGraphRef;
use commit_rate_limit_config::CommitRateLimitRef;
use commit_rate_limit_config::CommitRateLimitRule;
use commit_rate_limit_config::EligibilityCheck;
use commit_rate_limit_config::cache::ChangesetEligibilityCache;
use commit_rate_limit_config::inspect_changeset_eligibility;
use commit_rate_limit_config::is_eligible_for_rate_limit;
use commit_rate_limit_config::matches_user_filter;
use commit_rate_limit_config::parse_author_username;
use commit_rate_limit_config::touches_directories;
use context::CoreContext;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use history_traversal::AncestorFilterOptions;
use history_traversal::matching_ancestors_stream;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use stats::prelude::*;

define_stats! {
    prefix = "mononoke.commit_rate_limit";
    eligibility_cache_public_hit: dynamic_timeseries("{}.{}.public_hit", (repo_name: String, rate_limit_name: String); Rate, Sum),
    eligibility_cache_public_miss: dynamic_timeseries("{}.{}.public_miss", (repo_name: String, rate_limit_name: String); Rate, Sum),
    eligibility_cache_draft_hit: dynamic_timeseries("{}.{}.draft_hit", (repo_name: String, rate_limit_name: String); Rate, Sum),
    eligibility_cache_draft_miss: dynamic_timeseries("{}.{}.draft_miss", (repo_name: String, rate_limit_name: String); Rate, Sum),
}

// --- Repo traits ---

/// Trait alias for repository types that provide the facets needed by
/// commit rate limiting: bookmarks, blobstore, commit graph, and derived data.
pub trait Repo:
    BookmarksRef
    + RepoBlobstoreRef
    + CommitGraphRef
    + RepoDerivedDataRef
    + Clone
    + Send
    + Sync
    + 'static
{
}

impl<T> Repo for T where
    T: BookmarksRef
        + RepoBlobstoreRef
        + CommitGraphRef
        + RepoDerivedDataRef
        + Clone
        + Send
        + Sync
        + 'static
{
}

// --- Outcome types ---

#[derive(Debug, PartialEq, Eq)]
pub enum RateLimitOutcome {
    Allowed,
    Exceeded {
        total: u64,
        window_secs: u64,
        max_commits: u64,
    },
}

impl RateLimitOutcome {
    pub fn is_allowed(&self) -> bool {
        matches!(self, RateLimitOutcome::Allowed)
    }
}

/// Result of checking all rate limit rules for a single commit.
#[derive(Debug)]
pub struct CommitRateLimitCheckResult {
    pub passed: bool,
    pub rule_results: Vec<RuleCheckResult>,
}

/// Result of checking a single rate limit rule.
#[derive(Debug)]
pub struct RuleCheckResult {
    pub rule_name: String,
    pub outcome: RateLimitOutcome,
    /// The author username the rule was scoped to, when `per_user` is true.
    /// `None` for global (non-per-user) rules, or when the author could not
    /// be parsed.
    pub user_filter: Option<String>,
    /// Directories the rule applies to. Empty when the rule applies repo-wide.
    pub directories: Vec<String>,
}

// --- Public API ---

/// Check all commit rate limit rules concurrently and aggregate results.
/// Uses `try_collect` -- if any rule returns an error, remaining futures
/// may be cancelled and the first error is propagated.
pub async fn check_all_commit_rate_limits(
    ctx: &CoreContext,
    repo: &(impl Repo + CommitRateLimitRef),
    bonsai: &BonsaiChangeset,
    _cs_id: ChangesetId,
    bookmark: &BookmarkKey,
) -> Result<CommitRateLimitCheckResult> {
    let allow_bare_unixname = justknobs::eval(
        "scm/mononoke:allow_bare_author_unixname",
        None,
        Some("commit_rate_limit"),
    )
    .unwrap_or(false);
    let rules = repo.commit_rate_limit().rules();
    let futures: Vec<_> = rules
        .iter()
        .enumerate()
        .map(|(idx, rule)| {
            let user_filter = if rule.per_user() {
                parse_author_username(bonsai.author(), allow_bare_unixname)
                    .ok()
                    .flatten()
                    .map(|s| s.to_string())
            } else {
                None
            };
            let directories = rule.directories().to_vec();
            async move {
                let outcome = check_commit_rate_limit(
                    ctx,
                    repo,
                    bookmark,
                    bonsai,
                    rule,
                    user_filter.as_deref(),
                    allow_bare_unixname,
                )
                .await?;
                anyhow::Ok((
                    idx,
                    RuleCheckResult {
                        rule_name: rule.name().to_string(),
                        outcome,
                        user_filter,
                        directories,
                    },
                ))
            }
        })
        .collect();

    let mut indexed_results: Vec<(usize, RuleCheckResult)> = stream::iter(futures)
        .buffer_unordered(20)
        .try_collect()
        .await?;
    indexed_results.sort_by_key(|(idx, _)| *idx);
    let rule_results: Vec<RuleCheckResult> = indexed_results.into_iter().map(|(_, r)| r).collect();

    let passed = rule_results.iter().all(|r| r.outcome.is_allowed());
    Ok(CommitRateLimitCheckResult {
        passed,
        rule_results,
    })
}

/// Check whether a commit should be rate-limited.
///
/// Returns `RateLimitOutcome::Allowed` if the commit is under all configured
/// limits, or `RateLimitOutcome::Exceeded` with details of the first violated
/// limit.
///
/// The `user_filter` parameter should be `Some(username)` when the config is
/// per-user, allowing the caller to scope ancestor counting to commits by a
/// specific author. Pass `None` for global (non-per-user) configs.
pub async fn check_commit_rate_limit(
    ctx: &CoreContext,
    repo: &impl Repo,
    bookmark: &BookmarkKey,
    changeset: &BonsaiChangeset,
    config: &CommitRateLimitRule,
    user_filter: Option<&str>,
    allow_bare_unixname: bool,
) -> Result<RateLimitOutcome> {
    if !touches_directories(changeset, config.directories()) {
        return Ok(RateLimitOutcome::Allowed);
    }
    if !is_eligible_for_rate_limit(config.eligibility_checks(), changeset) {
        return Ok(RateLimitOutcome::Allowed);
    }

    let cache = config.cache().cloned();

    // Draft count is independent of the time window, so compute once.
    let draft_count = count_eligible_draft_ancestors(
        ctx,
        repo,
        bookmark,
        changeset,
        config,
        user_filter,
        cache.clone(),
        allow_bare_unixname,
    )
    .await?;

    for limit in config.limits() {
        let window = Duration::from_secs(limit.window_secs());
        let public_count = count_eligible_public_ancestors(
            ctx,
            repo,
            bookmark,
            window,
            config,
            user_filter,
            cache.clone(),
            allow_bare_unixname,
        )
        .await?;

        // +1 for the eligible changeset itself (if it weren't eligible,
        // we would have returned Allowed early above).
        let total = public_count + draft_count + 1;
        if total > limit.max_commits() {
            return Ok(RateLimitOutcome::Exceeded {
                total,
                window_secs: limit.window_secs(),
                max_commits: limit.max_commits(),
            });
        }
    }

    Ok(RateLimitOutcome::Allowed)
}

// --- Private helpers ---

async fn count_eligible_public_ancestors(
    ctx: &CoreContext,
    repo: &impl Repo,
    bookmark: &BookmarkKey,
    window: Duration,
    config: &CommitRateLimitRule,
    user_filter: Option<&str>,
    cache: Option<ChangesetEligibilityCache>,
    allow_bare_unixname: bool,
) -> Result<u64> {
    let bookmark_cs_id = repo
        .bookmarks()
        .get(ctx.clone(), bookmark, Freshness::MaybeStale)
        .await?;

    let bookmark_cs_id = match bookmark_cs_id {
        Some(id) => id,
        None => return Ok(0),
    };

    let until_timestamp = chrono::Utc::now().timestamp() - window.as_secs() as i64;

    let predicate = build_cached_ancestor_predicate(
        config.eligibility_checks(),
        config.directories(),
        user_filter,
        cache,
        config.repo_name(),
        config.name(),
        allow_bare_unixname,
    );

    let opts = AncestorFilterOptions {
        until_timestamp: Some(until_timestamp),
        descendants_of: None,
        exclude_changeset_and_ancestors: None,
    };

    matching_ancestors_stream(ctx, repo, bookmark_cs_id, opts, predicate)
        .await?
        .try_fold(0u64, |acc, _| async move { Ok(acc + 1) })
        .await
}

async fn count_eligible_draft_ancestors(
    ctx: &CoreContext,
    repo: &impl Repo,
    bookmark: &BookmarkKey,
    changeset: &BonsaiChangeset,
    config: &CommitRateLimitRule,
    user_filter: Option<&str>,
    cache: Option<ChangesetEligibilityCache>,
    allow_bare_unixname: bool,
) -> Result<u64> {
    let bookmark_cs_id = repo
        .bookmarks()
        .get(ctx.clone(), bookmark, Freshness::MaybeStale)
        .await?;

    let common = match bookmark_cs_id {
        Some(id) => vec![id],
        // Bookmark doesn't exist yet — there are no draft ancestors relative
        // to a non-existent bookmark, so nothing to count.
        None => return Ok(0),
    };

    let current_cs_id = changeset.get_changeset_id();

    let stream = repo
        .commit_graph()
        .ancestors_difference_stream(ctx, vec![current_cs_id], common)
        .await?;

    let ctx = ctx.clone();
    let repo_blobstore = repo.repo_blobstore().clone();
    let checks = config.eligibility_checks().to_vec();
    let directories = config.directories().to_vec();
    let user_filter = user_filter.map(|u| u.to_owned());
    let stats_repo = config.repo_name().to_owned();
    let stats_rl = config.name().to_owned();

    stream
        .try_filter_map(move |cs_id| {
            let ctx = ctx.clone();
            let repo_blobstore = repo_blobstore.clone();
            let cache = cache.clone();
            let checks = checks.clone();
            let directories = directories.clone();
            let user_filter = user_filter.clone();
            let stats_repo = stats_repo.clone();
            let stats_rl = stats_rl.clone();
            async move {
                if cs_id == current_cs_id {
                    return Ok(None);
                }

                // When cache is available, check BEFORE loading from blobstore.
                // Cache hits skip the expensive cs_id.load() entirely.
                if let Some(ref cache) = cache {
                    if let Some(cached) = cache.lookup(&cs_id) {
                        STATS::eligibility_cache_draft_hit.add_value(1, (stats_repo, stats_rl));
                        let matches = cached
                            .as_ref()
                            .map(|i| matches_user_filter(i, user_filter.as_deref()))
                            .unwrap_or(false);
                        return if matches { Ok(Some(cs_id)) } else { Ok(None) };
                    }
                    STATS::eligibility_cache_draft_miss.add_value(1, (stats_repo, stats_rl));
                }

                // Cache miss or no cache: load from blobstore.
                let bonsai = cs_id.load(&ctx, &repo_blobstore).await?;
                let info = inspect_changeset_eligibility(
                    &bonsai,
                    &checks,
                    &directories,
                    allow_bare_unixname,
                );

                if let Some(ref cache) = cache {
                    cache.insert(cs_id, info.clone());
                }

                let matches = info
                    .as_ref()
                    .map(|i| matches_user_filter(i, user_filter.as_deref()))
                    .unwrap_or(false);
                if matches { Ok(Some(cs_id)) } else { Ok(None) }
            }
        })
        .try_fold(0u64, |acc, _| async move { Ok(acc + 1) })
        .await
}

/// Build a predicate that uses the cache (if available) to avoid redundant
/// inspection. The cache is populated synchronously since `matching_ancestors_stream`
/// already loads the `BonsaiChangeset` before calling the predicate.
fn build_cached_ancestor_predicate(
    checks: &[EligibilityCheck],
    directories: &[String],
    user_filter: Option<&str>,
    cache: Option<ChangesetEligibilityCache>,
    repo_name: &str,
    name: &str,
    allow_bare_unixname: bool,
) -> Arc<dyn Fn(&BonsaiChangeset) -> bool + Send + Sync> {
    match cache {
        Some(cache) => {
            let checks = checks.to_vec();
            let directories = directories.to_vec();
            let user_filter = user_filter.map(|u| u.to_owned());
            let stats_repo = repo_name.to_owned();
            let stats_rl = name.to_owned();
            Arc::new(move |changeset: &BonsaiChangeset| {
                let cs_id = changeset.get_changeset_id();
                let info = cache.get_or_insert_with(
                    cs_id,
                    || {
                        STATS::eligibility_cache_public_hit
                            .add_value(1, (stats_repo.clone(), stats_rl.clone()));
                    },
                    || {
                        STATS::eligibility_cache_public_miss
                            .add_value(1, (stats_repo.clone(), stats_rl.clone()));
                        inspect_changeset_eligibility(
                            changeset,
                            &checks,
                            &directories,
                            allow_bare_unixname,
                        )
                    },
                );

                info.as_ref()
                    .map(|i| matches_user_filter(i, user_filter.as_deref()))
                    .unwrap_or(false)
            })
        }
        None => build_ancestor_predicate(checks, directories, user_filter, allow_bare_unixname),
    }
}

fn build_ancestor_predicate(
    checks: &[EligibilityCheck],
    directories: &[String],
    user_filter: Option<&str>,
    allow_bare_unixname: bool,
) -> Arc<dyn Fn(&BonsaiChangeset) -> bool + Send + Sync> {
    let checks = checks.to_vec();
    let directories = directories.to_vec();
    let user_filter = user_filter.map(|u| u.to_owned());

    Arc::new(move |changeset: &BonsaiChangeset| {
        let info = match inspect_changeset_eligibility(
            changeset,
            &checks,
            &directories,
            allow_bare_unixname,
        ) {
            Some(info) => info,
            None => return false,
        };
        matches_user_filter(&info, user_filter.as_deref())
    })
}
