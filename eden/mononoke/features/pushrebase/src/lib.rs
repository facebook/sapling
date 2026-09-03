/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Mononoke pushrebase implementation. The main goal of pushrebase is to decrease push contention.
//! Commits that client pushed are rebased on top of `onto_bookmark` on the server
//!
//!  Client
//!  ```text
//!     O <- `onto` on client, potentially outdated
//!     |
//!     O  O <- pushed set (in this case just one commit)
//!     | /
//!     O <- root
//!  ```
//!
//!  Server
//!  ```text
//!     O  <- update `onto` bookmark, pointing at the pushed commit
//!     |
//!     O  <- `onto` bookmark on the server before the push
//!     |
//!     O
//!     |
//!     O
//!     |
//!     O <- root
//!  ```
//!
//!  Terminology:
//!  *onto bookmark* - bookmark that is the destination of the rebase, for example "master"
//!
//!  *pushed set* - a set of commits that client has sent us.
//!  Note: all pushed set MUST be committed before doing pushrebase
//!  Note: pushed set MUST contain only one head
//!  Note: not all commits from pushed set maybe rebased on top of onto bookmark. See *rebased set*
//!
//!  *root* - parents of pushed set that are not in the pushed set (see graphs above)
//!
//!  *rebased set* - subset of pushed set that will be rebased on top of onto bookmark
//!  Note: Usually rebased set == pushed set. However in case of merges it may differ
//!
//! Pushrebase supports hooks, which can be used to modify rebased Bonsai commits as well as
//! sideload database updates in the transaction that moves forward the bookmark. See hooks.rs for
//! more information on those;

#![feature(trait_alias)]

use std::cmp::max;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::hash_map::Entry;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::format_err;
use blobrepo_utils::convert_diff_result_into_file_change_for_diamond_merge;
use blobstore::Loadable;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkTransactionHook;
use bookmarks::BookmarkUpdateLogId;
use bookmarks::BookmarkUpdateReason;
use bookmarks::BookmarksRef;
use bytes::Bytes;
use commit_graph::CommitGraphRef;
use commit_graph::CommitGraphWriterRef;
use content_manifest_derivation::RootContentManifestId;
use context::CoreContext;
use dbbookmarks::SqlBookmarksRef;
use derivation_queue_thrift::DerivationPriority;
use filenodes_derivation::FilenodesOnlyPublic;
use filestore::FilestoreConfigRef;
use fsnodes::RootFsnodeId;
use futures::FutureExt;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStream;
use futures::TryStreamExt;
use futures::future;
use futures::future::try_join;
use futures::future::try_join_all;
use futures::stream;
use manifest::BonsaiDiffFileChange;
use manifest::ManifestOps;
use manifest::bonsai_diff;
use maplit::hashmap;
use mercurial_derivation::DeriveHgChangeset;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mercurial_types::NonRootMPath;
use metaconfig_types::MergeResolutionOverride;
use metaconfig_types::PushrebaseFlags;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::DateTime;
use mononoke_types::DerivableType;
use mononoke_types::FileChange;
use mononoke_types::FileType;
use mononoke_types::GitLfs;
use mononoke_types::MPath;
use mononoke_types::PrefixTrie;
use mononoke_types::Timestamp;
use mononoke_types::check_case_conflicts;
use mononoke_types::content_manifest::compat;
use mononoke_types::find_path_conflicts;
use pushrebase_hook::PushrebaseCommitHook;
use pushrebase_hook::PushrebaseHook;
use pushrebase_hook::PushrebaseTransactionHook;
use pushrebase_hook::RebasedChangesets;
use repo_blobstore::RepoBlobstoreArc;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentityRef;
use shared_error::std::SharedError;
use stats::prelude::*;
use thiserror::Error;

mod merge_resolution_summary;
pub use merge_resolution_summary::MR_PATH_SAMPLE_CAP;
pub use merge_resolution_summary::MergeResolutionSummary;
use three_way_merge::MergeResult;
use three_way_merge::merge_text;
use tokio::sync::oneshot;
use tracing::info;
use tracing::warn;

define_stats! {
    prefix = "mononoke.pushrebase";
    // Clowntown: This is actually nanoseconds (ns), not microseconds (us)
    critical_section_success_duration_us: dynamic_timeseries("{}.critical_section_success_duration_us", (reponame: String); Average, Sum, Count),
    critical_section_failure_duration_us: dynamic_timeseries("{}.critical_section_failure_duration_us", (reponame: String); Average, Sum, Count),
    critical_section_retries_failed: dynamic_timeseries("{}.critical_section_retries_failed", (reponame: String); Average, Sum),
    commits_rebased: dynamic_timeseries("{}.commits_rebased", (reponame: String); Average, Sum, Count),
    conflict_rejections: dynamic_timeseries("{}.conflict_rejections", (reponame: String); Count),
    conflict_files_count: dynamic_timeseries("{}.conflict_files_count", (reponame: String); Average, Sum, Count),
    merge_resolution_lost_on_retry: dynamic_timeseries("{}.merge_resolution_lost_on_retry", (reponame: String); Count),
    noop_merge_commits_detected: dynamic_timeseries("{}.noop_merge_commits_detected", (reponame: String); Count),
    noop_merge_commits_rejected: dynamic_timeseries("{}.noop_merge_commits_rejected", (reponame: String); Count),
}

const MAX_REBASE_ATTEMPTS: usize = 100;

pub const MUTATION_KEYS: &[&str] = &["mutpred", "mutuser", "mutdate", "mutop", "mutsplit"];

pub const FAIL_PUSHREBASE_EXTRA: &str = "failpushrebase";

#[derive(Debug, Error)]
pub enum PushrebaseInternalError {
    #[error("Bonsai not found for hg changeset: {0}")]
    BonsaiNotFoundForHgChangeset(HgChangesetId),
    #[error("Pushrebase onto bookmark not found: {0}")]
    PushrebaseBookmarkNotFound(BookmarkKey),
    #[error("Only one head is allowed in pushed set")]
    PushrebaseTooManyHeads,
    #[error("No common pushrebase root for {0}, all possible roots: {1:?}")]
    PushrebaseNoCommonRoot(BookmarkKey, HashSet<ChangesetId>),
    #[error("No pushrebase roots found")]
    NoRoots,
    #[error("Pushrebase failed after too many unsuccessful rebases")]
    TooManyRebaseAttempts,
    #[error("Forbid pushrebase because root ({0}) is not a p1 of {1} bookmark")]
    P2RootRebaseForbidden(HgChangesetId, BookmarkKey),
    #[error("Unexpected file conflicts when adding new file changes to {0}")]
    NewFileChangesConflict(ChangesetId),
    #[error("Merge resolution was performed in a previous attempt but lost on retry")]
    MergeResolutionLostOnRetry,
}

#[derive(Debug, Error)]
pub enum PushrebaseError {
    #[error("Conflicts while pushrebasing: {0:?}")]
    Conflicts(Vec<PushrebaseConflict>),
    #[error(
        "PotentialCaseConflict: the change this commit introduces at {0} may conflict with other commits. Rebase and retry."
    )]
    PotentialCaseConflict(NonRootMPath),
    #[error("Pushrebase over merge")]
    RebaseOverMerge,
    #[error("Root is too far behind")]
    RootTooFarBehind,
    #[error(
        "Force failed pushrebase, please do a manual rebase. (Bonsai changeset id that triggered it is {0})"
    )]
    ForceFailPushrebase(ChangesetId),
    #[error(transparent)]
    Error(#[from] Error),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PushrebaseConflict {
    pub left: MPath,
    pub right: MPath,
}

impl PushrebaseConflict {
    fn new(left: MPath, right: MPath) -> Self {
        PushrebaseConflict { left, right }
    }
}

impl From<PushrebaseInternalError> for PushrebaseError {
    fn from(error: PushrebaseInternalError) -> Self {
        PushrebaseError::Error(error.into())
    }
}

#[derive(Debug, Clone)]
pub struct PushrebaseChangesetPair {
    pub id_old: ChangesetId,
    pub id_new: ChangesetId,
}

fn rebased_changesets_into_pairs(
    rebased_changesets: RebasedChangesets,
) -> Vec<PushrebaseChangesetPair> {
    rebased_changesets
        .into_iter()
        .map(|(id_old, (id_new, _))| PushrebaseChangesetPair { id_old, id_new })
        .collect()
}

/// Adapter that bridges the carry-forward state (`Vec<MergedFileInfo>`)
/// to `MergeResolutionSummary::from_carried_paths`. Returns `None` when
/// no MR ran in any prior attempt; otherwise a `Succeeded` summary
/// reconstructed from the path list.
fn synthesize_carried_summary(carried: &[MergedFileInfo]) -> Option<MergeResolutionSummary> {
    if carried.is_empty() {
        return None;
    }
    let paths: Vec<NonRootMPath> = carried.iter().map(|info| info.path.clone()).collect();
    Some(MergeResolutionSummary::from_carried_paths(paths))
}

#[derive(Debug, Clone, Copy)]
pub struct PushrebaseRetryNum(pub usize);

#[derive(Debug, Clone, Copy)]
pub struct PushrebaseDistance(pub usize);

#[derive(Debug, Clone)]
pub struct PushrebaseOutcome {
    pub old_bookmark_value: Option<ChangesetId>,
    pub head: ChangesetId,
    pub retry_num: PushrebaseRetryNum,
    pub rebased_changesets: Vec<PushrebaseChangesetPair>,
    pub pushrebase_distance: PushrebaseDistance,
    pub log_id: BookmarkUpdateLogId,
    /// Paths that were resolved via server-side 3-way merge.
    /// `None` means no merge resolution was performed (no conflicts, or feature disabled).
    /// `Some(paths)` means these paths had conflicting edits that were auto-merged.
    pub merge_resolved_paths: Option<Vec<NonRootMPath>>,
    /// Per-push merge-resolution summary. `None` during the staged rollout
    /// when not yet populated by all pushrebase paths; once every path
    /// populates this, the field will be tightened to required.
    /// See `MergeResolutionSummary::add_to_scuba` for the Scuba schema.
    pub merge_summary: Option<MergeResolutionSummary>,
}

/// A pushed stack prepared for conflict checking and rebasing.
pub struct PushrebaseStack {
    /// Changed files in the pushed stack.
    pub changed_files: Vec<MPath>,
    /// Bonsai changesets to rebase, topological order (ancestor first).
    pub changesets: Vec<BonsaiChangeset>,
    /// Head of the pushed stack.
    pub head: ChangesetId,
    /// Root of the pushed stack.
    pub root: ChangesetId,
}

pub struct PushrebaseRequest {
    pub stack: PushrebaseStack,
    /// Last bookmark value checked for conflicts. Updated on CAS-failure re-queue.
    pub conflict_check_base: ChangesetId,
    /// Carried merge resolution info from previous CAS-failure attempts.
    /// On retry, reconciled with new delta info to preserve O(delta) scans.
    pub carried_merge_file_info: Vec<MergedFileInfo>,
    /// Number of times this request has been retried due to CAS failures.
    pub retry_num: PushrebaseRetryNum,
    /// Pre-computed pushrebase hooks.
    pub hooks: Vec<Box<dyn PushrebaseHook>>,
    /// Channel for returning the result to the caller. Uses SharedError for cloneable error broadcasting.
    pub response_tx: oneshot::Sender<Result<PushrebaseOutcome, SharedError<PushrebaseError>>>,
}

pub trait Repo = BookmarksRef
    + RepoBlobstoreArc
    + RepoDerivedDataRef
    + RepoIdentityRef
    + FilestoreConfigRef
    + CommitGraphRef
    + CommitGraphWriterRef
    + Send
    + Sync;

/// Extended repo trait for pessimistic pushrebase, which needs direct
/// access to `SqlBookmarks` for `LockedBookmarkTransaction`.
pub trait PushrebaseRepo = Repo + SqlBookmarksRef;

/// Does a pushrebase of a list of commits `pushed` onto `onto_bookmark`
/// The commits from the pushed set should already be committed to the blobrepo
/// Returns updated bookmark value.
pub async fn do_pushrebase_bonsai(
    ctx: &CoreContext,
    repo: &impl PushrebaseRepo,
    config: &PushrebaseFlags,
    onto_bookmark: &BookmarkKey,
    pushed: &HashSet<BonsaiChangeset>,
    prepushrebase_hooks: &[Box<dyn PushrebaseHook>],
) -> Result<PushrebaseOutcome, PushrebaseError> {
    // Tag every Scuba sample emitted during this pushrebase with the
    // land-attribution keys from the request pushvars, so downstream
    // samples can be joined back to the originating land job and diff.
    // Rebinding `ctx` here means every downstream `ctx.scuba()` clone
    // inherits the fields.
    let ctx = ctx.with_mutated_scuba(|mut scuba| {
        // Per-land key to roll a land's attempts up to a terminal outcome.
        if let Some(land_instance_id) = config.land_instance_id.as_deref() {
            scuba.add("land_instance_id", land_instance_id);
        }
        // Phabricator diff FBID for per-diff attribution.
        if let Some(phab_diff_id) = config.phab_diff_id.as_deref() {
            scuba.add("phab_diff_id", phab_diff_id);
        }
        scuba
    });
    let ctx = &ctx;

    let stack = index_pushrebase_request(ctx, repo, config, onto_bookmark, pushed).await?;

    let use_pessimistic = justknobs::eval(
        "scm/mononoke:per_bookmark_locking",
        None,
        Some(&repo.repo_identity().id().to_string()),
    ) && config.pessimistic_locking_bookmarks.contains(onto_bookmark);

    if use_pessimistic {
        return rebase_with_lock(
            ctx,
            repo,
            config,
            onto_bookmark,
            &stack,
            prepushrebase_hooks,
        )
        .await;
    }

    rebase_in_loop(
        ctx,
        repo,
        config,
        onto_bookmark,
        &stack,
        prepushrebase_hooks,
    )
    .await
}

/// Computes changed files, head, root, and bonsai changesets for a pushed
/// stack.
pub async fn index_pushrebase_request(
    ctx: &CoreContext,
    repo: &impl Repo,
    config: &PushrebaseFlags,
    onto_bookmark: &BookmarkKey,
    pushed: &HashSet<BonsaiChangeset>,
) -> Result<PushrebaseStack, PushrebaseError> {
    let head = find_only_head_or_fail(pushed)?;
    let roots = find_roots(pushed);
    let root = find_closest_root(ctx, repo, config, onto_bookmark, &roots).await?;

    let client_bcs = fetch_bonsai_range_ancestor_not_included(ctx, repo, root, head).await?;
    let client_cf = find_changed_files_with(
        ctx,
        repo,
        root,
        head,
        &client_bcs,
        RangeDiffManifests::FromKnob,
    )
    .await?;

    check_filenodes_backfilled(ctx, repo, &head, config.not_generated_filenodes_limit).await?;

    Ok(PushrebaseStack {
        changed_files: client_cf,
        changesets: client_bcs,
        head,
        root,
    })
}

#[derive(Debug)]
pub struct RebasedStack {
    pub new_head: ChangesetId,
    pub rebased_changesets: Vec<PushrebaseChangesetPair>,
    /// NOT yet saved; the caller must persist before referencing.
    pub rebased_bonsais: Vec<BonsaiChangeset>,
    pub merge_summary: MergeResolutionSummary,
}

/// Rebases the linear stack `root..head` onto `onto` with pushrebase's
/// conflict detection and commit rewrite, moving no bookmark and creating no
/// changesets; merged file content is stored when merge resolution is on.
/// Runs no hooks; honors the caller's merge resolution setting. With it on, a
/// replay of an already-landed stack merges to a no-op rather than conflicting
/// on paths, so callers relying on replay failing closed must keep it off or
/// rely on the no-op merge commit rejection. Requires `rewritedates == false`
/// (checked) — date stamping makes the rewrite non-deterministic, breaking
/// callers' retry termination. Rejects merge commits in the stack; uses
/// content-manifest range diffs so repos without HG derived data work.
pub async fn rebase_stack_onto(
    ctx: &CoreContext,
    repo: &impl Repo,
    config: &PushrebaseFlags,
    root: ChangesetId,
    head: ChangesetId,
    onto: ChangesetId,
) -> Result<RebasedStack, PushrebaseError> {
    if config.rewritedates {
        return Err(PushrebaseError::Error(anyhow!(
            "rebase_stack_onto requires rewritedates to be off: date stamping \
             makes the rewrite non-deterministic, breaking retry termination"
        )));
    }

    let (root_is_ancestor_of_head, root_is_ancestor_of_onto) = try_join(
        repo.commit_graph().is_ancestor(ctx, root, head),
        repo.commit_graph().is_ancestor(ctx, root, onto),
    )
    .await
    .map_err(PushrebaseError::Error)?;
    if !root_is_ancestor_of_head {
        return Err(PushrebaseError::Error(anyhow!(
            "rebase_stack_onto: root {root} must be an ancestor of head {head}, but is not"
        )));
    }
    if !root_is_ancestor_of_onto {
        return Err(PushrebaseError::Error(anyhow!(
            "rebase_stack_onto: root {root} must be an ancestor of onto {onto}, but is not \
             (was the destination force-moved off the shared history?)"
        )));
    }

    let client_bcs = fetch_bonsai_range_ancestor_not_included(ctx, repo, root, head).await?;
    if client_bcs.is_empty() {
        return Err(PushrebaseError::Error(anyhow!(
            "rebase_stack_onto: empty stack between root {root} and head {head}"
        )));
    }
    if let Some(merge) = client_bcs.iter().find(|bcs| bcs.is_merge()) {
        return Err(PushrebaseError::Error(anyhow!(
            "rebase_stack_onto does not support merge commits in the stack: {}",
            merge.get_changeset_id()
        )));
    }

    let client_cf = find_changed_files_with(
        ctx,
        repo,
        root,
        head,
        &client_bcs,
        RangeDiffManifests::ContentCompat,
    )
    .await?;

    let conflict_result = check_pushrebase_conflicts_with(
        ctx,
        repo,
        config,
        root,
        root,
        onto,
        &client_bcs,
        &client_cf,
        RangeDiffManifests::ContentCompat,
    )
    .await?;

    let mut no_hooks: Vec<Box<dyn PushrebaseCommitHook>> = Vec::new();
    let (new_head, rebased_changesets, rebased_bonsais) = create_rebased_changesets(
        ctx,
        repo,
        config,
        &client_bcs,
        root,
        head,
        onto,
        &mut no_hooks,
        conflict_result.merged_file_overrides,
    )
    .await?;

    Ok(RebasedStack {
        new_head,
        rebased_changesets: rebased_changesets_into_pairs(rebased_changesets),
        rebased_bonsais,
        merge_summary: conflict_result.merge_summary,
    })
}

/// A successfully rebased request pending the CAS bookmark update.
struct PendingRebase {
    request: PushrebaseRequest,
    new_head: ChangesetId,
    pushrebase_distance: usize,
    old_bookmark_value: Option<ChangesetId>,
    merge_resolved_paths: Option<Vec<NonRootMPath>>,
    merge_summary: MergeResolutionSummary,
}

/// Result of a speculative (pre-lock) conflict check.
struct SpeculativeConflictResult {
    bookmark_value: ChangesetId,
    merge_info: Vec<MergedFileInfo>,
    server_changeset_count: usize,
    merge_summary: MergeResolutionSummary,
}

/// Output of a successful rebase under lock, ready to be committed.
struct RebaseUnderLockResult {
    new_head: ChangesetId,
    rebased_changesets: RebasedChangesets,
    txn_hooks: Vec<Box<dyn PushrebaseTransactionHook>>,
    merge_resolved_paths: Option<Vec<NonRootMPath>>,
    pushrebase_distance: usize,
    merge_summary: MergeResolutionSummary,
}

/// Lands multiple indexed stacks in a single critical section pass.
///
/// All requests must have equivalent hooks (only the first request's hooks
/// are used) and must not have file conflicts with each other.
///
/// Takes ownership of requests. Sends results via each request's oneshot
/// for resolved requests. Returns only CAS-failure requests for re-queuing
/// with updated `conflict_check_base`.
pub async fn do_batched_pushrebase(
    ctx: &CoreContext,
    repo: &impl PushrebaseRepo,
    config: &PushrebaseFlags,
    onto_bookmark: &BookmarkKey,
    requests: Vec<PushrebaseRequest>,
) -> Vec<PushrebaseRequest> {
    let ctx = ctx.with_mutated_scuba(|mut scuba| {
        // Per-land key to roll a land's attempts up to a terminal outcome.
        if let Some(land_instance_id) = config.land_instance_id.as_deref() {
            scuba.add("land_instance_id", land_instance_id);
        }
        // Phabricator diff FBID for per-diff attribution.
        if let Some(phab_diff_id) = config.phab_diff_id.as_deref() {
            scuba.add("phab_diff_id", phab_diff_id);
        }
        scuba
    });
    let ctx = &ctx;

    let use_pessimistic = justknobs::eval(
        "scm/mononoke:per_bookmark_locking",
        None,
        Some(&repo.repo_identity().id().to_string()),
    ) && config.pessimistic_locking_bookmarks.contains(onto_bookmark);

    if use_pessimistic {
        return batched_rebase_with_lock(ctx, repo, config, onto_bookmark, requests).await;
    }

    let should_log = config.monitoring_bookmark.as_deref() == Some(onto_bookmark.as_str());
    // Parallel all-bookmarks saturation counters: fire for EVERY bookmark's
    // land, but only in repos already tracked in ODS (monitoring_bookmark set).
    let emit_all_bookmarks = config.monitoring_bookmark.is_some();
    let reponame = repo.repo_identity().name();
    let repo_args = (reponame.to_string(),);
    let start_critical_section = Instant::now();

    // CRITICAL SECTION START: Read the current bookmark value.
    let old_bookmark_value = match get_bookmark_value(ctx, repo, onto_bookmark).await {
        Ok(v) => v,
        Err(e) => {
            let shared = SharedError::from(e);
            for req in requests {
                let _ = req.response_tx.send(Err(shared.clone()));
            }
            return vec![];
        }
    };

    if requests.is_empty() {
        return vec![];
    }

    // Run hooks' in_critical_section using the first request's hooks
    // (all requests in the batch share equivalent hooks).
    let hooks_result = try_join_all(requests[0].hooks.iter().map(|h| {
        h.in_critical_section(ctx, old_bookmark_value)
            .map_err(PushrebaseError::from)
    }))
    .await;

    let mut commit_hooks = match hooks_result {
        Ok(h) => h,
        Err(e) => {
            let shared = SharedError::from(e);
            for req in requests {
                let _ = req.response_tx.send(Err(shared.clone()));
            }
            return vec![];
        }
    };

    // Per-stack conflict detection and rebase.
    let mut pending: Vec<PendingRebase> = vec![];
    let mut running_head = old_bookmark_value;
    let mut all_rebased_changesets: RebasedChangesets = Default::default();
    let mut all_rebased_bonsais: Vec<BonsaiChangeset> = Vec::new();

    let mut requests_iter = requests.into_iter();
    while let Some(mut request) = requests_iter.next() {
        let bookmark_val = old_bookmark_value.unwrap_or(request.stack.root);
        // Narrow-range scan: use conflict_check_base as ancestor so retries
        // only scan the delta since the last attempt. On first attempt,
        // conflict_check_base == root, so the full range is scanned.
        let conflict_result = match check_pushrebase_conflicts(
            ctx,
            repo,
            config,
            request.stack.root,
            request.conflict_check_base,
            bookmark_val,
            &request.stack.changesets,
            &request.stack.changed_files,
        )
        .await
        {
            Ok(result) => result,
            Err(e) => {
                let _ = request.response_tx.send(Err(SharedError::from(e)));
                continue;
            }
        };

        // Reconcile carried merge info with delta info from this attempt
        let reconciled_overrides = match conflict_result.merged_file_overrides {
            Some(delta_info) => Some(reconcile_merge_file_info(
                &request.carried_merge_file_info,
                &delta_info,
            )),
            None if !request.carried_merge_file_info.is_empty() => {
                Some(request.carried_merge_file_info.clone())
            }
            None => None,
        };

        let merge_resolved_paths = reconciled_overrides
            .as_ref()
            .map(|overrides| overrides.iter().map(|info| info.path.clone()).collect());

        // The summary for a push that has been retried must reflect that
        // MR previously succeeded — otherwise a clean delta on retry would
        // hide a successful MR run. `carried_merge_file_info` is the
        // signal: non-empty means an earlier attempt resolved conflicts.
        let merge_summary = synthesize_carried_summary(&request.carried_merge_file_info)
            .map(|carried| {
                MergeResolutionSummary::combine(carried, conflict_result.merge_summary.clone())
            })
            .unwrap_or(conflict_result.merge_summary);

        // Store reconciled overrides on the request for carry-forward on re-queue
        request.carried_merge_file_info = reconciled_overrides.clone().unwrap_or_default();

        let pushrebase_distance = match try_join(
            repo.commit_graph()
                .changeset_linear_depth(ctx, bookmark_val),
            repo.commit_graph()
                .changeset_linear_depth(ctx, request.stack.root),
        )
        .await
        {
            Ok((bookmark_depth, root_depth)) => bookmark_depth.saturating_sub(root_depth) as usize,
            Err(e) => {
                let _ = request
                    .response_tx
                    .send(Err(SharedError::from(PushrebaseError::from(e))));
                continue;
            }
        };

        // Capture the running head before this request's rebase so each
        // request sees the correct "old bookmark value" for its position
        // in the batch.
        let request_old_bookmark_value = running_head;

        // Rebase this stack onto the running head using the immutable root.
        let onto = running_head.unwrap_or(request.stack.root);
        let rebase_result = create_rebased_changesets(
            ctx,
            repo,
            config,
            &request.stack.changesets,
            request.stack.root,
            request.stack.head,
            onto,
            &mut commit_hooks,
            reconciled_overrides,
        )
        .await;

        match rebase_result {
            Ok((new_head, rebased, rebased_bonsais)) => {
                all_rebased_changesets.extend(rebased);
                all_rebased_bonsais.extend(rebased_bonsais);
                running_head = Some(new_head);
                pending.push(PendingRebase {
                    request,
                    new_head,
                    pushrebase_distance,
                    old_bookmark_value: request_old_bookmark_value,
                    merge_resolved_paths,
                    merge_summary,
                });
            }
            Err(e) => {
                // Fail only the broken request.
                let _ = request.response_tx.send(Err(SharedError::from(e)));
                // `create_rebased_changesets` may have partially mutated
                // `commit_hooks` (e.g. globalrev assignments) before failing.
                // The hooks are now in an inconsistent state, so we cannot
                // continue processing the batch.  Requeue already-pending
                // requests and remaining unprocessed requests so they get
                // fresh hooks on their next pass through the batcher.
                return pending
                    .into_iter()
                    .map(|p| p.request)
                    .chain(requests_iter)
                    .map(|mut req| {
                        req.conflict_check_base =
                            old_bookmark_value.unwrap_or(req.conflict_check_base);
                        req.retry_num = PushrebaseRetryNum(req.retry_num.0 + 1);
                        req
                    })
                    .collect();
            }
        }
    }

    // Save all rebased changesets from all stacks in one batch.
    if let Err(e) = changesets_creation::save_changesets(ctx, repo, all_rebased_bonsais).await {
        let shared = SharedError::from(PushrebaseError::from(e));
        for p in pending {
            let _ = p.request.response_tx.send(Err(shared.clone()));
        }
        return vec![];
    }

    // If no stacks survived conflict detection + rebase, we're done.
    let final_head = match running_head {
        Some(head) if !pending.is_empty() => head,
        _ => return vec![],
    };

    // Convert commit hooks to transaction hooks.
    let txn_hooks = match try_join_all(
        commit_hooks
            .into_iter()
            .map(|h| h.into_transaction_hook(ctx, &all_rebased_changesets)),
    )
    .await
    {
        Ok(h) => h,
        Err(e) => {
            let shared = SharedError::from(PushrebaseError::from(e));
            for p in pending {
                let _ = p.request.response_tx.send(Err(shared.clone()));
            }
            return vec![];
        }
    };

    // Single bookmark CAS update.
    let move_result = try_move_bookmark(
        ctx.clone(),
        repo,
        onto_bookmark,
        old_bookmark_value,
        final_head,
        all_rebased_changesets,
        txn_hooks,
    )
    .await;

    let critical_section_duration_us: i64 = start_critical_section
        .elapsed()
        .as_nanos()
        .try_into()
        .unwrap_or(i64::MAX);

    match move_result {
        Ok(Some((_head, log_id, all_rebased_pairs))) => {
            // CAS succeeded — build per-stack outcomes and send via oneshot.
            if emit_all_bookmarks {
                bookmarks::saturation::record_pushrebase_success(
                    repo.repo_identity().name(),
                    critical_section_duration_us,
                    None,
                    all_rebased_pairs.len() as i64,
                );
            }
            if should_log {
                STATS::critical_section_success_duration_us
                    .add_value(critical_section_duration_us, repo_args.clone());
                STATS::commits_rebased.add_value(all_rebased_pairs.len() as i64, repo_args);
            }

            for p in pending {
                // Per-request: the batch success sample above is per-batch, so
                // record each landed request's retries-until-success separately.
                if emit_all_bookmarks {
                    bookmarks::saturation::record_pushrebase_retries(
                        repo.repo_identity().name(),
                        p.request.retry_num.0 as i64,
                    );
                }
                let stack_pairs: Vec<PushrebaseChangesetPair> = all_rebased_pairs
                    .iter()
                    .filter(|pair| {
                        p.request
                            .stack
                            .changesets
                            .iter()
                            .any(|cs| cs.get_changeset_id() == pair.id_old)
                    })
                    .cloned()
                    .collect();

                let mut sample = ctx.scuba().clone();
                sample
                    .add("repo_name", repo.repo_identity().name())
                    .add("retry_num", p.request.retry_num.0 as i64);
                // Clone for Scuba so the original can be moved into the
                // returned PushrebaseOutcome below; see rebase_with_lock
                // for the rationale.
                p.merge_summary.clone().add_to_scuba(&mut sample);
                sample.log_with_msg("batched_pushrebase_request_complete", None);

                let _ = p.request.response_tx.send(Ok(PushrebaseOutcome {
                    old_bookmark_value: Some(p.old_bookmark_value.unwrap_or(p.request.stack.root)),
                    head: p.new_head,
                    retry_num: p.request.retry_num,
                    rebased_changesets: stack_pairs,
                    pushrebase_distance: PushrebaseDistance(p.pushrebase_distance),
                    log_id,
                    merge_resolved_paths: p.merge_resolved_paths,
                    merge_summary: Some(p.merge_summary),
                }));
            }
            vec![]
        }
        Ok(None) => {
            // CAS failed — update conflict_check_base and return for re-queue.
            if emit_all_bookmarks {
                bookmarks::saturation::record_pushrebase_failure(
                    repo.repo_identity().name(),
                    critical_section_duration_us,
                );
            }
            if should_log {
                STATS::critical_section_failure_duration_us
                    .add_value(critical_section_duration_us, repo_args);
            }
            pending
                .into_iter()
                .map(|mut p| {
                    p.request.conflict_check_base =
                        old_bookmark_value.unwrap_or(p.request.conflict_check_base);
                    p.request.retry_num = PushrebaseRetryNum(p.request.retry_num.0 + 1);
                    // carried_merge_file_info is already updated on the request
                    p.request
                })
                .collect()
        }
        Err(e) => {
            let shared = SharedError::from(e);
            for p in pending {
                let _ = p.request.response_tx.send(Err(shared.clone()));
            }
            vec![]
        }
    }
}

async fn check_filenodes_backfilled(
    ctx: &CoreContext,
    repo: &impl RepoDerivedDataRef,
    head: &ChangesetId,
    limit: u64,
) -> Result<(), Error> {
    let derives_filenodes = repo
        .repo_derived_data()
        .active_config()
        .types
        .contains(&DerivableType::FileNodes);

    if !derives_filenodes {
        // Repo doesn't have filenodes derivation enabled, so no need to check
        // if they're backfilled
        return Ok(());
    }

    let underived = repo
        .repo_derived_data()
        .count_underived::<FilenodesOnlyPublic>(ctx, *head)
        .await?;
    if underived >= limit {
        Err(format_err!(
            "Too many commits do not have filenodes derived. This usually happens when \
            merging a new repo or pushing an extremely long stack.
            Contact source control @ fb if you encounter this issue."
        ))
    } else {
        Ok(())
    }
}

/// Info about a single file that was successfully merged during conflict
/// resolution. Carries the base/server content IDs so the cascading merge
/// in `create_rebased_changesets` can reuse them without re-fetching fsnodes.
///
/// Public for use in `PushrebaseRequest::carried_merge_file_info`.
/// Fields are private — external callers should only initialize with
/// `vec![]`; the pushrebase internals populate this on carry-forward.
#[derive(Clone, Debug, PartialEq)]
pub struct MergedFileInfo {
    path: NonRootMPath,
    base_content_id: ContentId,
    server_content_id: ContentId,
    file_type: FileType,
}

struct ConflictCheckResult {
    /// Number of server-side changesets (used for pushrebase_distance tracking).
    server_changeset_count: usize,
    /// If merge resolution succeeded, info about each merged file.
    /// `None` means no conflicts or merge resolution was not attempted.
    merged_file_overrides: Option<Vec<MergedFileInfo>>,
    /// Merge-resolution outcome for this conflict-check attempt. Always set
    /// on the Ok path: `NotNeeded` when there were no conflicts, `Succeeded`
    /// when MR resolved them. Failure-path summaries are not propagated here
    /// (the standalone "Pushrebase merge resolution failed" Scuba sample
    /// captures them); they ride with PushrebaseError in a follow-up.
    merge_summary: MergeResolutionSummary,
}

/// Checks for server-side conflicts against the client's pushed stack.
/// Returns conflict check results including the number of server-side changesets
/// and optionally merged file overrides if live merge resolution succeeds.
async fn check_pushrebase_conflicts(
    ctx: &CoreContext,
    repo: &impl Repo,
    config: &PushrebaseFlags,
    root: ChangesetId,
    ancestor: ChangesetId,
    descendant: ChangesetId,
    client_bcs: &[BonsaiChangeset],
    client_cf: &[MPath],
) -> Result<ConflictCheckResult, PushrebaseError> {
    check_pushrebase_conflicts_with(
        ctx,
        repo,
        config,
        root,
        ancestor,
        descendant,
        client_bcs,
        client_cf,
        RangeDiffManifests::FromKnob,
    )
    .await
}

async fn check_pushrebase_conflicts_with(
    ctx: &CoreContext,
    repo: &impl Repo,
    config: &PushrebaseFlags,
    root: ChangesetId,
    ancestor: ChangesetId,
    descendant: ChangesetId,
    client_bcs: &[BonsaiChangeset],
    client_cf: &[MPath],
    manifests: RangeDiffManifests,
) -> Result<ConflictCheckResult, PushrebaseError> {
    let server_bcs =
        fetch_bonsai_range_ancestor_not_included(ctx, repo, ancestor, descendant).await?;
    let server_bcs_len = server_bcs.len();

    if let Some(bcs) = server_bcs.iter().find(|bcs| should_fail_pushrebase(bcs)) {
        return Err(PushrebaseError::ForceFailPushrebase(bcs.get_changeset_id()));
    }

    // Safe with narrow ranges: if attempt 1 passed case-folding for
    // root→S1, no case conflict exists in that range. Retry only needs
    // to check S1→S2 for new case conflicts with client changesets.
    if config.casefolding_check {
        let conflict = check_case_conflicts(
            server_bcs.iter().chain(client_bcs.iter()),
            &config.casefolding_check_excluded_paths,
        );
        if let Some(conflict) = conflict {
            return Err(PushrebaseError::PotentialCaseConflict(conflict.1));
        }
    }

    let server_cf =
        find_changed_files_with(ctx, repo, ancestor, descendant, &server_bcs, manifests).await?;

    match intersect_changed_files(server_cf, client_cf.to_vec()) {
        Ok(()) => Ok(ConflictCheckResult {
            server_changeset_count: server_bcs_len,
            merged_file_overrides: None,
            merge_summary: MergeResolutionSummary::NotNeeded,
        }),
        Err(PushrebaseError::Conflicts(conflicts)) => {
            let reponame = repo.repo_identity().name();
            let conflict_files_count = conflicts.len() as u64;
            STATS::conflict_rejections.add_value(1, (reponame.to_string(),));
            STATS::conflict_files_count.add_value(conflicts.len() as i64, (reponame.to_string(),));

            // Per-request override wins; UseJk defers to the JK.
            let merge_enabled = match config.merge_resolution_override {
                MergeResolutionOverride::ForceOn => true,
                MergeResolutionOverride::ForceOff => false,
                MergeResolutionOverride::UseJk => justknobs::eval(
                    "scm/mononoke:pushrebase_enable_merge_resolution",
                    None,
                    Some(reponame),
                ),
            };
            let max_merge_conflicts: usize = justknobs::get_as::<usize>(
                "scm/mononoke:pushrebase_max_merge_conflicts",
                Some(reponame),
            );
            let max_merge_file_size: u64 = justknobs::get_as::<u64>(
                "scm/mononoke:pushrebase_max_merge_file_size",
                Some(reponame),
            );
            let merge_result = if merge_enabled {
                let derive_fsnodes: bool = justknobs::eval(
                    "scm/mononoke:pushrebase_merge_resolution_derive_fsnodes",
                    None,
                    Some(reponame),
                );
                Some(
                    collect_merge_file_info(
                        ctx,
                        repo,
                        &conflicts,
                        root,
                        &server_bcs,
                        client_bcs,
                        max_merge_conflicts,
                        max_merge_file_size,
                        derive_fsnodes,
                        &config.merge_resolution_excluded_path_prefixes,
                    )
                    .await,
                )
            } else {
                None
            };

            match merge_result {
                Some(Ok(merged_changes)) => {
                    let resolved_paths_sample = merged_changes
                        .iter()
                        .take(MR_PATH_SAMPLE_CAP)
                        .map(|info| info.path.clone())
                        .collect();
                    let merge_summary = MergeResolutionSummary::Succeeded {
                        conflict_files_count,
                        resolved_files_count: merged_changes.len() as u64,
                        resolved_paths_sample,
                    };
                    Ok(ConflictCheckResult {
                        server_changeset_count: server_bcs_len,
                        merged_file_overrides: Some(merged_changes),
                        merge_summary,
                    })
                }
                _ => {
                    // Failure-path summary is intentionally NOT propagated via
                    // PushrebaseError::Conflicts in this diff. The existing
                    // "Pushrebase merge resolution failed" Scuba sample captures
                    // it; a follow-up will fold the failure summary into a
                    // richer error variant once parity is verified.
                    if let Some(Err(ref err)) = merge_result {
                        ctx.scuba()
                            .clone()
                            .add("repo_name", reponame)
                            .add("merge_resolution_outcome", format!("{err}"))
                            .log_with_msg("Pushrebase merge resolution failed", None);
                    }
                    Err(PushrebaseError::Conflicts(conflicts))
                }
            }
        }
        Err(e) => Err(e),
    }
}

/// Pessimistic pushrebase: acquires a per-bookmark SQL lock before rebasing.
/// The lock guarantees exclusivity — no other writer can move this bookmark
/// during the rebase. CAS is retained as defense-in-depth.
///
/// The expensive conflict check runs outside the lock (speculative). Only
/// a small delta check runs inside the lock if the bookmark moved between
/// the speculative read and lock acquisition.
async fn rebase_with_lock(
    ctx: &CoreContext,
    repo: &impl PushrebaseRepo,
    config: &PushrebaseFlags,
    onto_bookmark: &BookmarkKey,
    stack: &PushrebaseStack,
    prepushrebase_hooks: &[Box<dyn PushrebaseHook>],
) -> Result<PushrebaseOutcome, PushrebaseError> {
    let overall_start = Instant::now();

    // Phase 1: Speculative conflict check OUTSIDE the lock.
    let speculative_bv = get_bookmark_value(ctx, repo, onto_bookmark).await?;
    let speculative_bv_cs = speculative_bv
        .ok_or_else(|| PushrebaseError::Error(anyhow!("bookmark {onto_bookmark} not found")))?;

    let speculative_conflicts = check_pushrebase_conflicts(
        ctx,
        repo,
        config,
        stack.root,
        stack.root,
        speculative_bv_cs,
        &stack.changesets,
        &stack.changed_files,
    )
    .await?;

    let speculative = SpeculativeConflictResult {
        bookmark_value: speculative_bv_cs,
        merge_info: speculative_conflicts
            .merged_file_overrides
            .unwrap_or_default(),
        server_changeset_count: speculative_conflicts.server_changeset_count,
        merge_summary: speculative_conflicts.merge_summary,
    };

    // Phase 2: Acquire per-bookmark lock.
    let lock_start = Instant::now();
    let sql_bookmarks = repo.sql_bookmarks();
    let locked_txn = sql_bookmarks
        .start_locked_transaction(ctx, onto_bookmark)
        .await
        .map_err(PushrebaseError::Error)?;
    let lock_wait_ms = lock_start.elapsed().as_millis() as i64;
    let lock_hold_start = Instant::now();
    let auth_value = locked_txn.current_value();

    // Phases 3+4: validate, rebase, save — all under lock.
    // On failure, rollback the lock so it is released promptly.
    let rebase_result = try_rebase_under_lock(
        ctx,
        repo,
        config,
        auth_value,
        speculative,
        stack,
        prepushrebase_hooks,
    )
    .await;

    let rebase = match rebase_result {
        Ok(r) => r,
        Err(e) => {
            locked_txn.rollback().await.ok();
            return Err(e);
        }
    };

    // Phase 5: Commit the bookmark move under the lock.
    let log_id = locked_txn
        .commit(
            ctx,
            rebase.new_head,
            BookmarkUpdateReason::Pushrebase,
            vec![wrap_pushrebase_hooks(rebase.txn_hooks)],
        )
        .await
        .map_err(PushrebaseError::Error)?;

    let log_id = log_id.ok_or_else(|| {
        PushrebaseError::Error(anyhow!(
            "CAS failed despite holding lock — non-pushrebase writer moved bookmark"
        ))
    })?;

    let total_ms = overall_start.elapsed().as_millis() as i64;
    let lock_hold_ms = lock_hold_start.elapsed().as_millis() as i64;
    let bookmark_moved = auth_value != Some(speculative_bv_cs);

    // Clone the summary so the same value can be both logged to Scuba
    // here and moved into the returned PushrebaseOutcome below. Cloning
    // is cheap (a Vec of paths capped at MR_PATH_SAMPLE_CAP) and removes
    // the silent foot-gun where a future change to `add_to_scuba` taking
    // `self` would break the subsequent move.
    let merge_summary = rebase.merge_summary.clone();
    let mut sample = ctx.scuba().clone();
    sample
        .add("pessimistic_lock_wait_ms", lock_wait_ms)
        .add("pessimistic_lock_hold_ms", lock_hold_ms)
        .add("pessimistic_total_ms", total_ms)
        .add("pessimistic_bookmark_moved", bookmark_moved)
        .add(
            "pessimistic_pushrebase_distance",
            rebase.pushrebase_distance as i64,
        )
        .add(
            "pessimistic_rebased_changesets",
            rebase.rebased_changesets.len() as i64,
        );
    merge_summary.add_to_scuba(&mut sample);
    sample.log_with_msg("pessimistic_pushrebase_complete", None);

    let rebased_pairs = rebased_changesets_into_pairs(rebase.rebased_changesets);

    Ok(PushrebaseOutcome {
        old_bookmark_value: auth_value,
        head: rebase.new_head,
        retry_num: PushrebaseRetryNum(0),
        rebased_changesets: rebased_pairs,
        pushrebase_distance: PushrebaseDistance(rebase.pushrebase_distance),
        log_id: BookmarkUpdateLogId(log_id),
        merge_resolved_paths: rebase.merge_resolved_paths,
        merge_summary: Some(rebase.merge_summary),
    })
}

/// Performs the delta-check, rebase, and save phases under the lock.
/// Does NOT commit or rollback — the caller owns the `LockedBookmarkTransaction`.
async fn try_rebase_under_lock(
    ctx: &CoreContext,
    repo: &impl PushrebaseRepo,
    config: &PushrebaseFlags,
    auth_value: Option<ChangesetId>,
    speculative: SpeculativeConflictResult,
    stack: &PushrebaseStack,
    prepushrebase_hooks: &[Box<dyn PushrebaseHook>],
) -> Result<RebaseUnderLockResult, PushrebaseError> {
    let auth_cs = auth_value.ok_or_else(|| {
        PushrebaseError::Error(anyhow!("bookmark deleted during lock acquisition"))
    })?;

    let mut merge_info = speculative.merge_info;
    let mut merge_summary = speculative.merge_summary;

    // Phase 3: Validate and delta check.
    let pushrebase_distance = if auth_cs != speculative.bookmark_value {
        let is_descendant = repo
            .commit_graph()
            .is_ancestor(ctx, speculative.bookmark_value, auth_cs)
            .await
            .map_err(PushrebaseError::Error)?;

        if !is_descendant {
            return Err(PushrebaseError::Error(anyhow!(
                "bookmark moved to non-descendant during lock acquisition, retry"
            )));
        }

        let delta_conflicts = check_pushrebase_conflicts(
            ctx,
            repo,
            config,
            stack.root,
            speculative.bookmark_value,
            auth_cs,
            &stack.changesets,
            &stack.changed_files,
        )
        .await?;

        let delta_overrides = delta_conflicts.merged_file_overrides.unwrap_or_default();
        merge_info = reconcile_merge_file_info(&merge_info, &delta_overrides);
        merge_summary =
            MergeResolutionSummary::combine(merge_summary, delta_conflicts.merge_summary);

        speculative.server_changeset_count + delta_conflicts.server_changeset_count
    } else {
        speculative.server_changeset_count
    };

    // Phase 4: Rebase + save.
    let mut hooks = try_join_all(prepushrebase_hooks.iter().map(|h| {
        h.in_critical_section(ctx, auth_value)
            .map_err(PushrebaseError::from)
    }))
    .await?;

    let merged_overrides = if merge_info.is_empty() {
        None
    } else {
        Some(merge_info)
    };

    let merge_resolved_paths = merged_overrides
        .as_ref()
        .map(|overrides| overrides.iter().map(|info| info.path.clone()).collect());

    let (new_head, rebased_changesets, rebased_bonsais) = create_rebased_changesets(
        ctx,
        repo,
        config,
        &stack.changesets,
        stack.root,
        stack.head,
        auth_cs,
        &mut hooks,
        merged_overrides,
    )
    .await?;

    changesets_creation::save_changesets(ctx, repo, rebased_bonsais).await?;

    let txn_hooks: Vec<Box<dyn PushrebaseTransactionHook>> = try_join_all(
        hooks
            .into_iter()
            .map(|h| h.into_transaction_hook(ctx, &rebased_changesets)),
    )
    .await?;

    Ok(RebaseUnderLockResult {
        new_head,
        rebased_changesets,
        txn_hooks,
        merge_resolved_paths,
        pushrebase_distance,
        merge_summary,
    })
}

fn fail_pending(pending: Vec<PendingRebase>, error: SharedError<PushrebaseError>) {
    for p in pending {
        let _ = p.request.response_tx.send(Err(error.clone()));
    }
}

/// Per-request result from speculative (pre-lock) conflict checking.
struct SpeculativeRequestCheck {
    request: PushrebaseRequest,
    merge_info: Vec<MergedFileInfo>,
    pushrebase_distance: usize,
    merge_summary: MergeResolutionSummary,
}

/// Batched pessimistic pushrebase: runs speculative conflict checks outside
/// the lock, then acquires a per-bookmark lock for delta checks + rebase.
async fn batched_rebase_with_lock(
    ctx: &CoreContext,
    repo: &impl PushrebaseRepo,
    config: &PushrebaseFlags,
    onto_bookmark: &BookmarkKey,
    requests: Vec<PushrebaseRequest>,
) -> Vec<PushrebaseRequest> {
    if requests.is_empty() {
        return vec![];
    }

    let should_log = config.monitoring_bookmark.as_deref() == Some(onto_bookmark.as_str());
    // Parallel all-bookmarks saturation counters (see do_batched_pushrebase).
    let emit_all_bookmarks = config.monitoring_bookmark.is_some();
    let repo_args = (repo.repo_identity().name().to_string(),);
    let overall_start = Instant::now();
    let batch_size = requests.len();

    // Phase 1: Speculative conflict checks OUTSIDE the lock.
    let speculative_bv = match get_bookmark_value(ctx, repo, onto_bookmark).await {
        Ok(v) => v,
        Err(e) => {
            let shared = SharedError::from(e);
            for req in requests {
                let _ = req.response_tx.send(Err(shared.clone()));
            }
            return vec![];
        }
    };

    let checked_requests =
        speculative_batch_check(ctx, repo, config, speculative_bv, requests).await;

    if checked_requests.is_empty() {
        return vec![];
    }

    // Phase 2: Acquire per-bookmark lock.
    let lock_start = Instant::now();
    let sql_bookmarks = repo.sql_bookmarks();
    let locked_txn = match sql_bookmarks
        .start_locked_transaction(ctx, onto_bookmark)
        .await
    {
        Ok(t) => t,
        Err(e) => {
            let shared = SharedError::from(PushrebaseError::from(e));
            log_pessimistic_batch_failure(ctx, "lock_acquisition", &shared);
            for c in checked_requests {
                let _ = c.request.response_tx.send(Err(shared.clone()));
            }
            return vec![];
        }
    };
    let lock_wait_ms = lock_start.elapsed().as_millis() as i64;
    // Saturation measures only the serialized (under-lock) window; the
    // speculative conflict check runs before the lock, so start timing here.
    let lock_hold_start = Instant::now();
    let auth_value = locked_txn.current_value();

    // Phase 3: Run hooks under lock.
    let requests_slice: Vec<&PushrebaseRequest> =
        checked_requests.iter().map(|c| &c.request).collect();
    let mut commit_hooks = match run_batch_hooks(ctx, &requests_slice, auth_value).await {
        Ok(h) => h,
        Err(e) => {
            if emit_all_bookmarks {
                bookmarks::saturation::record_pushrebase_failure(
                    repo.repo_identity().name(),
                    lock_hold_start.elapsed().as_nanos() as i64,
                );
            }
            let shared = SharedError::from(e);
            log_pessimistic_batch_failure(ctx, "hooks", &shared);
            for c in checked_requests {
                let _ = c.request.response_tx.send(Err(shared.clone()));
            }
            let _ = locked_txn.rollback().await;
            return vec![];
        }
    };

    // Phase 4: Delta conflict checks + rebase under lock.
    let rebase_result = rebase_batch_under_lock(
        ctx,
        repo,
        config,
        speculative_bv,
        auth_value,
        checked_requests,
        &mut commit_hooks,
    )
    .await;

    let state = match rebase_result {
        Ok(state) => state,
        Err((requeued, e)) => {
            if emit_all_bookmarks {
                bookmarks::saturation::record_pushrebase_failure(
                    repo.repo_identity().name(),
                    lock_hold_start.elapsed().as_nanos() as i64,
                );
            }
            log_pessimistic_batch_failure(
                ctx,
                "rebase",
                &SharedError::from(PushrebaseError::Error(anyhow!("{e:#}"))),
            );
            let _ = locked_txn.rollback().await;
            return requeued;
        }
    };

    if state.pending.is_empty() {
        let _ = locked_txn.rollback().await;
        return vec![];
    }

    // Phase 5: Save + commit + dispatch.
    let result = save_and_commit_batch(ctx, repo, locked_txn, state, commit_hooks).await;

    let critical_section_duration_us: i64 = overall_start
        .elapsed()
        .as_nanos()
        .try_into()
        .unwrap_or(i64::MAX);

    match result {
        Ok((log_id, all_rebased_pairs, pending)) => {
            if emit_all_bookmarks {
                bookmarks::saturation::record_pushrebase_success(
                    repo.repo_identity().name(),
                    lock_hold_start.elapsed().as_nanos() as i64,
                    None,
                    all_rebased_pairs.len() as i64,
                );
                // Per-request: the batch sample above is per-batch, so record
                // each landed request's retries-until-success separately.
                for p in &pending {
                    bookmarks::saturation::record_pushrebase_retries(
                        repo.repo_identity().name(),
                        p.request.retry_num.0 as i64,
                    );
                }
            }
            if should_log {
                STATS::critical_section_success_duration_us
                    .add_value(critical_section_duration_us, repo_args.clone());
                STATS::commits_rebased.add_value(all_rebased_pairs.len() as i64, repo_args);
            }

            let total_ms = overall_start.elapsed().as_millis() as i64;
            ctx.scuba()
                .clone()
                .add("pessimistic_lock_wait_ms", lock_wait_ms)
                .add("pessimistic_total_ms", total_ms)
                .add("pessimistic_batch_size", batch_size as i64)
                .add(
                    "pessimistic_rebased_changesets",
                    all_rebased_pairs.len() as i64,
                )
                .log_with_msg("pessimistic_batched_pushrebase_complete", None);

            dispatch_batch_results(ctx, repo, pending, log_id, &all_rebased_pairs);
            vec![]
        }
        Err((pending, e)) => {
            if emit_all_bookmarks {
                bookmarks::saturation::record_pushrebase_failure(
                    repo.repo_identity().name(),
                    lock_hold_start.elapsed().as_nanos() as i64,
                );
            }
            if should_log {
                STATS::critical_section_failure_duration_us
                    .add_value(critical_section_duration_us, repo_args);
            }
            log_pessimistic_batch_failure(ctx, "commit", &e);
            fail_pending(pending, e);
            vec![]
        }
    }
}

/// Runs speculative conflict checks for each request BEFORE the lock is
/// acquired. Requests that hit unresolvable conflicts are failed immediately.
async fn speculative_batch_check(
    ctx: &CoreContext,
    repo: &impl PushrebaseRepo,
    config: &PushrebaseFlags,
    speculative_bv: Option<ChangesetId>,
    requests: Vec<PushrebaseRequest>,
) -> Vec<SpeculativeRequestCheck> {
    let mut checked = Vec::with_capacity(requests.len());

    for request in requests {
        let bookmark_val = match speculative_bv {
            Some(v) => v,
            None => request.stack.root,
        };

        let conflict_result = match check_pushrebase_conflicts(
            ctx,
            repo,
            config,
            request.stack.root,
            request.conflict_check_base,
            bookmark_val,
            &request.stack.changesets,
            &request.stack.changed_files,
        )
        .await
        {
            Ok(result) => result,
            Err(e) => {
                let _ = request.response_tx.send(Err(SharedError::from(e)));
                continue;
            }
        };

        let merge_info = conflict_result.merged_file_overrides.unwrap_or_default();
        let merge_summary = conflict_result.merge_summary;

        let pushrebase_distance = match try_join(
            repo.commit_graph()
                .changeset_linear_depth(ctx, bookmark_val),
            repo.commit_graph()
                .changeset_linear_depth(ctx, request.stack.root),
        )
        .await
        {
            Ok((bookmark_depth, root_depth)) => bookmark_depth.saturating_sub(root_depth) as usize,
            Err(e) => {
                let _ = request
                    .response_tx
                    .send(Err(SharedError::from(PushrebaseError::from(e))));
                continue;
            }
        };

        checked.push(SpeculativeRequestCheck {
            request,
            merge_info,
            pushrebase_distance,
            merge_summary,
        });
    }

    checked
}

async fn run_batch_hooks(
    ctx: &CoreContext,
    requests: &[&PushrebaseRequest],
    old_bookmark_value: Option<ChangesetId>,
) -> Result<Vec<Box<dyn PushrebaseCommitHook>>, PushrebaseError> {
    let first = requests.first().ok_or_else(|| {
        PushrebaseError::Error(anyhow!("run_batch_hooks called with no requests"))
    })?;
    let hooks = try_join_all(first.hooks.iter().map(|h| {
        h.in_critical_section(ctx, old_bookmark_value)
            .map_err(PushrebaseError::from)
    }))
    .await?;
    Ok(hooks)
}

struct BatchRebaseState {
    pending: Vec<PendingRebase>,
    all_rebased_changesets: RebasedChangesets,
    all_rebased_bonsais: Vec<BonsaiChangeset>,
}

/// Rebases each request's stack under the lock. Uses speculative conflict
/// results from outside the lock; only runs a delta check if the bookmark
/// moved between the speculative read and lock acquisition.
async fn rebase_batch_under_lock(
    ctx: &CoreContext,
    repo: &impl PushrebaseRepo,
    config: &PushrebaseFlags,
    speculative_bv: Option<ChangesetId>,
    auth_value: Option<ChangesetId>,
    checked_requests: Vec<SpeculativeRequestCheck>,
    commit_hooks: &mut [Box<dyn PushrebaseCommitHook>],
) -> Result<BatchRebaseState, (Vec<PushrebaseRequest>, PushrebaseError)> {
    let mut pending: Vec<PendingRebase> = Vec::new();
    let mut running_head = auth_value;
    let mut all_rebased_changesets: RebasedChangesets = Default::default();
    let mut all_rebased_bonsais: Vec<BonsaiChangeset> = Vec::new();

    let mut checked_iter = checked_requests.into_iter();
    while let Some(checked) = checked_iter.next() {
        let mut request = checked.request;
        let mut merge_info = checked.merge_info;
        let mut pushrebase_distance = checked.pushrebase_distance;
        let mut merge_summary = checked.merge_summary;

        // Delta conflict check: only needed if the bookmark moved between
        // speculative read and lock acquisition.
        if auth_value != speculative_bv {
            let auth_cs = match auth_value {
                Some(v) => v,
                None => request.stack.root,
            };
            let spec_cs = match speculative_bv {
                Some(v) => v,
                None => request.stack.root,
            };

            if auth_cs != spec_cs {
                let delta_result = match check_pushrebase_conflicts(
                    ctx,
                    repo,
                    config,
                    request.stack.root,
                    spec_cs,
                    auth_cs,
                    &request.stack.changesets,
                    &request.stack.changed_files,
                )
                .await
                {
                    Ok(result) => result,
                    Err(e) => {
                        let _ = request.response_tx.send(Err(SharedError::from(e)));
                        continue;
                    }
                };

                let delta_overrides = delta_result.merged_file_overrides.unwrap_or_default();
                merge_info = reconcile_merge_file_info(&merge_info, &delta_overrides);
                pushrebase_distance += delta_result.server_changeset_count;
                merge_summary =
                    MergeResolutionSummary::combine(merge_summary, delta_result.merge_summary);
            }
        }

        let reconciled_overrides = if merge_info.is_empty() {
            if !request.carried_merge_file_info.is_empty() {
                Some(request.carried_merge_file_info.clone())
            } else {
                None
            }
        } else {
            Some(reconcile_merge_file_info(
                &request.carried_merge_file_info,
                &merge_info,
            ))
        };

        let merge_resolved_paths = reconciled_overrides
            .as_ref()
            .map(|overrides| overrides.iter().map(|info| info.path.clone()).collect());

        // Fold in any carried summary from prior CAS-failure retries
        // (mirrors the legacy non-pessimistic batched loop's semantics).
        if let Some(carried) = synthesize_carried_summary(&request.carried_merge_file_info) {
            merge_summary = MergeResolutionSummary::combine(carried, merge_summary);
        }

        request.carried_merge_file_info = reconciled_overrides.clone().unwrap_or_default();

        let request_old_bookmark_value = running_head;
        let onto = running_head.unwrap_or(request.stack.root);
        let rebase_result = create_rebased_changesets(
            ctx,
            repo,
            config,
            &request.stack.changesets,
            request.stack.root,
            request.stack.head,
            onto,
            commit_hooks,
            reconciled_overrides,
        )
        .await;

        match rebase_result {
            Ok((new_head, rebased, rebased_bonsais)) => {
                all_rebased_changesets.extend(rebased);
                all_rebased_bonsais.extend(rebased_bonsais);
                running_head = Some(new_head);
                pending.push(PendingRebase {
                    request,
                    new_head,
                    pushrebase_distance,
                    old_bookmark_value: request_old_bookmark_value,
                    merge_resolved_paths,
                    merge_summary,
                });
            }
            Err(e) => {
                let shared = SharedError::from(e);
                let _ = request.response_tx.send(Err(shared.clone()));
                let requeued = pending
                    .into_iter()
                    .map(|p| p.request)
                    .chain(checked_iter.map(|c| c.request))
                    .map(|mut req| {
                        req.conflict_check_base = auth_value.unwrap_or(req.conflict_check_base);
                        req.retry_num = PushrebaseRetryNum(req.retry_num.0 + 1);
                        req
                    })
                    .collect();
                return Err((requeued, PushrebaseError::Error(anyhow!("{shared:#}"))));
            }
        }
    }

    Ok(BatchRebaseState {
        pending,
        all_rebased_changesets,
        all_rebased_bonsais,
    })
}

/// Saves rebased changesets, commits the locked transaction, and returns
/// data needed to dispatch per-request results.
async fn save_and_commit_batch(
    ctx: &CoreContext,
    repo: &impl PushrebaseRepo,
    locked_txn: dbbookmarks::LockedBookmarkTransaction,
    state: BatchRebaseState,
    commit_hooks: Vec<Box<dyn PushrebaseCommitHook>>,
) -> Result<
    (u64, Vec<PushrebaseChangesetPair>, Vec<PendingRebase>),
    (Vec<PendingRebase>, SharedError<PushrebaseError>),
> {
    let BatchRebaseState {
        pending,
        all_rebased_changesets,
        all_rebased_bonsais,
    } = state;

    if let Err(e) = changesets_creation::save_changesets(ctx, repo, all_rebased_bonsais).await {
        let shared = SharedError::from(PushrebaseError::from(e));
        let _ = locked_txn.rollback().await;
        return Err((pending, shared));
    }

    let final_head = match pending.last() {
        Some(p) => p.new_head,
        None => {
            let _ = locked_txn.rollback().await;
            return Err((
                pending,
                SharedError::from(PushrebaseError::Error(anyhow!("no pending rebases"))),
            ));
        }
    };

    let txn_hooks = match try_join_all(
        commit_hooks
            .into_iter()
            .map(|h| h.into_transaction_hook(ctx, &all_rebased_changesets)),
    )
    .await
    {
        Ok(h) => h,
        Err(e) => {
            let shared = SharedError::from(PushrebaseError::from(e));
            let _ = locked_txn.rollback().await;
            return Err((pending, shared));
        }
    };

    let commit_result = locked_txn
        .commit(
            ctx,
            final_head,
            BookmarkUpdateReason::Pushrebase,
            vec![wrap_pushrebase_hooks(txn_hooks)],
        )
        .await;

    match commit_result {
        Ok(Some(log_id)) => {
            let all_rebased_pairs = rebased_changesets_into_pairs(all_rebased_changesets);
            Ok((log_id, all_rebased_pairs, pending))
        }
        Ok(None) => {
            let shared = SharedError::from(PushrebaseError::Error(anyhow!(
                "CAS failed despite holding lock — non-pushrebase writer moved bookmark"
            )));
            Err((pending, shared))
        }
        Err(e) => {
            let shared = SharedError::from(PushrebaseError::from(e));
            Err((pending, shared))
        }
    }
}

fn dispatch_batch_results(
    ctx: &CoreContext,
    repo: &impl PushrebaseRepo,
    pending: Vec<PendingRebase>,
    log_id: u64,
    all_rebased_pairs: &[PushrebaseChangesetPair],
) {
    for p in pending {
        let stack_pairs: Vec<PushrebaseChangesetPair> = all_rebased_pairs
            .iter()
            .filter(|pair| {
                p.request
                    .stack
                    .changesets
                    .iter()
                    .any(|cs| cs.get_changeset_id() == pair.id_old)
            })
            .cloned()
            .collect();

        let mut sample = ctx.scuba().clone();
        sample
            .add("repo_name", repo.repo_identity().name())
            .add("retry_num", p.request.retry_num.0 as i64);
        p.merge_summary.add_to_scuba(&mut sample);
        sample.log_with_msg("batched_pushrebase_request_complete", None);

        let _ = p.request.response_tx.send(Ok(PushrebaseOutcome {
            old_bookmark_value: Some(p.old_bookmark_value.unwrap_or(p.request.stack.root)),
            head: p.new_head,
            retry_num: p.request.retry_num,
            rebased_changesets: stack_pairs,
            pushrebase_distance: PushrebaseDistance(p.pushrebase_distance),
            log_id: BookmarkUpdateLogId(log_id),
            merge_resolved_paths: p.merge_resolved_paths,
            merge_summary: Some(p.merge_summary),
        }));
    }
}

fn log_pessimistic_batch_failure(
    ctx: &CoreContext,
    phase: &str,
    error: &SharedError<PushrebaseError>,
) {
    ctx.scuba()
        .clone()
        .add("pessimistic_failure_phase", phase.to_string())
        .add("pessimistic_failure_reason", format!("{error:#}"))
        .log_with_msg("pessimistic_batched_pushrebase_failure", None);
}

async fn rebase_in_loop(
    ctx: &CoreContext,
    repo: &impl Repo,
    config: &PushrebaseFlags,
    onto_bookmark: &BookmarkKey,
    stack: &PushrebaseStack,
    prepushrebase_hooks: &[Box<dyn PushrebaseHook>],
) -> Result<PushrebaseOutcome, PushrebaseError> {
    let should_log = config.monitoring_bookmark.as_deref() == Some(onto_bookmark.as_str());
    // Parallel all-bookmarks saturation counters (see do_batched_pushrebase).
    let emit_all_bookmarks = config.monitoring_bookmark.is_some();
    let mut any_attempt_resolved_conflicts = false;
    let repo_args = (repo.repo_identity().name().to_string(),);
    let mut latest_rebase_attempt = stack.root;
    let mut carried_merge_file_info: Vec<MergedFileInfo> = Vec::new();
    let mut total_pushrebase_distance: usize = 0;
    let mut accumulated_merge_summary = MergeResolutionSummary::NotNeeded;
    for retry_num in 0..MAX_REBASE_ATTEMPTS {
        let retry_num = PushrebaseRetryNum(retry_num);

        let start_critical_section = Instant::now();
        // CRITICAL SECTION START: After getting the value of the bookmark
        let old_bookmark_value = get_bookmark_value(ctx, repo, onto_bookmark).await?;
        let hooks = try_join_all(prepushrebase_hooks.iter().map(|h| {
            h.in_critical_section(ctx, old_bookmark_value)
                .map_err(PushrebaseError::from)
        }))
        .await?;

        // Narrow-range scan: only check changesets since the last attempt.
        // Carried MergedFileInfo from previous attempts provides the rest.
        // Note: if a carried file is deleted or type-changed on the server
        // in the delta range, collect_merge_file_info rejects the conflict
        // and check_pushrebase_conflicts returns Err(Conflicts) before
        // reconciliation is reached, so no special handling is needed.
        let conflict_result = check_pushrebase_conflicts(
            ctx,
            repo,
            config,
            stack.root,
            latest_rebase_attempt,
            old_bookmark_value.unwrap_or(stack.root),
            &stack.changesets,
            &stack.changed_files,
        )
        .await?;
        // Accumulate total pushrebase distance across retries since each
        // narrow-range scan only covers the delta since the last attempt.
        total_pushrebase_distance += conflict_result.server_changeset_count;
        let pushrebase_distance = PushrebaseDistance(total_pushrebase_distance);
        // Accumulate the per-attempt summary so the final outcome reflects
        // any MR success across the retry chain (Succeeded is sticky).
        accumulated_merge_summary = MergeResolutionSummary::combine(
            accumulated_merge_summary,
            conflict_result.merge_summary,
        );

        // Reconcile carried info with delta info from this attempt
        let reconciled_overrides = match conflict_result.merged_file_overrides {
            Some(delta_info) => Some(reconcile_merge_file_info(
                &carried_merge_file_info,
                &delta_info,
            )),
            None if !carried_merge_file_info.is_empty() => {
                // Delta had no conflicts, but we have carried info from
                // previous attempts — use it as-is.
                Some(carried_merge_file_info.clone())
            }
            None => None,
        };

        if reconciled_overrides.is_some() {
            any_attempt_resolved_conflicts = true;
        }

        let merge_resolved_paths = reconciled_overrides
            .as_ref()
            .map(|overrides| overrides.iter().map(|info| info.path.clone()).collect());

        // INVARIANT (defense-in-depth, expected unreachable with carry-forward):
        // If any previous attempt resolved conflicts, carried_merge_file_info
        // is non-empty, so reconciled_overrides is always Some. This check
        // guards against future logic changes that might break that property.
        if any_attempt_resolved_conflicts && merge_resolved_paths.is_none() {
            STATS::merge_resolution_lost_on_retry.add_value(1, repo_args.clone());

            // Log to Scuba for oncall visibility. The ODS counter alone
            // doesn't carry enough context to investigate.
            ctx.scuba().clone()
                .add("log_tag", "MergeResolutionLostOnRetry")
                .add("repo_name", repo.repo_identity().name())
                .add("retry_num", retry_num.0 as i64)
                .add(
                    "merge_resolution_invariant_violation",
                    "any_attempt_resolved_conflicts=true but final attempt has no merge_resolved_paths",
                )
                .log();

            return Err(PushrebaseInternalError::MergeResolutionLostOnRetry.into());
        }

        let rebase_outcome = do_rebase(
            ctx,
            repo,
            config,
            stack,
            old_bookmark_value,
            onto_bookmark,
            hooks,
            reconciled_overrides.clone(),
        )
        .await?;
        // CRITICAL SECTION END: Right after writing new value of bookmark

        let critical_section_duration_us: i64 = start_critical_section
            .elapsed()
            .as_nanos()
            .try_into()
            .unwrap_or(i64::MAX);
        if let Some((head, log_id, rebased_changesets)) = rebase_outcome {
            if emit_all_bookmarks {
                bookmarks::saturation::record_pushrebase_success(
                    repo.repo_identity().name(),
                    critical_section_duration_us,
                    Some(retry_num.0 as i64),
                    rebased_changesets.len() as i64,
                );
            }
            if should_log {
                STATS::critical_section_success_duration_us
                    .add_value(critical_section_duration_us, repo_args.clone());
                STATS::critical_section_retries_failed
                    .add_value(retry_num.0 as i64, repo_args.clone());
                STATS::commits_rebased
                    .add_value(rebased_changesets.len() as i64, repo_args.clone());
            }
            // Per-push Scuba sample so `mr_outcome` is queryable on every
            // pushrebase outcome, not only the pessimistic/batched paths.
            // See `rebase_with_lock` for the cloning rationale.
            let merge_summary_for_scuba = accumulated_merge_summary.clone();
            let mut sample = ctx.scuba().clone();
            sample
                .add("repo_name", repo.repo_identity().name())
                .add("retry_num", retry_num.0 as i64)
                .add("rebased_changesets", rebased_changesets.len() as i64);
            merge_summary_for_scuba.add_to_scuba(&mut sample);
            sample.log_with_msg("pushrebase_complete", None);

            let res = PushrebaseOutcome {
                old_bookmark_value: Some(old_bookmark_value.unwrap_or(stack.root)),
                head,
                retry_num,
                rebased_changesets,
                pushrebase_distance,
                log_id,
                merge_resolved_paths,
                merge_summary: Some(accumulated_merge_summary),
            };
            return Ok(res);
        } else {
            // CAS failed — carry forward merge info for next attempt
            carried_merge_file_info = reconciled_overrides.unwrap_or_default();
            latest_rebase_attempt = old_bookmark_value.unwrap_or(stack.root);
            if emit_all_bookmarks {
                bookmarks::saturation::record_pushrebase_failure(
                    repo.repo_identity().name(),
                    critical_section_duration_us,
                );
            }
            if should_log {
                STATS::critical_section_failure_duration_us
                    .add_value(critical_section_duration_us, repo_args.clone());
            }
        }
    }
    if emit_all_bookmarks {
        bookmarks::saturation::record_pushrebase_retries(
            repo.repo_identity().name(),
            MAX_REBASE_ATTEMPTS as i64,
        );
    }
    if should_log {
        STATS::critical_section_retries_failed.add_value(MAX_REBASE_ATTEMPTS as i64, repo_args);
    }

    Err(PushrebaseInternalError::TooManyRebaseAttempts.into())
}

fn should_fail_pushrebase(bcs: &BonsaiChangeset) -> bool {
    bcs.hg_extra().any(|(key, _)| key == FAIL_PUSHREBASE_EXTRA)
}

async fn do_rebase(
    ctx: &CoreContext,
    repo: &impl Repo,
    config: &PushrebaseFlags,
    stack: &PushrebaseStack,
    old_bookmark_value: Option<ChangesetId>,
    onto_bookmark: &BookmarkKey,
    mut hooks: Vec<Box<dyn PushrebaseCommitHook>>,
    merged_file_overrides: Option<Vec<MergedFileInfo>>,
) -> Result<
    Option<(
        ChangesetId,
        BookmarkUpdateLogId,
        Vec<PushrebaseChangesetPair>,
    )>,
    PushrebaseError,
> {
    let (new_head, rebased_changesets, rebased_bonsais) = create_rebased_changesets(
        ctx,
        repo,
        config,
        &stack.changesets,
        stack.root,
        stack.head,
        old_bookmark_value.unwrap_or(stack.root),
        &mut hooks,
        merged_file_overrides,
    )
    .await?;

    changesets_creation::save_changesets(ctx, repo, rebased_bonsais).await?;

    let hooks = try_join_all(
        hooks
            .into_iter()
            .map(|h| h.into_transaction_hook(ctx, &rebased_changesets)),
    )
    .await?;

    try_move_bookmark(
        ctx.clone(),
        repo,
        onto_bookmark,
        old_bookmark_value,
        new_head,
        rebased_changesets,
        hooks,
    )
    .await
}

// There should only be one head in the pushed set
fn find_only_head_or_fail(
    commits: &HashSet<BonsaiChangeset>,
) -> Result<ChangesetId, PushrebaseError> {
    let mut commits_set: HashSet<_> =
        HashSet::from_iter(commits.iter().map(|commit| commit.get_changeset_id()));
    for commit in commits {
        for p in commit.parents() {
            commits_set.remove(&p);
        }
    }
    if commits_set.len() == 1 {
        Ok(commits_set.iter().next().unwrap().clone())
    } else {
        Err(PushrebaseError::Error(
            PushrebaseInternalError::PushrebaseTooManyHeads.into(),
        ))
    }
}

/// Represents index of current child with regards to its parent
#[derive(Clone, Copy, PartialEq, Eq)]
struct ChildIndex(usize);

fn find_roots(commits: &HashSet<BonsaiChangeset>) -> HashMap<ChangesetId, ChildIndex> {
    let commits_set: HashSet<_> =
        HashSet::from_iter(commits.iter().map(|commit| commit.get_changeset_id()));
    let mut roots = HashMap::new();
    for commit in commits {
        for (index, parent) in commit.parents().enumerate() {
            if !commits_set.contains(&parent) {
                let ChildIndex(max_index) = roots.entry(parent.clone()).or_insert(ChildIndex(0));
                *max_index = max(index, *max_index);
            }
        }
    }
    roots
}

async fn find_closest_root(
    ctx: &CoreContext,
    repo: &impl Repo,
    config: &PushrebaseFlags,
    bookmark: &BookmarkKey,
    roots: &HashMap<ChangesetId, ChildIndex>,
) -> Result<ChangesetId, PushrebaseError> {
    let maybe_id = get_bookmark_value(ctx, repo, bookmark).await?;

    if let Some(id) = maybe_id {
        return find_closest_ancestor_root(ctx, repo, config, bookmark, roots, id).await;
    }

    let roots = repo
        .commit_graph()
        .many_changeset_generations(ctx, &roots.keys().copied().collect::<Vec<_>>())
        .await?;

    let (cs_id, _) = roots
        .into_iter()
        .max_by_key(|(_, gen_num)| gen_num.clone())
        .ok_or_else(|| PushrebaseError::from(PushrebaseInternalError::NoRoots))?;

    Ok(cs_id)
}

async fn find_closest_ancestor_root(
    ctx: &CoreContext,
    repo: &impl Repo,
    config: &PushrebaseFlags,
    bookmark: &BookmarkKey,
    roots: &HashMap<ChangesetId, ChildIndex>,
    onto_bookmark_cs_id: ChangesetId,
) -> Result<ChangesetId, PushrebaseError> {
    let id = repo
        .commit_graph()
        .filter_ancestors(ctx, onto_bookmark_cs_id, roots.keys().copied().collect())
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| {
            PushrebaseError::Error(
                PushrebaseInternalError::PushrebaseNoCommonRoot(
                    bookmark.clone(),
                    roots.keys().copied().collect(),
                )
                .into(),
            )
        })?;
    let (bookmark_generation, root_generation) = futures::try_join!(
        repo.commit_graph()
            .changeset_generation(ctx, onto_bookmark_cs_id),
        repo.commit_graph().changeset_generation(ctx, id),
    )?;
    let distance = bookmark_generation
        .value()
        .saturating_sub(root_generation.value());
    if config
        .recursion_limit
        .is_some_and(|limit| distance >= u64::try_from(limit).unwrap_or(u64::MAX))
    {
        return Err(PushrebaseError::RootTooFarBehind);
    }

    let index = roots
        .get(&id)
        .expect("closest root should be one of the candidate roots");
    if config.forbid_p2_root_rebases && *index != ChildIndex(0) {
        ctx.scuba().clone().log_with_msg(
            "pushrebase_p2_root_rejected",
            Some(format!(
                "root={}, bookmark={}, depth={}, child_index={}",
                id, bookmark, distance, index.0,
            )),
        );

        let hgcs = repo.derive_hg_changeset(ctx, id).await?;
        return Err(PushrebaseError::Error(
            PushrebaseInternalError::P2RootRebaseForbidden(hgcs, bookmark.clone()).into(),
        ));
    }

    Ok(id)
}

/// Backing manifests for the range diff of a merge commit whose other
/// parent lies outside the range.
#[derive(Copy, Clone)]
enum RangeDiffManifests {
    ContentCompat,
    /// HG historically; the knob rolls repos onto the content compat.
    /// Resolved lazily so the knob is read only when a merge is in range.
    FromKnob,
}

async fn find_changed_files_between_manifests(
    ctx: &CoreContext,
    repo: &impl Repo,
    ancestor: ChangesetId,
    descendant: ChangesetId,
) -> Result<Vec<MPath>, PushrebaseError> {
    let paths = find_bonsai_diff(ctx, repo, ancestor, descendant)
        .await?
        .map_ok(|diff| MPath::from(diff.into_path()))
        .try_collect()
        .await?;

    Ok(paths)
}

async fn find_changed_files_between_root_manifests(
    ctx: &CoreContext,
    repo: &impl Repo,
    ancestor: ChangesetId,
    descendant: ChangesetId,
) -> Result<Vec<MPath>, PushrebaseError> {
    let (d_mf, a_mf) = try_join(
        id_to_root_manifest_id(ctx, repo, descendant),
        id_to_root_manifest_id(ctx, repo, ancestor),
    )
    .await?;

    let paths = bonsai_diff(
        ctx.clone(),
        repo.repo_blobstore().clone(),
        d_mf,
        Some(a_mf).into_iter().collect(),
    )
    .map_ok(|diff| MPath::from(diff.into_path()))
    .try_collect()
    .await?;

    Ok(paths)
}

pub async fn find_bonsai_diff<T: Repo>(
    ctx: &CoreContext,
    repo: &T,
    ancestor: ChangesetId,
    descendant: ChangesetId,
) -> Result<
    impl TryStream<Ok = BonsaiDiffFileChange<(FileType, HgFileNodeId)>, Error = Error> + use<T>,
> {
    let (d_mf, a_mf) = try_join(
        id_to_manifestid(ctx, repo, descendant),
        id_to_manifestid(ctx, repo, ancestor),
    )
    .await?;

    Ok(bonsai_diff(
        ctx.clone(),
        repo.repo_blobstore().clone(),
        d_mf,
        Some(a_mf).into_iter().collect(),
    ))
}

async fn id_to_manifestid(
    ctx: &CoreContext,
    repo: &impl Repo,
    bcs_id: ChangesetId,
) -> Result<HgManifestId, Error> {
    let hg_cs_id = repo.derive_hg_changeset(ctx, bcs_id).await?;
    let hg_cs = hg_cs_id.load(ctx, repo.repo_blobstore()).await?;
    Ok(hg_cs.manifestid())
}

/// Content-manifest or fsnode root, per the JustKnobs compat gate.
async fn id_to_root_manifest_id(
    ctx: &CoreContext,
    repo: &impl Repo,
    bcs_id: ChangesetId,
) -> Result<compat::ContentManifestId, Error> {
    let repo_name = repo.repo_identity().name();
    let use_content_manifests = justknobs::eval(
        "scm/mononoke:derived_data_use_content_manifests",
        None,
        Some(repo_name),
    );

    if use_content_manifests {
        Ok(repo
            .repo_derived_data()
            .derive::<RootContentManifestId>(ctx, bcs_id, DerivationPriority::HIGH)
            .await?
            .into_content_manifest_id()
            .into())
    } else {
        Ok(repo
            .repo_derived_data()
            .derive::<RootFsnodeId>(ctx, bcs_id, DerivationPriority::HIGH)
            .await?
            .into_fsnode_id()
            .into())
    }
}

// from smaller generation number to larger
async fn fetch_bonsai_range_ancestor_not_included(
    ctx: &CoreContext,
    repo: &impl Repo,
    ancestor: ChangesetId,
    descendant: ChangesetId,
) -> Result<Vec<BonsaiChangeset>, PushrebaseError> {
    Ok(repo
        .commit_graph()
        .range_stream(ctx, ancestor, descendant)
        .await?
        .filter(|cs_id| future::ready(cs_id != &ancestor))
        .map(async |res| Result::<_, Error>::Ok(res.load(ctx, repo.repo_blobstore()).await?))
        .buffered(100)
        .try_collect::<Vec<_>>()
        .await?)
}

#[cfg(test)]
async fn find_changed_files(
    ctx: &CoreContext,
    repo: &impl Repo,
    ancestor: ChangesetId,
    descendant: ChangesetId,
) -> Result<Vec<MPath>, PushrebaseError> {
    let changesets =
        fetch_bonsai_range_ancestor_not_included(ctx, repo, ancestor, descendant).await?;
    find_changed_files_with(
        ctx,
        repo,
        ancestor,
        descendant,
        &changesets,
        RangeDiffManifests::FromKnob,
    )
    .await
}

async fn find_changed_files_with(
    ctx: &CoreContext,
    repo: &impl Repo,
    ancestor: ChangesetId,
    descendant: ChangesetId,
    changesets: &[BonsaiChangeset],
    manifests: RangeDiffManifests,
) -> Result<Vec<MPath>, PushrebaseError> {
    let ids: HashSet<_> = std::iter::once(ancestor)
        .chain(changesets.iter().map(BonsaiChangeset::get_changeset_id))
        .collect();
    let use_content_manifests = match manifests {
        RangeDiffManifests::ContentCompat => true,
        RangeDiffManifests::FromKnob => justknobs::eval(
            "scm/mononoke:pushrebase_range_diff_use_content_manifests",
            None,
            Some(repo.repo_identity().name()),
        ),
    };

    let file_changes =
        try_join_all(changesets.iter().map(|bcs| async {
            let id = bcs.get_changeset_id();
            let parents: Vec<_> = bcs.parents().collect();
            match *parents {
                [] | [_] => Ok(extract_conflict_files_from_bonsai_changeset(bcs)),
                [p0, p1] => match (ids.get(&p0), ids.get(&p1)) {
                    (Some(_), Some(_)) => Ok(extract_conflict_files_from_bonsai_changeset(bcs)),
                    (Some(parent), None) | (None, Some(parent)) => {
                        if use_content_manifests {
                            find_changed_files_between_root_manifests(ctx, repo, id, *parent).await
                        } else {
                            find_changed_files_between_manifests(ctx, repo, id, *parent).await
                        }
                    }
                    (None, None) => panic!(
                        "`range_stream` produced invalid result for: ({descendant}, {ancestor})",
                    ),
                },
                _ => panic!("pushrebase supports only two parents"),
            }
        }))
        .await?;

    let mut changed_files = file_changes
        .into_iter()
        .flatten()
        .chain(find_subtree_changes(changesets)?)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    changed_files.sort_unstable();

    Ok(changed_files)
}

fn extract_conflict_files_from_bonsai_changeset(bcs: &BonsaiChangeset) -> Vec<MPath> {
    bcs.file_changes()
        .flat_map(|(path, file_change)| {
            let mut v = vec![];
            if let Some((copy_from_path, _)) = file_change.copy_from() {
                v.push(MPath::from(copy_from_path.clone()));
            }
            v.push(MPath::from(path.clone()));
            v.into_iter()
        })
        .collect::<Vec<MPath>>()
}

fn find_subtree_changes(changesets: &[BonsaiChangeset]) -> Result<Vec<MPath>, PushrebaseError> {
    let cs_ids = changesets
        .iter()
        .map(|bcs| bcs.get_changeset_id())
        .collect::<HashSet<_>>();

    let mut paths = Vec::new();
    for bcs in changesets {
        for (path, change) in bcs.subtree_changes() {
            paths.push(path.clone());
            if let Some((from_csid, from_path)) = change.change_source() {
                if cs_ids.contains(&from_csid) {
                    // This change is copying from the rebase set, so its
                    // origin will be updated as part of the pushrebase.
                    // This means we must make the source has not changed
                    // since the root.
                    paths.push(from_path.clone());
                }
            }
        }
    }
    Ok(paths)
}

/// `left` and `right` are considerered to be conflict free, if none of the element from `left`
/// is prefix of element from `right`, and vice versa.
fn intersect_changed_files(left: Vec<MPath>, right: Vec<MPath>) -> Result<(), PushrebaseError> {
    let conflicts: Vec<PushrebaseConflict> = find_path_conflicts(left, right)
        .into_iter()
        .map(|(l, r)| PushrebaseConflict::new(l, r))
        .collect();

    if conflicts.is_empty() {
        Ok(())
    } else {
        Err(PushrebaseError::Conflicts(conflicts))
    }
}

/// Whether the root manifest [`fetch_manifest_file`] would read for `cs_id` is
/// already derived, so merge resolution can run without paying for derivation
/// inside the pushrebase critical section.
///
/// This must probe the same manifest type `fetch_manifest_file` derives --
/// probing fsnodes on a repo that has migrated to content manifests would
/// report "not derived" forever and silently disable merge resolution.
async fn root_manifest_is_derived(
    ctx: &CoreContext,
    repo: &impl Repo,
    cs_id: ChangesetId,
) -> Result<bool> {
    let repo_name = repo.repo_identity().name();
    let use_content_manifests = justknobs::eval(
        "scm/mononoke:derived_data_use_content_manifests",
        None,
        Some(repo_name),
    );

    if use_content_manifests {
        Ok(repo
            .repo_derived_data()
            .fetch_derived::<RootContentManifestId>(ctx, cs_id)
            .await?
            .is_some())
    } else {
        Ok(repo
            .repo_derived_data()
            .fetch_derived::<RootFsnodeId>(ctx, cs_id)
            .await?
            .is_some())
    }
}

/// Fetch manifest file entry for a given path from a changeset's manifest.
/// Returns the ContentManifestFile which provides access to content_id and file_type.
/// Uses content_manifest or fsnode depending on the JustKnobs gate.
async fn fetch_manifest_file(
    ctx: &CoreContext,
    repo: &impl Repo,
    cs_id: ChangesetId,
    path: &NonRootMPath,
) -> Result<Option<compat::ContentManifestFile>> {
    use manifest::Entry;

    let root_id = id_to_root_manifest_id(ctx, repo, cs_id).await?;

    let entry = root_id
        .find_entry(
            ctx.clone(),
            repo.repo_blobstore().clone(),
            path.clone().into(),
        )
        .await?;

    match entry {
        Some(Entry::Leaf(file)) => Ok(Some(file.into())),
        _ => Ok(None),
    }
}

/// Outcome of attempting a three-way merge on a single file.
enum FileMergeOutcome {
    /// Successfully merged content.
    Clean(Bytes),
    /// True content conflict.
    Conflict(String),
    /// Internal error during fetch.
    Error(anyhow::Error),
}

/// 3-way merge using three ContentIds directly (no fsnode lookup).
///
/// Used by the cascading merge in the rebase loop, where the base content
/// comes from a tracked map rather than a fsnode manifest.
async fn merge_file_by_content_ids(
    ctx: &CoreContext,
    repo: &impl Repo,
    path: &NonRootMPath,
    base_content_id: ContentId,
    local_content_id: ContentId,
    other_content_id: ContentId,
) -> FileMergeOutcome {
    let (base_bytes, local_bytes, other_bytes) = futures::join!(
        filestore::fetch_concat(repo.repo_blobstore(), ctx, base_content_id),
        filestore::fetch_concat(repo.repo_blobstore(), ctx, local_content_id),
        filestore::fetch_concat(repo.repo_blobstore(), ctx, other_content_id),
    );

    match (base_bytes, local_bytes, other_bytes) {
        (Ok(base), Ok(local), Ok(other)) => match merge_text(&base, &local, &other) {
            MergeResult::Clean(merged) => FileMergeOutcome::Clean(Bytes::from(merged)),
            MergeResult::Conflict(desc) => {
                FileMergeOutcome::Conflict(format!("file {path}: {desc}"))
            }
        },
        (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => FileMergeOutcome::Error(e),
    }
}

/// Error type for merge resolution failures.
#[derive(Debug, Error)]
enum MergeResolutionError {
    /// A file is missing in one or more versions, or has copy info, or type mismatch.
    #[error("skipped: {0}")]
    Skipped(String),
    /// Too many conflicting files to attempt merge.
    #[error("too many conflicting files")]
    TooManyConflicts,
    /// Internal error fetching file content.
    #[error(transparent)]
    InternalError(Error),
}

/// Collect file metadata needed for cascading merge resolution.
///
/// For each conflicting path, validates that the conflict is an exact path
/// match (not a prefix conflict), checks file types, sizes, and copy info,
/// then fetches the base content ID from fsnodes. Returns a list of
/// `MergedFileInfo` structs containing the path, base content ID, server
/// content ID, and file type. The actual 3-way merge is deferred to the
/// per-commit rebase loop in `create_rebased_changesets`.
///
/// The server-side content is obtained directly from the bonsai changesets
/// rather than deriving manifests, to avoid expensive derivation in the
/// critical section of pushrebase.
///
/// Fails if any file cannot be merged (missing, type mismatch, copy info,
/// too large, or prefix conflict).
async fn collect_merge_file_info(
    ctx: &CoreContext,
    repo: &impl Repo,
    conflicts: &[PushrebaseConflict],
    root: ChangesetId,
    server_bcs: &[BonsaiChangeset],
    client_bcs: &[BonsaiChangeset],
    max_conflicts: usize,
    max_file_size: u64,
    derive_fsnodes: bool,
    excluded_path_prefixes: &PrefixTrie,
) -> Result<Vec<MergedFileInfo>, MergeResolutionError> {
    // Only handle exact path matches (not prefix conflicts like dir vs dir/file)
    let exact_conflicts: Vec<_> = conflicts.iter().filter(|c| c.left == c.right).collect();

    // If there are prefix conflicts, we can't merge those
    if exact_conflicts.len() != conflicts.len() {
        return Err(MergeResolutionError::Skipped(
            "prefix conflicts present".to_string(),
        ));
    }

    if exact_conflicts.len() > max_conflicts {
        return Err(MergeResolutionError::TooManyConflicts);
    }

    // If derive_fsnodes is false, check if the root manifest is already
    // derived. If not, skip merge resolution to avoid expensive derivation in
    // the pushrebase critical section.
    if !derive_fsnodes {
        let is_derived = root_manifest_is_derived(ctx, repo, root)
            .await
            .map_err(MergeResolutionError::InternalError)?;
        if !is_derived {
            return Err(MergeResolutionError::Skipped(
                "root manifest not derived for base commit".to_string(),
            ));
        }
    }

    // Build a map of path -> FileChange from the client changesets
    let client_changes: HashMap<&NonRootMPath, &FileChange> = client_bcs
        .iter()
        .flat_map(|bcs| bcs.file_changes_map().iter())
        .collect();

    // Build a map of path -> FileChange from the server changesets
    // (latest-wins semantics since server_bcs is oldest-to-newest)
    let server_changes: HashMap<&NonRootMPath, &FileChange> = server_bcs
        .iter()
        .flat_map(|bcs| bcs.file_changes_map().iter())
        .collect();

    let mut merged_file_changes = Vec::new();

    for conflict in &exact_conflicts {
        let path = &conflict.left;
        let non_root_path = match path.clone().into_optional_non_root_path() {
            Some(nrp) => nrp,
            None => {
                return Err(MergeResolutionError::Skipped(
                    "root path conflict".to_string(),
                ));
            }
        };

        if excluded_path_prefixes.contains_prefix(&non_root_path) {
            return Err(MergeResolutionError::Skipped(format!(
                "file {non_root_path} is under an excluded path prefix",
            )));
        }

        // Get the client file change for this path
        let client_fc = match client_changes.get(&non_root_path) {
            Some(FileChange::Change(tc)) => tc,
            _ => {
                return Err(MergeResolutionError::Skipped(format!(
                    "file {path} not a tracked change in pushed changeset",
                )));
            }
        };

        // Skip files with copy info
        if client_fc.copy_from().is_some() {
            return Err(MergeResolutionError::Skipped(format!(
                "file {path} has copy-from info",
            )));
        }

        // Skip files that are too large
        if client_fc.size() > max_file_size {
            return Err(MergeResolutionError::Skipped(format!(
                "file {} is too large ({} bytes)",
                path,
                client_fc.size(),
            )));
        }

        // Get server (bookmark head) content from the server bonsai changesets
        let server_fc = match server_changes.get(&non_root_path) {
            Some(FileChange::Change(tc)) => tc,
            _ => {
                return Err(MergeResolutionError::Skipped(format!(
                    "file {path} not a tracked change in bookmark head",
                )));
            }
        };

        // Also check server file size
        if server_fc.size() > max_file_size {
            return Err(MergeResolutionError::Skipped(format!(
                "file {} is too large on server ({} bytes)",
                path,
                server_fc.size(),
            )));
        }

        let local_file_type = client_fc.file_type();

        // Validate server file type matches client file type
        if server_fc.file_type() != local_file_type {
            return Err(MergeResolutionError::Skipped(format!(
                "file {} has type mismatch: client={:?}, server={:?}",
                path,
                local_file_type,
                server_fc.file_type(),
            )));
        }

        // Fetch base content from root manifest — capture the content ID
        // so the cascading merge in create_rebased_changesets can reuse it.
        let base_file = match fetch_manifest_file(ctx, repo, root, &non_root_path).await {
            Ok(Some(f)) => f,
            Ok(None) => {
                return Err(MergeResolutionError::Skipped(format!(
                    "file {non_root_path} not found in base",
                )));
            }
            Err(e) => return Err(MergeResolutionError::InternalError(e)),
        };

        if base_file.file_type() != local_file_type {
            return Err(MergeResolutionError::Skipped(format!(
                "file {} has type mismatch: base={:?}, local={:?}",
                non_root_path,
                base_file.file_type(),
                local_file_type,
            )));
        }

        let base_content_id = base_file.content_id();
        let server_content_id = server_fc.content_id().clone();

        // Record metadata for the cascading merge in
        // create_rebased_changesets. The actual 3-way merge is deferred
        // to the rebase loop where it runs per-commit with the correct
        // base/local/other for each commit in the stack.
        merged_file_changes.push(MergedFileInfo {
            path: non_root_path,
            base_content_id,
            server_content_id,
            file_type: local_file_type,
        });
    }

    // Log success
    ctx.scuba()
        .clone()
        .add("repo_name", repo.repo_identity().name())
        .add("merge_resolution_outcome", "success")
        .add("merge_resolution_files", merged_file_changes.len() as i64)
        .log_with_msg("Pushrebase merge resolution succeeded", None);

    info!(
        "Pushrebase merge resolution succeeded for {} files in {}",
        merged_file_changes.len(),
        repo.repo_identity().name(),
    );

    Ok(merged_file_changes)
}

/// Reconciles carried MergedFileInfo from previous CAS retry attempts
/// with new delta info from the latest attempt. Returns the merged set.
///
/// Rules:
/// - Path in both: update server_content_id from delta (file_type is
///   guaranteed identical by collect_merge_file_info validation)
/// - Path only in carried: keep as-is (server unchanged in delta)
/// - Path only in delta: insert fresh entry
///
/// Note: If a file in `carried` is deleted on the server in the delta
/// range, this function is never called for that scenario —
/// `check_pushrebase_conflicts` returns `Err(Conflicts)` before
/// reaching the reconciliation step, since a client modification
/// conflicting with a server deletion is an irreconcilable conflict.
fn reconcile_merge_file_info(
    carried: &[MergedFileInfo],
    delta: &[MergedFileInfo],
) -> Vec<MergedFileInfo> {
    let mut by_path: HashMap<NonRootMPath, MergedFileInfo> = carried
        .iter()
        .map(|info| (info.path.clone(), info.clone()))
        .collect();

    for info in delta {
        match by_path.entry(info.path.clone()) {
            Entry::Occupied(mut e) => {
                e.get_mut().server_content_id = info.server_content_id;
            }
            Entry::Vacant(e) => {
                e.insert(info.clone());
            }
        }
    }

    by_path.into_values().collect()
}

async fn get_bookmark_value(
    ctx: &CoreContext,
    repo: &impl BookmarksRef,
    bookmark_name: &BookmarkKey,
) -> Result<Option<ChangesetId>, PushrebaseError> {
    let maybe_cs_id = repo
        .bookmarks()
        .get(ctx.clone(), bookmark_name, bookmarks::Freshness::MostRecent)
        .await?;

    Ok(maybe_cs_id)
}

async fn create_rebased_changesets(
    ctx: &CoreContext,
    repo: &impl Repo,
    config: &PushrebaseFlags,
    // `root..head` in topological order, so holders of the stack need not
    // fetch it twice.
    rebased_set: &[BonsaiChangeset],
    root: ChangesetId,
    head: ChangesetId,
    onto: ChangesetId,
    hooks: &mut [Box<dyn PushrebaseCommitHook>],
    merged_file_overrides: Option<Vec<MergedFileInfo>>,
) -> Result<(ChangesetId, RebasedChangesets, Vec<BonsaiChangeset>), PushrebaseError> {
    let rebased_set_ids: HashSet<_> = rebased_set.iter().map(|cs| cs.get_changeset_id()).collect();

    let date = if config.rewritedates {
        Some(Timestamp::now())
    } else {
        None
    };

    // rebased_set already sorted in topological order (oldest first), which
    // guarantees that all required nodes will be updated by the time they
    // are needed.
    //
    // Cascading merge: when merge resolution is active, we perform a
    // per-commit 3-way merge instead of applying overrides only to HEAD.
    // This ensures every intermediate commit has correct content.
    //
    // We track two maps for merge paths:
    //   old_parent_content: content in the ORIGINAL parent chain (pre-rebase)
    //   new_parent_content: content in the REBASED parent chain (post-rebase)
    // For each commit that touches a merge path, we merge:
    //   merge(old_parent_content, commit_content, new_parent_content)
    // then update both maps for the next commit in the stack.

    // Initialize cascading merge state from MergedFileInfo. The base and
    // server content IDs were already captured by collect_merge_file_info,
    // so no additional fsnode fetches are needed here.
    let mut merge_paths: HashSet<NonRootMPath> = HashSet::new();
    let mut old_parent_content: HashMap<NonRootMPath, ContentId> = HashMap::new();
    let mut new_parent_content: HashMap<NonRootMPath, ContentId> = HashMap::new();
    let mut merge_file_types: HashMap<NonRootMPath, FileType> = HashMap::new();

    if let Some(ref overrides) = merged_file_overrides {
        for info in overrides {
            merge_paths.insert(info.path.clone());
            old_parent_content.insert(info.path.clone(), info.base_content_id);
            new_parent_content.insert(info.path.clone(), info.server_content_id);
            merge_file_types.insert(info.path.clone(), info.file_type);
        }
    }

    // Create a fake timestamp, it doesn't matter what timestamp root has

    let mut remapping = hashmap! { root => (onto, Timestamp::now()) };
    let mut rebased = Vec::new();
    // Tracks commits whose every file_change resolved to a duplicate of trunk
    // content via merge resolution — these would land as no-op commits.
    let mut noop_commits: Vec<(ChangesetId, Vec<NonRootMPath>)> = Vec::new();
    for bcs_old in rebased_set.iter().cloned() {
        let id_old = bcs_old.get_changeset_id();

        // Compute per-commit merge overrides via cascading merge.
        let mut overrides_for_this: Vec<(NonRootMPath, FileChange)> = Vec::new();
        let mut duplicate_paths: HashSet<NonRootMPath> = HashSet::new();
        for (path, fc) in bcs_old.file_changes_map() {
            if !merge_paths.contains(path) {
                continue;
            }

            let local_content_id = match fc {
                FileChange::Change(tc) => tc.content_id().clone(),
                _ => continue,
            };

            let base_id = match old_parent_content.get(path) {
                Some(id) => *id,
                None => continue,
            };
            let other_id = match new_parent_content.get(path) {
                Some(id) => *id,
                None => continue,
            };

            // If the new parent has the same content as the old parent,
            // there's nothing to merge — just update tracking.
            if base_id == other_id {
                old_parent_content.insert(path.clone(), local_content_id);
                new_parent_content.insert(path.clone(), local_content_id);
                continue;
            }

            // Client wrote identical content to what's already on the server.
            // After rebase, this file_change becomes a no-op (its content
            // matches the new parent's content at this path). Skip the merge
            // entirely and record the path so we can classify the commit.
            if local_content_id == other_id {
                duplicate_paths.insert(path.clone());
                old_parent_content.insert(path.clone(), local_content_id);
                new_parent_content.insert(path.clone(), local_content_id);
                continue;
            }

            let file_type = merge_file_types
                .get(path)
                .copied()
                .unwrap_or(FileType::Regular);

            match merge_file_by_content_ids(ctx, repo, path, base_id, local_content_id, other_id)
                .await
            {
                FileMergeOutcome::Clean(merged_bytes) => {
                    let size = merged_bytes.len() as u64;
                    let meta = filestore::store(
                        repo.repo_blobstore(),
                        *repo.filestore_config(),
                        ctx,
                        &filestore::StoreRequest::new(size),
                        stream::once(future::ok(merged_bytes)),
                    )
                    .await?;

                    overrides_for_this.push((
                        path.clone(),
                        FileChange::tracked(
                            meta.content_id,
                            file_type,
                            meta.total_size,
                            None,
                            GitLfs::FullContent,
                        ),
                    ));

                    // Update tracking for downstream commits.
                    old_parent_content.insert(path.clone(), local_content_id);
                    new_parent_content.insert(path.clone(), meta.content_id);
                }
                FileMergeOutcome::Conflict(description) => {
                    // Cascading merge failed — fall back to the standard
                    // conflict rejection. This surfaces as a normal
                    // pushrebase conflict error to the client.
                    warn!("Cascading merge conflict on {}: {}", path, description,);
                    return Err(PushrebaseError::Conflicts(vec![PushrebaseConflict {
                        left: MPath::from(path.clone()),
                        right: MPath::from(path.clone()),
                    }]));
                }
                FileMergeOutcome::Error(err) => {
                    warn!("Cascading merge error on {}: {:#}", path, err);
                    return Err(PushrebaseError::Conflicts(vec![PushrebaseConflict {
                        left: MPath::from(path.clone()),
                        right: MPath::from(path.clone()),
                    }]));
                }
            }
        }

        // Classify the commit: if every file_change it touches was a duplicate
        // of trunk content, the rebased commit will land as a no-op. Track for
        // post-loop logging + optional rejection.
        let real_change_count = bcs_old
            .file_changes_map()
            .keys()
            .filter(|p| !duplicate_paths.contains(*p))
            .count();
        if real_change_count == 0 && !duplicate_paths.is_empty() {
            noop_commits.push((id_old, duplicate_paths.iter().cloned().collect()));
        }

        let overrides_ref = if overrides_for_this.is_empty() {
            None
        } else {
            Some(&overrides_for_this)
        };

        let bcs_new = rebase_changeset(
            ctx,
            bcs_old,
            &remapping,
            date.as_ref(),
            &root,
            &onto,
            repo,
            &rebased_set_ids,
            hooks,
            overrides_ref,
        )
        .await?;
        let timestamp = Timestamp::from(*bcs_new.author_date());
        remapping.insert(id_old, (bcs_new.get_changeset_id(), timestamp));
        rebased.push(bcs_new);
    }

    // Post-loop: if any commits became no-ops due to merge resolution, log
    // them to Scuba/ODS and (when JK enabled) reject the entire stack with
    // a Conflicts error matching the pre-merge-resolution behavior.
    if !noop_commits.is_empty() {
        let repo_name = repo.repo_identity().name();
        let repo_args = (repo_name.to_string(),);

        let reject_noop = justknobs::eval(
            "scm/mononoke:pushrebase_reject_noop_merge_commits",
            None,
            Some(repo_name),
        );
        let enforcement = if reject_noop { "rejected" } else { "logged" };

        for (cs_id, paths) in &noop_commits {
            STATS::noop_merge_commits_detected.add_value(1, repo_args.clone());

            let path_strs: Vec<String> = paths.iter().take(10).map(|p| p.to_string()).collect();
            ctx.scuba()
                .clone()
                .add("repo_name", repo_name)
                .add("noop_changeset_id", cs_id.to_string())
                .add("noop_duplicate_paths", path_strs.join(", "))
                .add("noop_duplicate_path_count", paths.len() as i64)
                .add("noop_enforcement_action", enforcement)
                .log_with_msg("Pushrebase no-op merge commit detected", None);
        }

        if reject_noop {
            STATS::noop_merge_commits_rejected.add_value(noop_commits.len() as i64, repo_args);
            let conflicts: Vec<PushrebaseConflict> = noop_commits
                .into_iter()
                .flat_map(|(_, paths)| paths)
                .map(|p| PushrebaseConflict {
                    left: MPath::from(p.clone()),
                    right: MPath::from(p),
                })
                .collect();
            return Err(PushrebaseError::Conflicts(conflicts));
        }
    }

    Ok((
        remapping
            .get(&head)
            .map(|(cs, _)| cs)
            .cloned()
            .unwrap_or(head),
        // `root` wasn't rebased, so let's remove it
        remapping
            .into_iter()
            .filter(|(id_old, _)| *id_old != root)
            .collect(),
        rebased,
    ))
}

async fn rebase_changeset(
    ctx: &CoreContext,
    bcs: BonsaiChangeset,
    remapping: &HashMap<ChangesetId, (ChangesetId, Timestamp)>,
    timestamp: Option<&Timestamp>,
    root: &ChangesetId,
    onto: &ChangesetId,
    repo: &impl Repo,
    rebased_set: &HashSet<ChangesetId>,
    hooks: &mut [Box<dyn PushrebaseCommitHook>],
    merged_file_overrides: Option<&Vec<(NonRootMPath, FileChange)>>,
) -> Result<BonsaiChangeset> {
    let orig_cs_id = bcs.get_changeset_id();
    let new_file_changes =
        generate_additional_bonsai_file_changes(ctx, &bcs, root, onto, repo, rebased_set).await?;
    let mut bcs = bcs.into_mut();

    bcs.parents = bcs
        .parents
        .into_iter()
        .map(|p| remapping.get(&p).map(|(cs, _)| cs).cloned().unwrap_or(p))
        .collect();

    match timestamp {
        Some(timestamp) => {
            let author_tz = bcs.author_date.tz_offset_secs();
            bcs.author_date = DateTime::from_timestamp(timestamp.timestamp_seconds(), author_tz)?;
            if let Some(committer_date) = &mut bcs.committer_date {
                let committer_tz = committer_date.tz_offset_secs();
                *committer_date =
                    DateTime::from_timestamp(timestamp.timestamp_seconds(), committer_tz)?;
            }
        }
        None => {}
    }

    // Mutation information from the original commit must be stripped.
    for key in MUTATION_KEYS {
        bcs.hg_extra.remove(*key);
    }

    // Copy information in bonsai changeset contains a commit parent. So parent changes, then
    // copy information for all copied/moved files needs to be updated
    let mut file_changes = bcs.file_changes;
    for file_change in file_changes.values_mut() {
        match file_change {
            FileChange::Change(tc) => {
                *file_change = FileChange::tracked(
                    tc.content_id().clone(),
                    tc.file_type(),
                    tc.size(),
                    tc.copy_from().map(|(path, cs)| {
                        (
                            path.clone(),
                            remapping.get(cs).map(|(cs, _)| cs).cloned().unwrap_or(*cs),
                        )
                    }),
                    GitLfs::FullContent,
                );
            }
            FileChange::Deletion
            | FileChange::UntrackedDeletion
            | FileChange::UntrackedChange(_) => {}
        }
    }

    // Subtree changes might be sourced from the rebase set, in which case they must be updated.
    for (_path, change) in bcs.subtree_changes.iter_mut() {
        if let Some((from_csid, _from_path)) = change.change_source() {
            if rebased_set.contains(&from_csid) {
                if let Some((new_from_csid, _)) = remapping.get(&from_csid) {
                    change.replace_source_changeset_id(*new_from_csid);
                }
            }
        }
    }

    // Apply merged file overrides from merge resolution.
    // These replace the original file changes for conflicting paths with
    // the merged content that incorporates both local and server-side edits.
    if let Some(overrides) = merged_file_overrides {
        for (path, fc) in overrides {
            file_changes.insert(path.clone(), fc.clone());
        }
    }

    let new_file_paths: HashSet<_> =
        HashSet::from_iter(new_file_changes.iter().map(|(path, _)| path));
    for path in file_changes.keys() {
        if new_file_paths.contains(path) {
            return Err(PushrebaseInternalError::NewFileChangesConflict(orig_cs_id).into());
        }
    }

    file_changes.extend(new_file_changes);
    bcs.file_changes = file_changes;

    for hook in hooks.iter_mut() {
        hook.post_rebase_changeset(orig_cs_id, &mut bcs)?;
    }

    bcs.freeze()
}

// Merge bonsai commits are treated specially in Mononoke. If parents of the merge commit
// have the same file but with a different content, then there's a conflict and to resolve it
// this file should be present in merge bonsai commit. So if we are pushrebasing a merge
// commit we need to take special care.
// See example below
//
// o <- onto
// |
// A   C <-  commit to pushrebase
// | / |
// o   D
// | /
// B
//
// If commit 'A' changes any of the files that existed in commit B (say, file.txt), then
// after commit 'C' is pushrebased on top of master then bonsai logic will try to merge
// file.txt from commit D and from "onto". If bonsai commit that corresponds
// to a rebased commit C doesn't have a file.txt entry, then we'll have invalid bonsai
// changeset (i.e. changeset for which no derived data can be derived, including hg changesets).
//
// generate_additional_bonsai_file_changes works around this problem. It returns a Vec containing
// a file change for all files that were changed between root and onto and that are different between onto
// and parent of bcs that's outside of rebase set (in the example above one of the file changes will be the file
// change for "file.txt").
//
// o <- onto
// |
// A  <- modifies file.txt
// |
// |   C <- Commit C is a merge commit we are pushrebasing
// | / |
// o   D <- commit D has file.txt (because it exists in commit B), so we need to add additional change file.txt
// | /
// B <- this commit has file.text
//
// The file change sets the file to the file as it exists in onto, thus resolving the
// conflict. Since these files were changed after bcs lineage forked off of the root, that means
// that bcs has a "stale" version of them, and that's why we use onto's version instead.
//
// Note that there's another correct solution - we could just add union of changed files for
// (root::onto) and changed files for (root::bcs), however that would add a lot of unnecessary
// file change entries to the pushrebased bonsai merge commit. That would be especially wasteful
// for the case we care about the most - merging a new repo - because we'd list all newly added files.
//
// Note that we don't need to do that if both parents of the merge commit are in the rebased
// set (see example below)
//
// o <- onto
// |
// A      C
// |    / |
// o   X  D
// |  / /
// | Z
// |/
// B
async fn generate_additional_bonsai_file_changes(
    ctx: &CoreContext,
    bcs: &BonsaiChangeset,
    root: &ChangesetId,
    onto: &ChangesetId,
    repo: &impl Repo,
    rebased_set: &HashSet<ChangesetId>,
) -> Result<Vec<(NonRootMPath, FileChange)>> {
    let parents: Vec<_> = bcs.parents().collect();

    if parents.len() <= 1 {
        return Ok(vec![]);
    }

    // We use non_root_parent_outside_of_rebase_set below to figure out what
    // stale entries we DO NOT need to add to the bonsai changeset.
    // o <- onto
    // |
    // A
    // |
    // |   C <- this is the commit being rebased (bcs_id)
    // | / |
    // o   D <- this is non_root_parent_outside_of_rebase_set
    // | /
    // B
    let non_root_parents_outside_of_rebase_set = parents
        .iter()
        .filter(|p| !rebased_set.contains(p) && p != &root)
        .collect::<Vec<_>>();

    if non_root_parents_outside_of_rebase_set.is_empty() {
        return Ok(vec![]);
    }

    let bonsai_diff = find_bonsai_diff(ctx, repo, *root, *onto)
        .await?
        .try_collect::<Vec<_>>()
        .await?;

    let mut paths = vec![];
    for res in &bonsai_diff {
        paths.push(res.path().clone())
    }

    // If a file is not present in the parent, then no need to add it to the new_file_changes.
    // This is done in order to not add unnecessary file changes if they are guaranteed to
    // not have conflicts.
    // Consider the following case:
    //
    // o <- onto
    // |
    // A  <- adds file.txt
    // |
    // |   C <- commit C doesn't have file.txt either
    // | / |
    // o   D <- commit D doesn't have file.txt, so no conflicts possible after pushrebase
    // | /
    // B
    let mut futs = vec![];
    for p in non_root_parents_outside_of_rebase_set {
        let paths = paths.clone();
        futs.push(async move {
            let mfid = id_to_manifestid(ctx, repo, *p).await?;
            let stale = mfid
                .find_entries(ctx.clone(), repo.repo_blobstore().clone(), paths)
                .try_filter_map(async |(path, _)| Ok(Option::<NonRootMPath>::from(path)))
                .try_collect::<HashSet<_>>()
                .await?;
            Result::<_, Error>::Ok(stale)
        });
    }

    let stale_entries = future::try_join_all(futs)
        .await?
        .into_iter()
        .flatten()
        .collect::<HashSet<_>>();

    let mut new_file_changes = vec![];
    for res in bonsai_diff {
        if !stale_entries.contains(res.path()) {
            continue;
        }

        new_file_changes.push(convert_diff_result_into_file_change_for_diamond_merge(
            ctx, repo, res,
        ));
    }

    new_file_changes
        .into_iter()
        .collect::<stream::FuturesUnordered<_>>()
        .try_collect()
        .await
}

/// Wrap a list of pushrebase transaction hooks into a single
/// `BookmarkTransactionHook` closure that runs them sequentially.
///
/// Used by both the optimistic path (`try_move_bookmark`) and
/// pessimistic path (`rebase_with_lock`, `batched_rebase_with_lock`).
fn wrap_pushrebase_hooks(
    hooks: Vec<Box<dyn PushrebaseTransactionHook>>,
) -> BookmarkTransactionHook {
    let hooks = Arc::new(hooks);
    Arc::new(move |ctx, mut sql_txn| {
        let hooks = hooks.clone();
        async move {
            for hook in hooks.iter() {
                sql_txn = hook.populate_transaction(&ctx, sql_txn).await?
            }
            Ok(sql_txn)
        }
        .boxed()
    })
}

async fn try_move_bookmark(
    ctx: CoreContext,
    repo: &impl Repo,
    bookmark: &BookmarkKey,
    old_value: Option<ChangesetId>,
    new_value: ChangesetId,
    rebased_changesets: RebasedChangesets,
    hooks: Vec<Box<dyn PushrebaseTransactionHook>>,
) -> Result<
    Option<(
        ChangesetId,
        BookmarkUpdateLogId,
        Vec<PushrebaseChangesetPair>,
    )>,
    PushrebaseError,
> {
    let mut txn = repo.bookmarks().create_transaction(ctx);

    match old_value {
        Some(old_value) => {
            txn.update(
                bookmark,
                new_value,
                old_value,
                BookmarkUpdateReason::Pushrebase,
            )?;
        }
        None => {
            txn.create(bookmark, new_value, BookmarkUpdateReason::Pushrebase)?;
        }
    }

    let maybe_log_id = txn
        .commit_with_hooks(vec![wrap_pushrebase_hooks(hooks)])
        .await?
        .map(BookmarkUpdateLogId::from);

    Ok(maybe_log_id.map(|log_id| {
        (
            new_value,
            log_id,
            rebased_changesets_into_pairs(rebased_changesets),
        )
    }))
}

#[cfg(test)]
mod tests;
