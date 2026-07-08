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
use std::collections::VecDeque;
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
    #[error("Internal error: root changeset {0} not found")]
    RootNotFound(ChangesetId),
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

/// Result of indexing a pushrebase request
pub struct PushrebaseRequestIndex {
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
    /// Changed files in the pushed stack.
    pub changed_files: Vec<MPath>,
    /// Bonsai changesets to rebase, topological order (ancestor first).
    pub changesets: Vec<BonsaiChangeset>,
    /// Head of the pushed stack.
    pub head: ChangesetId,
    /// Root of the pushed stack. Immutable; used for rebasing.
    pub root: ChangesetId,
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
    // Tag every Scuba sample emitted during this pushrebase with the QE
    // arm derived from the per-request merge-resolution override, so the
    // MR QE readout can bucket completion, dry-run, and merge-failure
    // samples by arm without a cross-table join. `bypass` covers
    // out-of-experiment traffic and the JK default. Rebinding `ctx` here
    // means every downstream `ctx.scuba()` clone inherits the field.
    let ctx = ctx.with_mutated_scuba(|mut scuba| {
        scuba.add("mr_qe_arm", config.merge_resolution_override.qe_arm_str());
        // Per-land key to roll a land's attempts up to a terminal outcome.
        if let Some(land_instance_id) = config.land_instance_id.as_deref() {
            scuba.add("land_instance_id", land_instance_id);
        }
        // QE bucketing key for per-diff dedup in the readout.
        if let Some(phab_diff_id) = config.phab_diff_id.as_deref() {
            scuba.add("phab_diff_id", phab_diff_id);
        }
        scuba
    });
    let ctx = &ctx;

    let PushrebaseRequestIndex {
        changed_files: client_cf,
        changesets: client_bcs,
        head,
        root,
    } = index_pushrebase_request(ctx, repo, config, onto_bookmark, pushed).await?;

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
            head,
            root,
            client_cf,
            &client_bcs,
            prepushrebase_hooks,
        )
        .await;
    }

    rebase_in_loop(
        ctx,
        repo,
        config,
        onto_bookmark,
        head,
        root,
        client_cf,
        &client_bcs,
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
) -> Result<PushrebaseRequestIndex, PushrebaseError> {
    let head = find_only_head_or_fail(pushed)?;
    let roots = find_roots(pushed);
    let root = find_closest_root(ctx, repo, config, onto_bookmark, &roots).await?;

    let (mut client_cf, client_bcs) = try_join(
        find_changed_files(ctx, repo, root, head),
        fetch_bonsai_range_ancestor_not_included(ctx, repo, root, head),
    )
    .await?;

    client_cf.extend(find_subtree_changes(&client_bcs)?);

    check_filenodes_backfilled(ctx, repo, &head, config.not_generated_filenodes_limit).await?;

    Ok(PushrebaseRequestIndex {
        changed_files: client_cf,
        changesets: client_bcs,
        head,
        root,
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
        let bookmark_val = old_bookmark_value.unwrap_or(request.root);
        // Narrow-range scan: use conflict_check_base as ancestor so retries
        // only scan the delta since the last attempt. On first attempt,
        // conflict_check_base == root, so the full range is scanned.
        let conflict_result = match check_pushrebase_conflicts(
            ctx,
            repo,
            config,
            request.root,
            request.conflict_check_base,
            bookmark_val,
            &request.changesets,
            &request.changed_files,
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
                .changeset_linear_depth(ctx, request.root),
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
        let onto = running_head.unwrap_or(request.root);
        let rebase_result = create_rebased_changesets(
            ctx,
            repo,
            config,
            request.root,
            request.head,
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
                    old_bookmark_value: Some(p.old_bookmark_value.unwrap_or(p.request.root)),
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

    let mut server_cf = find_changed_files(ctx, repo, ancestor, descendant).await?;
    server_cf.extend(find_subtree_changes(&server_bcs)?);

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
                    // Run dry-run merge if enabled (for logging/observability)
                    let dry_run_enabled = justknobs::eval(
                        "scm/mononoke:pushrebase_dry_run_merge_resolution",
                        None,
                        Some(reponame),
                    );
                    if dry_run_enabled {
                        let derive_fsnodes: bool = justknobs::eval(
                            "scm/mononoke:pushrebase_merge_resolution_derive_fsnodes",
                            None,
                            Some(reponame),
                        );
                        dry_run_merge_check(
                            ctx,
                            repo,
                            &conflicts,
                            root,
                            &server_bcs,
                            client_bcs,
                            max_merge_conflicts,
                            max_merge_file_size,
                            derive_fsnodes,
                        )
                        .await;
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
    head: ChangesetId,
    root: ChangesetId,
    client_cf: Vec<MPath>,
    client_bcs: &[BonsaiChangeset],
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
        root,
        root,
        speculative_bv_cs,
        client_bcs,
        &client_cf,
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
        root,
        head,
        &client_cf,
        client_bcs,
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
    root: ChangesetId,
    head: ChangesetId,
    client_cf: &[MPath],
    client_bcs: &[BonsaiChangeset],
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
            root,
            speculative.bookmark_value,
            auth_cs,
            client_bcs,
            client_cf,
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
        root,
        head,
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
            None => request.root,
        };

        let conflict_result = match check_pushrebase_conflicts(
            ctx,
            repo,
            config,
            request.root,
            request.conflict_check_base,
            bookmark_val,
            &request.changesets,
            &request.changed_files,
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
                .changeset_linear_depth(ctx, request.root),
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
                None => request.root,
            };
            let spec_cs = match speculative_bv {
                Some(v) => v,
                None => request.root,
            };

            if auth_cs != spec_cs {
                let delta_result = match check_pushrebase_conflicts(
                    ctx,
                    repo,
                    config,
                    request.root,
                    spec_cs,
                    auth_cs,
                    &request.changesets,
                    &request.changed_files,
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
        let onto = running_head.unwrap_or(request.root);
        let rebase_result = create_rebased_changesets(
            ctx,
            repo,
            config,
            request.root,
            request.head,
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
            old_bookmark_value: Some(p.old_bookmark_value.unwrap_or(p.request.root)),
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
    head: ChangesetId,
    root: ChangesetId,
    client_cf: Vec<MPath>,
    client_bcs: &[BonsaiChangeset],
    prepushrebase_hooks: &[Box<dyn PushrebaseHook>],
) -> Result<PushrebaseOutcome, PushrebaseError> {
    let should_log = config.monitoring_bookmark.as_deref() == Some(onto_bookmark.as_str());
    // Parallel all-bookmarks saturation counters (see do_batched_pushrebase).
    let emit_all_bookmarks = config.monitoring_bookmark.is_some();
    let mut any_attempt_resolved_conflicts = false;
    let repo_args = (repo.repo_identity().name().to_string(),);
    let mut latest_rebase_attempt = root;
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
            root,
            latest_rebase_attempt,
            old_bookmark_value.unwrap_or(root),
            client_bcs,
            &client_cf,
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
            root,
            head,
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
                old_bookmark_value: Some(old_bookmark_value.unwrap_or(root)),
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
            latest_rebase_attempt = old_bookmark_value.unwrap_or(root);
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
    root: ChangesetId,
    head: ChangesetId,
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
        root,
        head,
        old_bookmark_value.unwrap_or(root),
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

    let roots = roots.keys().map(async |root| {
        let root_gen = repo
            .commit_graph()
            .changeset_generation(ctx, *root)
            .await
            .map_err(|_| PushrebaseError::from(PushrebaseInternalError::RootNotFound(*root)))?;

        Result::<_, PushrebaseError>::Ok((*root, root_gen))
    });

    let roots = try_join_all(roots).await?;

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
    let mut queue = VecDeque::new();
    queue.push_back(onto_bookmark_cs_id);

    let mut queued = HashSet::new();
    let mut depth = 0;

    loop {
        if depth > 0 && depth % 1000 == 0 {
            info!(
                "pushrebase depth: {depth}, searching from bookmark {bookmark} at {onto_bookmark_cs_id} back to one of {} possible roots",
                roots.len()
            );
        }

        if let Some(recursion_limit) = config.recursion_limit {
            if depth >= recursion_limit {
                return Err(PushrebaseError::RootTooFarBehind);
            }
        }

        depth += 1;

        let id = queue.pop_front().ok_or_else(|| {
            PushrebaseError::Error(
                PushrebaseInternalError::PushrebaseNoCommonRoot(
                    bookmark.clone(),
                    roots.keys().cloned().collect(),
                )
                .into(),
            )
        })?;

        if let Some(index) = roots.get(&id) {
            if config.forbid_p2_root_rebases && *index != ChildIndex(0) {
                ctx.scuba().clone().log_with_msg(
                    "pushrebase_p2_root_rejected",
                    Some(format!(
                        "root={}, bookmark={}, depth={}, child_index={}",
                        id, bookmark, depth, index.0,
                    )),
                );

                let hgcs = repo.derive_hg_changeset(ctx, id).await?;
                return Err(PushrebaseError::Error(
                    PushrebaseInternalError::P2RootRebaseForbidden(hgcs, bookmark.clone()).into(),
                ));
            }

            return Ok(id);
        }

        let parents = repo.commit_graph().changeset_parents(ctx, id).await?;

        queue.extend(parents.into_iter().filter(|p| queued.insert(*p)));
    }
}

/// find changed files by comparing manifests of `ancestor` and `descendant`
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

async fn find_changed_files(
    ctx: &CoreContext,
    repo: &impl Repo,
    ancestor: ChangesetId,
    descendant: ChangesetId,
) -> Result<Vec<MPath>, PushrebaseError> {
    let id_to_bcs = repo
        .commit_graph()
        .range_stream(ctx, ancestor, descendant)
        .await?
        .map(async |bcs_id| {
            let bcs = bcs_id.load(ctx, repo.repo_blobstore()).await?;
            anyhow::Ok((bcs_id, bcs))
        })
        .buffered(100)
        .try_collect::<HashMap<_, _>>()
        .await?;

    let ids: HashSet<_> = id_to_bcs.keys().copied().collect();

    let file_changes_futs: Vec<_> = id_to_bcs
        .into_iter()
        .filter(|(id, _)| *id != ancestor)
        .map(async |(id, bcs)| {
            let parents: Vec<_> = bcs.parents().collect();
            match *parents {
                [] | [_] => Ok(extract_conflict_files_from_bonsai_changeset(bcs)),
                [p0_id, p1_id] => {
                    match (ids.get(&p0_id), ids.get(&p1_id)) {
                        (Some(_), Some(_)) => {
                            // both parents are in the rebase set, so we can just take
                            // filechanges from bonsai changeset
                            Ok(extract_conflict_files_from_bonsai_changeset(bcs))
                        }
                        (Some(p_id), None) | (None, Some(p_id)) => {
                            // TODO(stash, T40460159) - include copy sources in the list of
                            // conflict files

                            // one of the parents is not in the rebase set, to calculate
                            // changed files in this case we will compute manifest diff
                            // between elements that are in rebase set.
                            find_changed_files_between_manifests(ctx, repo, id, *p_id).await
                        }
                        (None, None) => panic!(
                            "`range_stream` produced invalid result for: ({descendant}, {ancestor})",
                        ),
                    }
                }
                _ => panic!("pushrebase supports only two parents"),
            }
        })
        .collect();

    let file_changes = try_join_all(file_changes_futs).await?;

    let mut file_changes_union = file_changes
        .into_iter()
        .flatten()
        .collect::<HashSet<_>>() // compute union
        .into_iter()
        .collect::<Vec<_>>();
    file_changes_union.sort_unstable();

    Ok(file_changes_union)
}

fn extract_conflict_files_from_bonsai_changeset(bcs: BonsaiChangeset) -> Vec<MPath> {
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

    let repo_name = repo.repo_identity().name();
    let use_content_manifests = justknobs::eval(
        "scm/mononoke:derived_data_use_content_manifests",
        None,
        Some(repo_name),
    );

    let root_id: compat::ContentManifestId = if use_content_manifests {
        repo.repo_derived_data()
            .derive::<RootContentManifestId>(ctx, cs_id, DerivationPriority::LOW)
            .await?
            .into_content_manifest_id()
            .into()
    } else {
        repo.repo_derived_data()
            .derive::<RootFsnodeId>(ctx, cs_id, DerivationPriority::LOW)
            .await?
            .into_fsnode_id()
            .into()
    };

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
    /// Cannot attempt merge (file missing, type mismatch, etc.).
    Skipped(String),
    /// Internal error during fetch.
    Error(anyhow::Error),
}

/// Attempt a 3-way merge on a single file at `path`.
///
/// Fetches the base (root) version from the manifest (content_manifest or
/// fsnode, depending on the JustKnobs gate). The other (server-side) content
/// is passed directly as `other_content_id` to avoid expensive manifest
/// derivation in the critical section — callers obtain it from the bonsai
/// changesets instead.
///
/// If `expected_file_type` is `Some`, validates that the base file type
/// matches (strict mode for actual merge resolution). If `None`, skips type
/// checking (used by dry-run).
async fn try_merge_file(
    ctx: &CoreContext,
    repo: &impl Repo,
    root: ChangesetId,
    path: &NonRootMPath,
    local_content_id: ContentId,
    other_content_id: ContentId,
    expected_file_type: Option<FileType>,
) -> FileMergeOutcome {
    // Fetch base manifest entry (only derivation needed — root is pre-derived)
    let base_file = match fetch_manifest_file(ctx, repo, root, path).await {
        Ok(Some(f)) => f,
        Ok(None) => {
            return FileMergeOutcome::Skipped(format!("file {path} not found in base"));
        }
        Err(e) => return FileMergeOutcome::Error(e),
    };

    // Validate file types if expected type is provided
    if let Some(local_type) = expected_file_type {
        let base_type = base_file.file_type();
        if base_type != local_type {
            return FileMergeOutcome::Skipped(format!(
                "file {path} has type mismatch: base={base_type:?}, local={local_type:?}",
            ));
        }
    }

    // Fetch all three file contents concurrently
    let (base_bytes, local_bytes, other_bytes) = futures::join!(
        filestore::fetch_concat(repo.repo_blobstore(), ctx, base_file.content_id()),
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

/// Dry-run check for merge-resolvable conflicts, logging outcomes
/// to Scuba without changing pushrebase behavior.
///
/// This fetches file content for each conflicting path from the common
/// ancestor, pushed changeset, and bookmark head, then runs merge_text
/// to determine if the conflict would be resolvable. No actual merge
/// result is used — this is purely for observability.
async fn dry_run_merge_check(
    ctx: &CoreContext,
    repo: &impl Repo,
    conflicts: &[PushrebaseConflict],
    root: ChangesetId,
    server_bcs: &[BonsaiChangeset],
    client_bcs: &[BonsaiChangeset],
    max_conflicts: usize,
    max_file_size: u64,
    derive_fsnodes: bool,
) {
    let repo_name = repo.repo_identity().name();

    // If derive_fsnodes is false, check if fsnodes are already derived.
    // If not, skip dry-run to avoid expensive derivation.
    if !derive_fsnodes {
        let root_fsnode = repo
            .repo_derived_data()
            .fetch_derived::<RootFsnodeId>(ctx, root)
            .await;
        if !matches!(root_fsnode, Ok(Some(_))) {
            ctx.scuba()
                .clone()
                .add("repo_name", repo_name)
                .add("merge_dry_run_outcome", "skipped_fsnodes_not_derived")
                .log_with_msg("Pushrebase dry-run merge resolution", None);
            return;
        }
    }

    // Only attempt on exact path matches (left == right), skip prefix conflicts
    let exact_conflicts: Vec<_> = conflicts.iter().filter(|c| c.left == c.right).collect();
    let prefix_conflict_count = conflicts.len() - exact_conflicts.len();

    // Skip if there are any prefix conflicts (e.g. dir vs dir/file from subtree copies)
    if prefix_conflict_count > 0 {
        ctx.scuba()
            .clone()
            .add("repo_name", repo_name)
            .add("merge_dry_run_outcome", "skipped_prefix_conflicts")
            .add(
                "merge_dry_run_prefix_conflicts",
                prefix_conflict_count as i64,
            )
            .add(
                "merge_dry_run_exact_conflicts",
                exact_conflicts.len() as i64,
            )
            .log_with_msg("Pushrebase dry-run merge resolution", None);
        return;
    }

    if exact_conflicts.is_empty() {
        return;
    }

    // Fail early if there are more conflicts than the limit — processing only a
    // subset would give a misleading signal about merge-resolvability.
    if exact_conflicts.len() > max_conflicts {
        ctx.scuba()
            .clone()
            .add("repo_name", repo_name)
            .add("merge_dry_run_outcome", "too_many_conflicts")
            .add(
                "merge_dry_run_total_conflicts",
                exact_conflicts.len() as i64,
            )
            .add("merge_dry_run_max_conflicts", max_conflicts as i64)
            .log_with_msg("Pushrebase dry-run merge resolution", None);
        return;
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

    let mut all_clean = true;
    let mut resolved_count: i64 = 0;
    let mut conflict_count: i64 = 0;
    let mut skipped_count: i64 = 0;
    let mut error_count: i64 = 0;
    let mut skip_reasons: Vec<String> = Vec::new();
    let mut error_reasons: Vec<String> = Vec::new();

    for conflict in &exact_conflicts {
        let path = &conflict.left;

        // Get the NonRootMPath version for file lookups
        let non_root_path = match path.clone().into_optional_non_root_path() {
            Some(nrp) => nrp,
            None => continue,
        };

        // Get local (pushed) content from the client file change
        let local_fc = match client_changes.get(&non_root_path) {
            Some(FileChange::Change(tc)) => tc,
            _ => {
                all_clean = false;
                skipped_count += 1;
                skip_reasons.push(format!(
                    "{non_root_path}: not a tracked change in pushed changeset",
                ));
                continue;
            }
        };

        // Get server (bookmark head) content from the server bonsai changesets
        let server_fc = match server_changes.get(&non_root_path) {
            Some(FileChange::Change(tc)) => tc,
            _ => {
                all_clean = false;
                skipped_count += 1;
                skip_reasons.push(format!(
                    "{non_root_path}: not a tracked change in bookmark head",
                ));
                continue;
            }
        };

        // Fail early if any file exceeds the size limit — we can't resolve
        // all conflicts if we have to skip a file, so the entire merge would fail.
        if local_fc.size() > max_file_size || server_fc.size() > max_file_size {
            ctx.scuba()
                .clone()
                .add("repo_name", repo_name)
                .add("merge_dry_run_outcome", "file_too_large")
                .add("merge_dry_run_file", non_root_path.to_string())
                .add(
                    "merge_dry_run_file_size",
                    std::cmp::max(local_fc.size(), server_fc.size()) as i64,
                )
                .add("merge_dry_run_max_file_size", max_file_size as i64)
                .log_with_msg("Pushrebase dry-run merge resolution", None);
            return;
        }

        match try_merge_file(
            ctx,
            repo,
            root,
            &non_root_path,
            local_fc.content_id(),
            server_fc.content_id(),
            None,
        )
        .await
        {
            FileMergeOutcome::Clean(_) => resolved_count += 1,
            FileMergeOutcome::Conflict(description) => {
                all_clean = false;
                conflict_count += 1;
                skip_reasons.push(description);
            }
            FileMergeOutcome::Skipped(reason) => {
                all_clean = false;
                skipped_count += 1;
                skip_reasons.push(reason);
            }
            FileMergeOutcome::Error(err) => {
                all_clean = false;
                error_count += 1;
                error_reasons.push(format!("{err:#}"));
            }
        }
    }

    let outcome = if all_clean && skipped_count == 0 && error_count == 0 {
        "all_clean"
    } else if conflict_count > 0 {
        "some_conflicts"
    } else if skipped_count > 0 {
        "skipped"
    } else {
        "error"
    };

    let mut scuba = ctx.scuba().clone();
    scuba
        .add("repo_name", repo_name)
        .add("merge_dry_run_outcome", outcome)
        .add("merge_dry_run_resolved", resolved_count)
        .add("merge_dry_run_conflicts", conflict_count)
        .add("merge_dry_run_skipped", skipped_count)
        .add("merge_dry_run_errors", error_count);

    if !skip_reasons.is_empty() {
        scuba.add("merge_dry_run_skip_reasons", skip_reasons.join(", "));
    }
    if !error_reasons.is_empty() {
        scuba.add("merge_dry_run_error_reasons", error_reasons.join(", "));
    }

    scuba.log_with_msg("Pushrebase dry-run merge resolution", None);
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

    // If derive_fsnodes is false, check if fsnodes are already derived.
    // If not, skip merge resolution to avoid expensive derivation in the
    // pushrebase critical section.
    if !derive_fsnodes {
        let root_fsnode = repo
            .repo_derived_data()
            .fetch_derived::<RootFsnodeId>(ctx, root)
            .await
            .map_err(|e| MergeResolutionError::InternalError(e.into()))?;
        if root_fsnode.is_none() {
            return Err(MergeResolutionError::Skipped(
                "fsnodes not derived for base commit".to_string(),
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
    root: ChangesetId,
    head: ChangesetId,
    onto: ChangesetId,
    hooks: &mut [Box<dyn PushrebaseCommitHook>],
    merged_file_overrides: Option<Vec<MergedFileInfo>>,
) -> Result<(ChangesetId, RebasedChangesets, Vec<BonsaiChangeset>), PushrebaseError> {
    let rebased_set = find_rebased_set(ctx, repo, root, head).await?;

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
    for bcs_old in rebased_set {
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
                FileMergeOutcome::Skipped(_) | FileMergeOutcome::Error(_) => {
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

// Order - from lowest generation number to highest
async fn find_rebased_set(
    ctx: &CoreContext,
    repo: &impl Repo,
    root: ChangesetId,
    head: ChangesetId,
) -> Result<Vec<BonsaiChangeset>, PushrebaseError> {
    fetch_bonsai_range_ancestor_not_included(ctx, repo, root, head).await
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
mod tests {
    use std::collections::BTreeMap;
    use std::str::FromStr;
    use std::time::Duration;

    use anyhow::Context;
    use anyhow::format_err;
    use async_trait::async_trait;
    use blobrepo_hg::BlobRepoHg;
    use bonsai_hg_mapping::BonsaiHgMapping;
    use bonsai_hg_mapping::BonsaiHgMappingRef;
    use bookmarks::BookmarkTransactionError;
    use bookmarks::Bookmarks;
    use cloned::cloned;
    use commit_graph::CommitGraph;
    use commit_graph::CommitGraphWriter;
    use fbinit::FacebookInit;
    use filestore::FilestoreConfig;
    use fixtures::Linear;
    use fixtures::ManyFilesDirs;
    use fixtures::MergeEven;
    use fixtures::TestRepoFixture;
    use futures::future::TryFutureExt;
    use futures::future::try_join_all;
    use futures::stream;
    use futures::stream::TryStreamExt;
    use justknobs::test_helpers::JustKnobsInMemory;
    use justknobs::test_helpers::KnobVal;
    use justknobs::test_helpers::override_just_knobs;
    use manifest::Entry;
    use manifest::ManifestOps;
    use maplit::btreemap;
    use maplit::hashmap;
    use maplit::hashset;
    use mononoke_macros::mononoke;
    use mononoke_types::BonsaiChangesetMut;
    use mononoke_types::FileType;
    use mononoke_types::GitLfs;
    use mononoke_types::PrefixTrie;
    use mononoke_types::RepositoryId;
    use mutable_counters::MutableCounters;
    use mutable_counters::MutableCountersRef;
    use mutable_counters::SqlMutableCounters;
    use repo_blobstore::RepoBlobstore;
    use repo_blobstore::RepoBlobstoreRef;
    use repo_derived_data::RepoDerivedData;
    use repo_identity::RepoIdentity;
    use sql_ext::Transaction;
    use sql_ext::TransactionResult;
    use test_repo_factory::TestRepoFactory;
    use tests_utils::CreateCommitContext;
    use tests_utils::bookmark;
    use tests_utils::drawdag::extend_from_dag_with_actions;
    use tests_utils::resolve_cs_id;

    use super::*;

    fn init_just_knobs_for_test() {
        override_just_knobs(JustKnobsInMemory::new(hashmap! {
            "scm/mononoke:pushrebase_dry_run_merge_resolution".to_string() => KnobVal::Bool(false),
            "scm/mononoke:pushrebase_enable_merge_resolution".to_string() => KnobVal::Bool(false),
            "scm/mononoke:pushrebase_merge_resolution_derive_fsnodes".to_string() => KnobVal::Bool(true),
            "scm/mononoke:per_bookmark_locking".to_string() => KnobVal::Bool(false),
        }));
    }

    #[facet::container]
    #[derive(Clone)]
    struct PushrebaseTestRepo {
        #[facet]
        bonsai_hg_mapping: dyn BonsaiHgMapping,

        #[facet]
        bookmarks: dyn Bookmarks,

        #[facet]
        sql_bookmarks: dbbookmarks::SqlBookmarks,

        #[facet]
        repo_blobstore: RepoBlobstore,

        #[facet]
        repo_derived_data: RepoDerivedData,

        #[facet]
        repo_identity: RepoIdentity,

        #[facet]
        filestore_config: FilestoreConfig,

        #[facet]
        mutable_counters: dyn MutableCounters,

        #[facet]
        commit_graph: CommitGraph,

        #[facet]
        commit_graph_writer: dyn CommitGraphWriter,
    }

    async fn fetch_bonsai_changesets(
        ctx: &CoreContext,
        repo: &(impl Repo + BonsaiHgMappingRef),
        commit_ids: &HashSet<HgChangesetId>,
    ) -> Result<HashSet<BonsaiChangeset>, PushrebaseError> {
        let futs = commit_ids.iter().map(async |hg_cs_id| {
            let bcs_id = repo
                .bonsai_hg_mapping()
                .get_bonsai_from_hg(ctx, *hg_cs_id)
                .await?
                .ok_or_else(|| {
                    Error::from(PushrebaseInternalError::BonsaiNotFoundForHgChangeset(
                        *hg_cs_id,
                    ))
                })?;

            let bcs = bcs_id
                .load(ctx, repo.repo_blobstore())
                .await
                .context("While initial bonsai changesets fetching")?;

            Result::<_, Error>::Ok(bcs)
        });

        let ret = try_join_all(futs).await?.into_iter().collect();
        Ok(ret)
    }

    async fn do_pushrebase(
        ctx: &CoreContext,
        repo: &(impl PushrebaseRepo + BonsaiHgMappingRef),
        config: &PushrebaseFlags,
        onto_bookmark: &BookmarkKey,
        pushed_set: &HashSet<HgChangesetId>,
    ) -> Result<PushrebaseOutcome, PushrebaseError> {
        init_just_knobs_for_test();
        let pushed = fetch_bonsai_changesets(ctx, repo, pushed_set).await?;

        let res = do_pushrebase_bonsai(ctx, repo, config, onto_bookmark, &pushed, &[]).await?;

        Ok(res)
    }

    async fn set_bookmark(
        ctx: CoreContext,
        repo: &(impl Repo + BonsaiHgMappingRef),
        book: &BookmarkKey,
        cs_id: &str,
    ) -> Result<(), Error> {
        let head = HgChangesetId::from_str(cs_id)?;
        let head = repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(&ctx, head)
            .await?
            .ok_or_else(|| Error::msg(format_err!("Head not found: {cs_id:?}")))?;

        let mut txn = repo.bookmarks().create_transaction(ctx);
        txn.force_set(book, head, BookmarkUpdateReason::TestMove)?;
        txn.commit().await?;
        Ok(())
    }

    fn make_paths(paths: &[&str]) -> Vec<MPath> {
        let paths: Result<_, _> = paths.iter().map(MPath::new).collect();
        paths.unwrap()
    }

    fn master_bookmark() -> BookmarkKey {
        BookmarkKey::new("master").unwrap()
    }

    async fn push_and_verify(
        ctx: &CoreContext,
        repo: &(impl PushrebaseRepo + BonsaiHgMappingRef),
        parent: ChangesetId,
        bookmark: &BookmarkKey,
        content: BTreeMap<&str, Option<&str>>,
        should_succeed: bool,
    ) -> Result<(), Error> {
        let mut commit_ctx = CreateCommitContext::new(ctx, repo, vec![parent]);

        for (path, maybe_content) in content.iter() {
            let path: &str = path;
            commit_ctx = match maybe_content {
                Some(content) => commit_ctx.add_file(path, *content),
                None => commit_ctx.delete_file(path),
            };
        }

        let cs_id = commit_ctx.commit().await?;

        let hgcss = hashset![repo.derive_hg_changeset(ctx, cs_id).await?];

        let res = do_pushrebase(ctx, repo, &PushrebaseFlags::default(), bookmark, &hgcss).await;

        if should_succeed {
            assert!(res.is_ok());
        } else {
            should_have_conflicts(res);
        }

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_one_commit(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;
        // Bottom commit of the repo
        let parents = vec!["2d7d4ba9ce0a6ffd222de7785b249ead9c51c536"];
        let bcs_id = CreateCommitContext::new(&ctx, &repo, parents)
            .add_file("file", "content")
            .commit()
            .await?;

        let hg_cs = repo.derive_hg_changeset(&ctx, bcs_id).await?;

        let book = master_bookmark();
        bookmark(&ctx, &repo, book.clone())
            .set_to("a5ffa77602a066db7d5cfb9fb5823a0895717c5a")
            .await?;

        do_pushrebase(&ctx, &repo, &Default::default(), &book, &hashset![hg_cs])
            .map_err(|err| format_err!("{err:?}"))
            .await?;
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_one_commit_transaction_hook(fb: FacebookInit) -> Result<(), Error> {
        #[derive(Copy, Clone)]
        struct Hook(RepositoryId);

        #[async_trait]
        impl PushrebaseHook for Hook {
            async fn in_critical_section(
                &self,
                _ctx: &CoreContext,
                _old_bookmark_value: Option<ChangesetId>,
            ) -> Result<Box<dyn PushrebaseCommitHook>, Error> {
                Ok(Box::new(*self) as Box<dyn PushrebaseCommitHook>)
            }
        }

        #[async_trait]
        impl PushrebaseCommitHook for Hook {
            fn post_rebase_changeset(
                &mut self,
                _bcs_old: ChangesetId,
                _bcs_new: &mut BonsaiChangesetMut,
            ) -> Result<(), Error> {
                Ok(())
            }

            async fn into_transaction_hook(
                self: Box<Self>,
                _ctx: &CoreContext,
                changesets: &RebasedChangesets,
            ) -> Result<Box<dyn PushrebaseTransactionHook>, Error> {
                let (_, (cs_id, _)) = changesets
                    .iter()
                    .next()
                    .ok_or_else(|| Error::msg("No rebased changeset"))?;
                Ok(Box::new(TransactionHook(self.0, *cs_id)) as Box<dyn PushrebaseTransactionHook>)
            }
        }

        struct TransactionHook(RepositoryId, ChangesetId);

        #[async_trait]
        impl PushrebaseTransactionHook for TransactionHook {
            async fn populate_transaction(
                &self,
                ctx: &CoreContext,
                txn: Transaction,
            ) -> Result<Transaction, BookmarkTransactionError> {
                let key = format!("{}", self.1);

                let ret =
                    SqlMutableCounters::set_counter_on_txn(ctx, self.0, &key, 1, None, txn).await?;

                match ret {
                    TransactionResult::Succeeded(txn) => Ok(txn),
                    TransactionResult::Failed => Err(Error::msg("Did not update").into()),
                }
            }
        }

        let ctx = CoreContext::test_mock(fb);
        let factory = TestRepoFactory::new(fb)?;
        let repo: PushrebaseTestRepo = factory.build().await?;
        Linear::init_repo(fb, &repo).await?;
        // Bottom commit of the repo
        let parents = vec!["2d7d4ba9ce0a6ffd222de7785b249ead9c51c536"];
        let bcs_id = CreateCommitContext::new(&ctx, &repo, parents)
            .add_file("file", "content")
            .commit()
            .await?;

        let bcs = bcs_id.load(&ctx, repo.repo_blobstore()).await?;

        let mut book = master_bookmark();

        bookmark(&ctx, &repo, book.clone())
            .set_to("a5ffa77602a066db7d5cfb9fb5823a0895717c5a")
            .await?;

        let hook: Box<dyn PushrebaseHook> = Box::new(Hook(repo.repo_identity().id()));
        let hooks = [hook];

        do_pushrebase_bonsai(
            &ctx,
            &repo,
            &Default::default(),
            &book,
            &hashset![bcs.clone()],
            &hooks,
        )
        .map_err(|err| format_err!("{err:?}"))
        .await?;

        let master_val = resolve_cs_id(&ctx, &repo, "master").await?;
        let key = format!("{master_val}");
        assert_eq!(
            repo.mutable_counters().get_counter(&ctx, &key).await?,
            Some(1),
        );

        // Now do the same with another non-existent bookmark,
        // make sure cs id is created.
        book = BookmarkKey::new("newbook")?;
        do_pushrebase_bonsai(
            &ctx,
            &repo,
            &Default::default(),
            &book,
            &hashset![bcs],
            &hooks,
        )
        .map_err(|err| format_err!("{err:?}"))
        .await?;

        let key = format!("{}", resolve_cs_id(&ctx, &repo, "newbook").await?);
        assert_eq!(
            repo.mutable_counters().get_counter(&ctx, &key).await?,
            Some(1),
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_stack(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;
        // Bottom commit of the repo
        let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
        let p = repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(&ctx, root)
            .await?
            .ok_or_else(|| Error::msg("Root is missing"))?;
        let bcs_id_1 = CreateCommitContext::new(&ctx, &repo, vec![p])
            .add_file("file", "content")
            .commit()
            .await?;
        let bcs_id_2 = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_1])
            .add_file("file2", "content")
            .commit()
            .await?;

        assert_eq!(
            find_changed_files(&ctx, &repo, p, bcs_id_2).await?,
            make_paths(&["file", "file2"]),
        );

        let book = master_bookmark();
        set_bookmark(
            ctx.clone(),
            &repo,
            &book,
            "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
        )
        .await?;

        let hg_cs_1 = repo.derive_hg_changeset(&ctx, bcs_id_1).await?;
        let hg_cs_2 = repo.derive_hg_changeset(&ctx, bcs_id_2).await?;
        do_pushrebase(
            &ctx,
            &repo,
            &Default::default(),
            &book,
            &hashset![hg_cs_1, hg_cs_2],
        )
        .await?;
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_stack_with_renames(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;
        // Bottom commit of the repo
        let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
        let p = repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(&ctx, root)
            .await?
            .ok_or_else(|| Error::msg("p is missing"))?;
        let bcs_id_1 = CreateCommitContext::new(&ctx, &repo, vec![p])
            .add_file("file", "content")
            .commit()
            .await?;
        let bcs_id_2 = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_1])
            .add_file_with_copy_info("file_renamed", "content", (bcs_id_1, "file"))
            .commit()
            .await?;

        assert_eq!(
            find_changed_files(&ctx, &repo, p, bcs_id_2).await?,
            make_paths(&["file", "file_renamed"]),
        );

        let book = master_bookmark();
        set_bookmark(
            ctx.clone(),
            &repo,
            &book,
            "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
        )
        .await?;

        let hg_cs_1 = repo.derive_hg_changeset(&ctx, bcs_id_1).await?;
        let hg_cs_2 = repo.derive_hg_changeset(&ctx, bcs_id_2).await?;
        do_pushrebase(
            &ctx,
            &repo,
            &Default::default(),
            &book,
            &hashset![hg_cs_1, hg_cs_2],
        )
        .await?;

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_multi_root(fb: FacebookInit) -> Result<(), Error> {
        //
        // master -> o
        //           |
        //           :  o <- bcs3
        //           :  |
        //           :  o <- bcs2
        //           : /|
        //           |/ |
        //  root1 -> o  |
        //           |  o <- bcs1 (outside of rebase set)
        //           o /
        //           |/
        //  root0 -> o
        //
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;
        let config = PushrebaseFlags::default();

        let root0 = repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(
                &ctx,
                HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?,
            )
            .await?
            .ok_or_else(|| Error::msg("root0 is missing"))?;

        let root1 = repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(
                &ctx,
                HgChangesetId::from_str("607314ef579bd2407752361ba1b0c1729d08b281")?,
            )
            .await?
            .ok_or_else(|| Error::msg("root0 is missing"))?;

        let bcs_id_1 = CreateCommitContext::new(&ctx, &repo, vec![root0])
            .add_file("f0", "f0")
            .delete_file("files")
            .commit()
            .await?;
        let bcs_id_2 = CreateCommitContext::new(&ctx, &repo, vec![root1, bcs_id_1])
            .add_file("f1", "f1")
            .commit()
            .await?;
        let bcs_id_3 = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_2])
            .add_file("f2", "f2")
            .commit()
            .await?;

        let book = master_bookmark();
        set_bookmark(
            ctx.clone(),
            &repo,
            &book,
            "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
        )
        .await?;
        let bcs_id_master = repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(
                &ctx,
                HgChangesetId::from_str("a5ffa77602a066db7d5cfb9fb5823a0895717c5a")?,
            )
            .await?
            .ok_or_else(|| Error::msg("bcs_id_master is missing"))?;

        let root = root1;
        assert_eq!(
            find_closest_root(
                &ctx,
                &repo,
                &config,
                &book,
                &hashmap! {root0 => ChildIndex(0), root1 => ChildIndex(0) },
            )
            .await?,
            root,
        );

        assert_eq!(
            find_changed_files(&ctx, &repo, root, bcs_id_3).await?,
            make_paths(&["f0", "f1", "f2"]),
        );

        let hg_cs_1 = repo.derive_hg_changeset(&ctx, bcs_id_1).await?;
        let hg_cs_2 = repo.derive_hg_changeset(&ctx, bcs_id_2).await?;
        let hg_cs_3 = repo.derive_hg_changeset(&ctx, bcs_id_3).await?;
        let bcs_id_rebased = do_pushrebase(
            &ctx,
            &repo,
            &config,
            &book,
            &hashset![hg_cs_1, hg_cs_2, hg_cs_3],
        )
        .await?;

        // should only rebase {bcs2, bcs3}
        let rebased = find_rebased_set(&ctx, &repo, bcs_id_master, bcs_id_rebased.head).await?;
        assert_eq!(rebased.len(), 2);
        let bcs2 = &rebased[0];
        let bcs3 = &rebased[1];

        // bcs3 parent correctly updated and contains only {bcs2}
        assert_eq!(
            bcs3.parents().collect::<Vec<_>>(),
            vec![bcs2.get_changeset_id()]
        );

        // bcs2 parents contains old bcs1 and old master bookmark
        assert_eq!(
            bcs2.parents().collect::<HashSet<_>>(),
            hashset! { bcs_id_1, bcs_id_master },
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_conflict(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;
        let root = repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(
                &ctx,
                HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?,
            )
            .await?
            .ok_or_else(|| Error::msg("Root is missing"))?;

        let bcs_id_1 = CreateCommitContext::new(&ctx, &repo, vec![root])
            .add_file("f0", "f0")
            .commit()
            .await?;
        let bcs_id_2 = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_1])
            .add_file("9/file", "file")
            .commit()
            .await?;
        let bcs_id_3 = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_2])
            .add_file("f1", "f1")
            .commit()
            .await?;

        let book = master_bookmark();
        set_bookmark(
            ctx.clone(),
            &repo,
            &book,
            "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
        )
        .await?;

        let hg_cs_1 = repo.derive_hg_changeset(&ctx, bcs_id_1).await?;
        let hg_cs_2 = repo.derive_hg_changeset(&ctx, bcs_id_2).await?;
        let hg_cs_3 = repo.derive_hg_changeset(&ctx, bcs_id_3).await?;
        let result = do_pushrebase(
            &ctx,
            &repo,
            &Default::default(),
            &book,
            &hashset![hg_cs_1, hg_cs_2, hg_cs_3],
        )
        .await;
        match result {
            Err(PushrebaseError::Conflicts(conflicts)) => {
                assert_eq!(
                    conflicts,
                    vec![PushrebaseConflict {
                        left: MPath::new("9")?,
                        right: MPath::new("9/file")?,
                    },],
                );
            }
            _ => panic!("push-rebase should have failed with conflict"),
        }
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_caseconflicting_rename(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;
        let root = repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(
                &ctx,
                HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?,
            )
            .await?
            .ok_or_else(|| Error::msg("Root is missing"))?;

        let bcs_id_1 = CreateCommitContext::new(&ctx, &repo, vec![root])
            .add_file("FILE", "file")
            .commit()
            .await?;
        let bcs_id_2 = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_1])
            .delete_file("FILE")
            .commit()
            .await?;
        let bcs_id_3 = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_2])
            .add_file("file", "file")
            .commit()
            .await?;

        let hgcss = hashset![
            repo.derive_hg_changeset(&ctx, bcs_id_1).await?,
            repo.derive_hg_changeset(&ctx, bcs_id_2).await?,
            repo.derive_hg_changeset(&ctx, bcs_id_3).await?,
        ];

        let book = master_bookmark();
        set_bookmark(
            ctx.clone(),
            &repo,
            &book,
            "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
        )
        .await?;

        do_pushrebase(&ctx, &repo, &Default::default(), &book, &hgcss).await?;

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_caseconflicting_dirs(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;
        let root = repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(
                &ctx,
                HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?,
            )
            .await?
            .ok_or_else(|| Error::msg("Root is missing"))?;

        let bcs_id_1 = CreateCommitContext::new(&ctx, &repo, vec![root])
            .add_file("DIR/a", "a")
            .add_file("DIR/b", "b")
            .commit()
            .await?;
        let bcs_id_2 = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_1])
            .add_file("dir/a", "a")
            .delete_file("DIR/a")
            .delete_file("DIR/b")
            .commit()
            .await?;
        let hgcss = hashset![
            repo.derive_hg_changeset(&ctx, bcs_id_1).await?,
            repo.derive_hg_changeset(&ctx, bcs_id_2).await?,
        ];

        let book = master_bookmark();
        set_bookmark(
            ctx.clone(),
            &repo,
            &book,
            "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
        )
        .await?;

        do_pushrebase(&ctx, &repo, &Default::default(), &book, &hgcss).await?;

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_recursion_limit(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;
        let root = repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(
                &ctx,
                HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?,
            )
            .await?
            .ok_or_else(|| Error::msg("Root is missing"))?;

        // create a lot of commits
        let (_, bcss) = stream::iter((0..128usize).map(Ok))
            .try_fold((root, vec![]), async |(head, mut bcss), index| {
                let file = format!("f{index}");
                let content = format!("{index}");
                let bcs = CreateCommitContext::new(&ctx, &repo, vec![head])
                    .add_file(file.as_str(), content)
                    .commit()
                    .await?;
                bcss.push(bcs);
                Result::<_, Error>::Ok((bcs, bcss))
            })
            .await?;

        let hgcss =
            try_join_all(bcss.iter().map(|bcs| repo.derive_hg_changeset(&ctx, *bcs))).await?;
        let book = master_bookmark();
        set_bookmark(
            ctx.clone(),
            &repo,
            &book,
            "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
        )
        .await?;
        do_pushrebase(
            &ctx,
            &repo,
            &Default::default(),
            &book.clone(),
            &hgcss.into_iter().collect(),
        )
        .await?;

        let bcs = CreateCommitContext::new(&ctx, &repo, vec![root])
            .add_file("file", "data")
            .commit()
            .await?;

        let hgcss = hashset![repo.derive_hg_changeset(&ctx, bcs).await?];

        // try rebase with small recursion limit
        let config = PushrebaseFlags {
            recursion_limit: Some(128),
            ..Default::default()
        };
        let result = do_pushrebase(&ctx, &repo, &config, &book, &hgcss).await;
        match result {
            Err(PushrebaseError::RootTooFarBehind) => {}
            _ => panic!("push-rebase should have failed because root too far behind"),
        }

        let config = PushrebaseFlags {
            recursion_limit: Some(256),
            ..Default::default()
        };
        do_pushrebase(&ctx, &repo, &config, &book, &hgcss).await?;

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_rewritedates(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;
        let (commits, _dag) = extend_from_dag_with_actions(
            &ctx,
            &repo,
            r#"
                A-B-C
                   \
                    D
                # author_date: D "2020-01-01 01:00:00+04:00"
                # committer: D "Committer <committer@example.test>"
                # committer_date: D "2020-01-01 09:00:00-02:00"
                # bookmark: C keep
                # bookmark: C rewrite
            "#,
        )
        .await?;

        let config = PushrebaseFlags {
            rewritedates: false,
            ..Default::default()
        };
        let source = hashset![commits["D"].load(&ctx, repo.repo_blobstore()).await?];
        let bcs_keep_date = do_pushrebase_bonsai(
            &ctx,
            &repo,
            &config,
            &BookmarkKey::new("keep")?,
            &source,
            &[],
        )
        .await?;

        let config = PushrebaseFlags {
            rewritedates: true,
            ..Default::default()
        };
        let bcs_rewrite_date = do_pushrebase_bonsai(
            &ctx,
            &repo,
            &config,
            &BookmarkKey::new("rewrite")?,
            &source,
            &[],
        )
        .await?;

        let bcs = commits["D"].load(&ctx, repo.repo_blobstore()).await?;
        let bcs_keep_date = bcs_keep_date.head.load(&ctx, repo.repo_blobstore()).await?;
        let bcs_rewrite_date = bcs_rewrite_date
            .head
            .load(&ctx, repo.repo_blobstore())
            .await?;

        // For the keep variant, the time should not have changed.
        assert_eq!(bcs.author_date(), bcs_keep_date.author_date());
        assert_eq!(bcs.committer_date(), bcs_keep_date.committer_date());

        // For the rewrite variant, the time should be updated.
        assert!(bcs.author_date() < bcs_rewrite_date.author_date());
        assert!(bcs.committer_date() < bcs_rewrite_date.committer_date());

        // Timezone shouldn't have changed for either author or committer.
        assert_eq!(
            bcs.author_date().tz_offset_secs(),
            bcs_rewrite_date.author_date().tz_offset_secs()
        );
        assert_eq!(
            bcs.committer_date().unwrap().tz_offset_secs(),
            bcs_rewrite_date.committer_date().unwrap().tz_offset_secs()
        );

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_case_conflict(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = ManyFilesDirs::get_repo(fb).await;
        let root = repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(
                &ctx,
                HgChangesetId::from_str("5a28e25f924a5d209b82ce0713d8d83e68982bc8")?,
            )
            .await?
            .ok_or_else(|| Error::msg("Root is missing"))?;

        let bcs = CreateCommitContext::new(&ctx, &repo, vec![root])
            .add_file("Dir1/file_1_in_dir1", "data")
            .commit()
            .await?;

        let hgcss = hashset![repo.derive_hg_changeset(&ctx, bcs).await?];

        let book = master_bookmark();
        set_bookmark(
            ctx.clone(),
            &repo,
            &book,
            "2f866e7e549760934e31bf0420a873f65100ad63",
        )
        .await?;

        let result = do_pushrebase(&ctx, &repo, &Default::default(), &book, &hgcss).await;
        match result {
            Err(PushrebaseError::PotentialCaseConflict(conflict)) => {
                assert_eq!(conflict, NonRootMPath::new("Dir1/file_1_in_dir1")?)
            }
            _ => panic!("push-rebase should have failed with case conflict"),
        };

        // make sure that it is succeeds with disabled casefolding
        do_pushrebase(
            &ctx,
            &repo,
            &PushrebaseFlags {
                casefolding_check: false,
                ..Default::default()
            },
            &book,
            &hgcss,
        )
        .await?;

        Ok(())
    }
    #[mononoke::fbinit_test]

    async fn pushrebase_case_conflict_exclusion(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = ManyFilesDirs::get_repo(fb).await;
        let root = repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(
                &ctx,
                HgChangesetId::from_str("5a28e25f924a5d209b82ce0713d8d83e68982bc8")?,
            )
            .await?
            .ok_or_else(|| Error::msg("Root is missing"))?;

        let bcs1 = CreateCommitContext::new(&ctx, &repo, vec![root])
            .add_file("dir1/File_1_in_dir1", "data")
            .commit()
            .await?;

        let hgcs1 = hashset![repo.derive_hg_changeset(&ctx, bcs1).await?];

        let bcs2 = CreateCommitContext::new(&ctx, &repo, vec![root])
            .add_file("dir2/File_1_in_dir2", "data")
            .commit()
            .await?;

        let hgcs2 = hashset![repo.derive_hg_changeset(&ctx, bcs2).await?];

        let book = master_bookmark();
        set_bookmark(
            ctx.clone(),
            &repo,
            &book,
            "2f866e7e549760934e31bf0420a873f65100ad63",
        )
        .await?;

        let result = do_pushrebase(&ctx, &repo, &Default::default(), &book, &hgcs1).await;
        match result {
            Err(PushrebaseError::PotentialCaseConflict(conflict)) => {
                assert_eq!(conflict, NonRootMPath::new("dir1/File_1_in_dir1")?)
            }
            _ => panic!("push-rebase should have failed with case conflict"),
        };

        // make sure that it is succeeds with exclusion
        do_pushrebase(
            &ctx,
            &repo,
            &PushrebaseFlags {
                casefolding_check: true,
                casefolding_check_excluded_paths: PrefixTrie::from_iter(
                    vec![Some(NonRootMPath::new("dir1")?)].into_iter(),
                ),
                ..Default::default()
            },
            &book,
            &hgcs1,
        )
        .await?;

        // revert bookmark back
        set_bookmark(
            ctx.clone(),
            &repo,
            &book,
            "2f866e7e549760934e31bf0420a873f65100ad63",
        )
        .await?;
        // make sure that exclusion doesn't exclude too much
        let result = do_pushrebase(
            &ctx,
            &repo,
            &PushrebaseFlags {
                casefolding_check: true,
                casefolding_check_excluded_paths: PrefixTrie::from_iter(
                    vec![Some(NonRootMPath::new("dir1")?)].into_iter(),
                ),
                ..Default::default()
            },
            &book,
            &hgcs2,
        )
        .await;
        match result {
            Err(PushrebaseError::PotentialCaseConflict(conflict)) => {
                assert_eq!(conflict, NonRootMPath::new("dir2/File_1_in_dir2")?)
            }
            _ => panic!("push-rebase should have failed with case conflict"),
        };
        Ok(())
    }

    #[mononoke::test]
    fn pushrebase_intersect_changed() -> Result<(), Error> {
        match intersect_changed_files(
            make_paths(&["a/b/c", "c", "a/b/d", "d/d", "b", "e/c"]),
            make_paths(&["d/f", "a/b/d/f", "c", "e"]),
        ) {
            Err(PushrebaseError::Conflicts(conflicts)) => assert_eq!(
                *conflicts,
                [
                    PushrebaseConflict {
                        left: MPath::new("a/b/d")?,
                        right: MPath::new("a/b/d/f")?,
                    },
                    PushrebaseConflict {
                        left: MPath::new("c")?,
                        right: MPath::new("c")?,
                    },
                    PushrebaseConflict {
                        left: MPath::new("e/c")?,
                        right: MPath::new("e")?,
                    },
                ]
            ),
            _ => panic!("should contain conflict"),
        };

        Ok(())
    }

    #[mononoke::test]
    fn pushrebase_intersect_changed_with_reponame() -> Result<(), Error> {
        // Verifies intersect_changed_files detects exact path conflicts
        match intersect_changed_files(make_paths(&["a/b/c"]), make_paths(&["a/b/c"])) {
            Err(PushrebaseError::Conflicts(conflicts)) => {
                assert_eq!(conflicts.len(), 1);
                assert_eq!(
                    conflicts[0],
                    PushrebaseConflict {
                        left: MPath::new("a/b/c")?,
                        right: MPath::new("a/b/c")?,
                    }
                );
                Ok(())
            }
            _ => Err(Error::msg("expected conflict")),
        }
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_executable_bit_change(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;
        let path_1 = NonRootMPath::new("1")?;

        let root_hg = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
        let root_cs = root_hg.load(&ctx, repo.repo_blobstore()).await?;

        let root_1_id = root_cs
            .manifestid()
            .find_entry(
                ctx.clone(),
                repo.repo_blobstore().clone(),
                path_1.clone().into(),
            )
            .await?
            .and_then(|entry| Some(entry.into_leaf()?.1))
            .ok_or_else(|| Error::msg("path_1 missing in manifest"))?;

        // crate filechange with with same content as "1" but set executable bit
        let root = repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(&ctx, root_hg)
            .await?
            .ok_or_else(|| Error::msg("Root missing"))?;
        let root_bcs = root.load(&ctx, repo.repo_blobstore()).await?;
        let file_1 = match root_bcs
            .file_changes()
            .find(|(path, _)| path == &&path_1)
            .ok_or_else(|| Error::msg("path_1 missing in file_changes"))?
            .1
        {
            FileChange::Change(tc) => tc.clone(),
            _ => return Err(Error::msg("path_1 change info missing")),
        };
        assert_eq!(file_1.file_type(), FileType::Regular);
        let file_1_exec = FileChange::tracked(
            file_1.content_id(),
            FileType::Executable,
            file_1.size(),
            /* copy_from */ None,
            GitLfs::FullContent,
        );

        let bcs = CreateCommitContext::new(&ctx, &repo, vec![root])
            .add_file_change(path_1.clone(), file_1_exec.clone())
            .commit()
            .await?;

        let hgcss = hashset![repo.derive_hg_changeset(&ctx, bcs).await?];

        let book = master_bookmark();
        set_bookmark(
            ctx.clone(),
            &repo,
            &book,
            "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
        )
        .await?;

        let result = do_pushrebase(&ctx, &repo, &Default::default(), &book, &hgcss).await?;
        let result_bcs = result.head.load(&ctx, repo.repo_blobstore()).await?;
        let file_1_result = match result_bcs
            .file_changes()
            .find(|(path, _)| path == &&path_1)
            .ok_or_else(|| Error::msg("path_1 missing in file_changes"))?
            .1
        {
            FileChange::Change(tc) => tc.clone(),
            _ => return Err(Error::msg("path_1 change info missing")),
        };
        assert_eq!(FileChange::Change(file_1_result), file_1_exec);

        let result_hg = repo.derive_hg_changeset(&ctx, result.head).await?;
        let result_cs = result_hg.load(&ctx, repo.repo_blobstore()).await?;
        let result_1_id = result_cs
            .manifestid()
            .find_entry(
                ctx.clone(),
                repo.repo_blobstore().clone(),
                path_1.clone().into(),
            )
            .await?
            .and_then(|entry| Some(entry.into_leaf()?.1))
            .ok_or_else(|| Error::msg("path_1 missing in manifest"))?;

        // `result_1_id` should be equal to `root_1_id`, because executable flag
        // is not a part of file envelope
        assert_eq!(root_1_id, result_1_id);

        Ok(())
    }

    async fn count_commits_between(
        ctx: CoreContext,
        repo: &(impl Repo + BonsaiHgMappingRef),
        ancestor: HgChangesetId,
        descendant: BookmarkKey,
    ) -> Result<usize, Error> {
        let ancestor = repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(&ctx, ancestor)
            .await?
            .ok_or_else(|| Error::msg("ancestor not found"))?;

        let descendant = repo
            .get_bookmark_hg(ctx.clone(), &descendant)
            .await?
            .ok_or_else(|| Error::msg("bookmark not found"))?;

        let descendant = repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(&ctx, descendant)
            .await?
            .ok_or_else(|| Error::msg("bonsai not found"))?;

        let n = repo
            .commit_graph()
            .range_stream(&ctx, ancestor, descendant)
            .await?
            .count()
            .await;

        Ok(n)
    }

    #[derive(Copy, Clone)]
    struct SleepHook;

    #[async_trait]
    impl PushrebaseHook for SleepHook {
        async fn in_critical_section(
            &self,
            _ctx: &CoreContext,
            _old_bookmark_value: Option<ChangesetId>,
        ) -> Result<Box<dyn PushrebaseCommitHook>, Error> {
            let us = rand::random_range(0..100);
            tokio::time::sleep(Duration::from_micros(us)).await;
            Ok(Box::new(*self) as Box<dyn PushrebaseCommitHook>)
        }
    }

    #[async_trait]
    impl PushrebaseCommitHook for SleepHook {
        fn post_rebase_changeset(
            &mut self,
            _bcs_old: ChangesetId,
            _bcs_new: &mut BonsaiChangesetMut,
        ) -> Result<(), Error> {
            Ok(())
        }

        async fn into_transaction_hook(
            self: Box<Self>,
            _ctx: &CoreContext,
            _changesets: &RebasedChangesets,
        ) -> Result<Box<dyn PushrebaseTransactionHook>, Error> {
            Ok(Box::new(*self) as Box<dyn PushrebaseTransactionHook>)
        }
    }

    #[async_trait]
    impl PushrebaseTransactionHook for SleepHook {
        async fn populate_transaction(
            &self,
            _ctx: &CoreContext,
            txn: Transaction,
        ) -> Result<Transaction, BookmarkTransactionError> {
            Ok(txn)
        }
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_simultaneously(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;
        // Bottom commit of the repo
        let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
        let p = repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(&ctx, root)
            .await?
            .ok_or_else(|| Error::msg("Root is missing"))?;
        let parents = vec![p];

        let book = master_bookmark();
        set_bookmark(
            ctx.clone(),
            &repo,
            &book,
            "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
        )
        .await?;

        let num_pushes = 10;
        let mut futs = vec![];
        for i in 0..num_pushes {
            cloned!(ctx, repo, book);

            let hooks = [Box::new(SleepHook) as Box<dyn PushrebaseHook>];

            let f = format!("file{i}");
            let bcs_id = CreateCommitContext::new(&ctx, &repo, parents.clone())
                .add_file(f.as_str(), "content")
                .commit()
                .await?;

            let bcs = bcs_id.load(&ctx, repo.repo_blobstore()).await?;

            let fut = async move {
                do_pushrebase_bonsai(
                    &ctx,
                    &repo,
                    &Default::default(),
                    &book,
                    &hashset![bcs],
                    &hooks,
                )
                .await
            };

            futs.push(fut);
        }

        let res = try_join_all(futs).await?;
        let mut has_retry_num_bigger_1 = false;
        for r in res {
            if r.retry_num.0 > 1 {
                has_retry_num_bigger_1 = true;
            }
        }

        assert!(has_retry_num_bigger_1);

        let previous_master = HgChangesetId::from_str("a5ffa77602a066db7d5cfb9fb5823a0895717c5a")?;
        let commits_between = count_commits_between(ctx, &repo, previous_master, book).await?;

        // `- 1` because range_stream is inclusive
        assert_eq!(commits_between - 1, num_pushes);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_create_new_bookmark(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;
        // Bottom commit of the repo
        let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
        let p = repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(&ctx, root)
            .await?
            .ok_or_else(|| Error::msg("Root is missing"))?;
        let parents = vec![p];

        let bcs_id = CreateCommitContext::new(&ctx, &repo, parents)
            .add_file("file", "content")
            .commit()
            .await?;

        let hg_cs = repo.derive_hg_changeset(&ctx, bcs_id).await?;

        let book = BookmarkKey::new("newbook")?;
        do_pushrebase(&ctx, &repo, &Default::default(), &book, &hashset![hg_cs]).await?;
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_simultaneously_and_create_new(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;
        // Bottom commit of the repo
        let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
        let p = repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(&ctx, root)
            .await?
            .ok_or_else(|| Error::msg("Root is missing"))?;
        let parents = vec![p];

        let book = BookmarkKey::new("newbook")?;

        let num_pushes = 10;
        let mut futs = vec![];
        for i in 0..num_pushes {
            cloned!(ctx, repo, book);

            let hooks = [Box::new(SleepHook) as Box<dyn PushrebaseHook>];

            let f = format!("file{i}");
            let bcs_id = CreateCommitContext::new(&ctx, &repo, parents.clone())
                .add_file(f.as_str(), "content")
                .commit()
                .await?;

            let bcs = bcs_id.load(&ctx, repo.repo_blobstore()).await?;

            let fut = async move {
                do_pushrebase_bonsai(
                    &ctx,
                    &repo,
                    &Default::default(),
                    &book,
                    &hashset![bcs],
                    &hooks,
                )
                .await
            };

            futs.push(fut);
        }

        let res = try_join_all(futs).await?;
        let mut has_retry_num_bigger_1 = false;
        for r in res {
            if r.retry_num.0 > 1 {
                has_retry_num_bigger_1 = true;
            }
        }

        assert!(has_retry_num_bigger_1);

        let commits_between = count_commits_between(ctx, &repo, root, book).await?;
        // `- 1` because range_stream is inclusive
        assert_eq!(commits_between - 1, num_pushes);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_one_commit_with_bundle_id(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;
        // Bottom commit of the repo
        let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
        let p = repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(&ctx, root)
            .await?
            .ok_or_else(|| Error::msg("Root is missing"))?;
        let parents = vec![p];

        let bcs_id = CreateCommitContext::new(&ctx, &repo, parents)
            .add_file("file", "content")
            .commit()
            .await?;
        let hg_cs = repo.derive_hg_changeset(&ctx, bcs_id).await?;

        let book = master_bookmark();
        set_bookmark(
            ctx.clone(),
            &repo,
            &book,
            "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
        )
        .await?;

        do_pushrebase(&ctx, &repo, &Default::default(), &book, &hashset![hg_cs]).await?;

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn forbid_p2_root_rebases(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;

        let root = repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(
                &ctx,
                HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?,
            )
            .await?
            .ok_or_else(|| Error::msg("Root is missing"))?;

        let bcs_id_0 = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("merge_file", "merge content")
            .commit()
            .await?;
        let bcs_id_1 = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_0, root])
            .add_file("file", "content")
            .commit()
            .await?;
        let hgcss = hashset![
            repo.derive_hg_changeset(&ctx, bcs_id_0).await?,
            repo.derive_hg_changeset(&ctx, bcs_id_1).await?,
        ];

        let book = master_bookmark();
        set_bookmark(
            ctx.clone(),
            &repo,
            &book,
            "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
        )
        .await?;

        let config_forbid_p2 = PushrebaseFlags {
            forbid_p2_root_rebases: true,
            ..Default::default()
        };

        assert!(
            do_pushrebase(&ctx, &repo, &config_forbid_p2, &book, &hgcss)
                .await
                .is_err()
        );

        let config_allow_p2 = PushrebaseFlags {
            forbid_p2_root_rebases: false,
            ..Default::default()
        };

        do_pushrebase(&ctx, &repo, &config_allow_p2, &book, &hgcss).await?;

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_over_merge(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = test_repo_factory::build_empty(ctx.fb).await?;

        let p1 = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("p1", "some content")
            .commit()
            .await?;

        let p2 = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("p2", "some content")
            .commit()
            .await?;

        let merge = CreateCommitContext::new(&ctx, &repo, vec![p1, p2])
            .add_file("merge", "some content")
            .commit()
            .await?;

        let book = master_bookmark();

        let merge_hg_cs_id = repo.derive_hg_changeset(&ctx, merge).await?;

        set_bookmark(ctx.clone(), &repo, &book, &{
            // https://github.com/rust-lang/rust/pull/64856
            let r = format!("{merge_hg_cs_id}");
            r
        })
        .await?;

        // Modify a file touched in another branch - should fail
        push_and_verify(
            &ctx,
            &repo,
            p1,
            &book,
            btreemap! {"p2" => Some("some content")},
            false,
        )
        .await?;

        // Modify a file modified in th merge commit - should fail
        push_and_verify(
            &ctx,
            &repo,
            p1,
            &book,
            btreemap! {"merge" => Some("some content")},
            false,
        )
        .await?;

        // Any other files should succeed
        push_and_verify(
            &ctx,
            &repo,
            p1,
            &book,
            btreemap! {"p1" => Some("some content")},
            true,
        )
        .await?;

        push_and_verify(
            &ctx,
            &repo,
            p1,
            &book,
            btreemap! {"otherfile" => Some("some content")},
            true,
        )
        .await?;

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_over_merge_even(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = MergeEven::get_repo(fb).await;

        // 4dcf230cd2f20577cb3e88ba52b73b376a2b3f69 - is a merge commit,
        // 3cda5c78aa35f0f5b09780d971197b51cad4613a is one of the ancestors
        let root = repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(
                &ctx,
                HgChangesetId::from_str("3cda5c78aa35f0f5b09780d971197b51cad4613a")?,
            )
            .await?
            .ok_or_else(|| Error::msg("Root is missing"))?;

        // Modifies the same file "branch" - pushrebase should fail because of conflicts
        let bcs_id_should_fail = CreateCommitContext::new(&ctx, &repo, vec![root])
            .add_file("branch", "some content")
            .commit()
            .await?;

        let bcs_id_should_succeed = CreateCommitContext::new(&ctx, &repo, vec![root])
            .add_file("randomfile", "some content")
            .commit()
            .await?;

        let book = master_bookmark();

        let hgcss = hashset![repo.derive_hg_changeset(&ctx, bcs_id_should_fail).await?];

        let res = do_pushrebase(&ctx, &repo, &PushrebaseFlags::default(), &book, &hgcss).await;

        should_have_conflicts(res);
        let hgcss = hashset![
            repo.derive_hg_changeset(&ctx, bcs_id_should_succeed)
                .await?,
        ];

        do_pushrebase(&ctx, &repo, &PushrebaseFlags::default(), &book, &hgcss).await?;

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_of_branch_merge(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = test_repo_factory::build_empty(ctx.fb).await?;

        // Pushrebase two branch merges (bcs_id_first_merge and bcs_id_second_merge)
        // on top of master
        let bcs_id_base = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("base", "base")
            .commit()
            .await?;

        let bcs_id_p1 = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_base])
            .add_file("p1", "p1")
            .commit()
            .await?;

        let bcs_id_p2 = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_base])
            .add_file("p2", "p2")
            .commit()
            .await?;

        let bcs_id_first_merge = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_p1, bcs_id_p2])
            .add_file("merge", "merge")
            .commit()
            .await?;

        let bcs_id_second_merge =
            CreateCommitContext::new(&ctx, &repo, vec![bcs_id_first_merge, bcs_id_p2])
                .add_file("merge2", "merge")
                .commit()
                .await?;

        // Modify base file again
        let bcs_id_master = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_p1])
            .add_file("base", "base2")
            .commit()
            .await?;

        let hg_cs = repo.derive_hg_changeset(&ctx, bcs_id_master).await?;

        let book = master_bookmark();
        set_bookmark(ctx.clone(), &repo, &book, &{
            // https://github.com/rust-lang/rust/pull/64856
            let r = format!("{hg_cs}");
            r
        })
        .await?;

        let hgcss = hashset![
            repo.derive_hg_changeset(&ctx, bcs_id_first_merge).await?,
            repo.derive_hg_changeset(&ctx, bcs_id_second_merge).await?,
        ];

        do_pushrebase(&ctx, &repo, &PushrebaseFlags::default(), &book, &hgcss).await?;

        let new_master = get_bookmark_value(&ctx, &repo, &BookmarkKey::new("master")?)
            .await?
            .ok_or_else(|| Error::msg("master not set"))?;

        let master_hg = repo.derive_hg_changeset(&ctx, new_master).await?;

        ensure_content(
            &ctx,
            master_hg,
            &repo,
            btreemap! {
                    "base".to_string()=> "base2".to_string(),
                    "merge".to_string()=> "merge".to_string(),
                    "merge2".to_string()=> "merge".to_string(),
                    "p1".to_string()=> "p1".to_string(),
                    "p2".to_string()=> "p2".to_string(),
            },
        )
        .await?;

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_of_branch_merge_with_removal(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = test_repo_factory::build_empty(ctx.fb).await?;

        // Pushrebase two branch merges (bcs_id_first_merge and bcs_id_second_merge)
        // on top of master
        let bcs_id_base = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("base", "base")
            .commit()
            .await?;

        let bcs_id_p1 = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_base])
            .add_file("p1", "p1")
            .commit()
            .await?;

        let bcs_id_p2 = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_base])
            .add_file("p2", "p2")
            .commit()
            .await?;

        let bcs_id_merge = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_p1, bcs_id_p2])
            .add_file("merge", "merge")
            .commit()
            .await?;

        // Modify base file again
        let bcs_id_master = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_p1])
            .delete_file("base")
            .add_file("anotherfile", "anotherfile")
            .commit()
            .await?;

        let hg_cs = repo.derive_hg_changeset(&ctx, bcs_id_master).await?;

        let book = master_bookmark();
        set_bookmark(ctx.clone(), &repo, &book, &{
            // https://github.com/rust-lang/rust/pull/64856
            let r = format!("{hg_cs}");
            r
        })
        .await?;

        let hgcss = hashset![repo.derive_hg_changeset(&ctx, bcs_id_merge).await?,];

        do_pushrebase(&ctx, &repo, &PushrebaseFlags::default(), &book, &hgcss).await?;

        let new_master = get_bookmark_value(&ctx, &repo, &BookmarkKey::new("master")?)
            .await?
            .ok_or_else(|| Error::msg("master not set"))?;

        let master_hg = repo.derive_hg_changeset(&ctx, new_master).await?;

        ensure_content(
            &ctx,
            master_hg,
            &repo,
            btreemap! {
                    "anotherfile".to_string() => "anotherfile".to_string(),
                    "merge".to_string()=> "merge".to_string(),
                    "p1".to_string()=> "p1".to_string(),
                    "p2".to_string()=> "p2".to_string(),
            },
        )
        .await?;

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_of_branch_merge_with_rename(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = test_repo_factory::build_empty(ctx.fb).await?;

        // Pushrebase two branch merges (bcs_id_first_merge and bcs_id_second_merge)
        // on top of master
        let bcs_id_base = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("base", "base")
            .commit()
            .await?;

        let bcs_id_p1 = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_base])
            .add_file("p1", "p1")
            .commit()
            .await?;

        let bcs_id_p2 = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_base])
            .add_file("p2", "p2")
            .commit()
            .await?;

        let bcs_id_merge = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_p1, bcs_id_p2])
            .add_file("merge", "merge")
            .commit()
            .await?;

        // Remove base file
        let bcs_id_pre_pre_master = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_p1])
            .delete_file("base")
            .commit()
            .await?;

        // Move to base file
        let bcs_id_pre_master = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_pre_pre_master])
            .add_file_with_copy_info("base", "somecontent", (bcs_id_pre_pre_master, "p1"))
            .commit()
            .await?;

        let bcs_id_master = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_pre_master])
            .add_file("somefile", "somecontent")
            .commit()
            .await?;

        let hg_cs = repo.derive_hg_changeset(&ctx, bcs_id_master).await?;

        let book = master_bookmark();
        set_bookmark(ctx.clone(), &repo, &book, &{
            // https://github.com/rust-lang/rust/pull/64856
            let r = format!("{hg_cs}");
            r
        })
        .await?;

        let hgcss = hashset![repo.derive_hg_changeset(&ctx, bcs_id_merge).await?];

        do_pushrebase(&ctx, &repo, &PushrebaseFlags::default(), &book, &hgcss).await?;

        let new_master = get_bookmark_value(&ctx, &repo.clone(), &BookmarkKey::new("master")?)
            .await?
            .ok_or_else(|| Error::msg("master is not set"))?;

        let master_hg = repo.derive_hg_changeset(&ctx, new_master).await?;

        ensure_content(
            &ctx,
            master_hg,
            &repo,
            btreemap! {
                    "base".to_string() => "somecontent".to_string(),
                    "somefile".to_string() => "somecontent".to_string(),
                    "merge".to_string()=> "merge".to_string(),
                    "p1".to_string()=> "p1".to_string(),
                    "p2".to_string()=> "p2".to_string(),
            },
        )
        .await?;

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_pushrebase_new_repo_merge_no_new_file_changes(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;

        // First commit in the new repo
        let other_first_commit = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("otherrepofile", "otherrepocontent")
            .commit()
            .await?;

        let bcs_id = CreateCommitContext::new_root(&ctx, &repo)
            // Bottom commit of the main repo
            .add_parent("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")
            .add_parent(other_first_commit)
            .commit()
            .await?;

        let hg_cs = repo.derive_hg_changeset(&ctx, bcs_id).await?;

        let result = do_pushrebase(
            &ctx,
            &repo,
            &Default::default(),
            &master_bookmark(),
            &hashset![hg_cs],
        )
        .map_err(|err| format_err!("{err:?}"))
        .await?;

        let bcs = result.head.load(&ctx, repo.repo_blobstore()).await?;
        assert_eq!(bcs.file_changes().collect::<Vec<_>>(), vec![]);

        let master_hg = repo.derive_hg_changeset(&ctx, result.head).await?;

        ensure_content(
            &ctx,
            master_hg,
            &repo,
            btreemap! {
                    "1".to_string()=> "1\n".to_string(),
                    "2".to_string()=> "2\n".to_string(),
                    "3".to_string()=> "3\n".to_string(),
                    "4".to_string()=> "4\n".to_string(),
                    "5".to_string()=> "5\n".to_string(),
                    "6".to_string()=> "6\n".to_string(),
                    "7".to_string()=> "7\n".to_string(),
                    "8".to_string()=> "8\n".to_string(),
                    "9".to_string()=> "9\n".to_string(),
                    "10".to_string()=> "modified10\n".to_string(),

                    "files".to_string()=> "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n".to_string(),
                    "otherrepofile".to_string()=> "otherrepocontent".to_string(),
            },
        )
        .await?;

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_test_failpushrebase_extra(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;

        // Create one commit on top of latest commit in the linear repo
        let before_head_commit = "79a13814c5ce7330173ec04d279bf95ab3f652fb";
        let head_bcs_id = CreateCommitContext::new(&ctx, &repo, vec![before_head_commit])
            .add_file("file", "content")
            .add_extra(FAIL_PUSHREBASE_EXTRA.to_string(), vec![])
            .commit()
            .await?;

        bookmark(&ctx, &repo, "head").set_to(head_bcs_id).await?;

        let bcs_id = CreateCommitContext::new(&ctx, &repo, vec![before_head_commit])
            .add_file("file", "content2")
            .commit()
            .await?;

        let hg_cs = repo.derive_hg_changeset(&ctx, bcs_id).await?;

        let err = do_pushrebase(
            &ctx,
            &repo,
            &Default::default(),
            &BookmarkKey::new("head")?,
            &hashset![hg_cs],
        )
        .await;

        match err {
            Err(PushrebaseError::ForceFailPushrebase(_)) => {}
            _ => {
                return Err(format_err!(
                    "unexpected result: expected ForceFailPushrebase error, found {err:?}"
                ));
            }
        };

        // Now create the same commit on top of head commit - pushrebase should succeed
        let bcs_id = CreateCommitContext::new(&ctx, &repo, vec![head_bcs_id])
            .add_file("file", "content2")
            .commit()
            .await?;

        let hg_cs = repo.derive_hg_changeset(&ctx, bcs_id).await?;

        do_pushrebase(
            &ctx,
            &repo,
            &Default::default(),
            &BookmarkKey::new("head")?,
            &hashset![hg_cs],
        )
        .map_err(|err| format_err!("{err:?}"))
        .await?;

        Ok(())
    }

    async fn ensure_content(
        ctx: &CoreContext,
        hg_cs_id: HgChangesetId,
        repo: &impl Repo,
        expected: BTreeMap<String, String>,
    ) -> Result<(), Error> {
        let cs = hg_cs_id.load(ctx, repo.repo_blobstore()).await?;

        let entries = cs
            .manifestid()
            .list_all_entries(ctx.clone(), repo.repo_blobstore().clone())
            .try_collect::<Vec<_>>()
            .await?;

        let mut actual = BTreeMap::new();
        for (path, entry) in entries {
            match entry {
                Entry::Leaf((_, filenode_id)) => {
                    let store = repo.repo_blobstore();
                    let content_id = filenode_id.load(ctx, store).await?.content_id();
                    let content = filestore::fetch_concat(store, ctx, content_id).await?;

                    let s = String::from_utf8_lossy(content.as_ref()).into_owned();
                    actual.insert(
                        format!("{}", Option::<NonRootMPath>::from(path).unwrap()),
                        s,
                    );
                }
                Entry::Tree(_) => {}
            }
        }

        assert_eq!(expected, actual);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn batched_pushrebase_two_stacks(fb: FacebookInit) -> Result<(), Error> {
        #[derive(Copy, Clone)]
        struct Hook(RepositoryId);

        #[async_trait]
        impl PushrebaseHook for Hook {
            async fn in_critical_section(
                &self,
                _ctx: &CoreContext,
                _old_bookmark_value: Option<ChangesetId>,
            ) -> Result<Box<dyn PushrebaseCommitHook>, Error> {
                Ok(Box::new(*self) as Box<dyn PushrebaseCommitHook>)
            }
        }

        #[async_trait]
        impl PushrebaseCommitHook for Hook {
            fn post_rebase_changeset(
                &mut self,
                _bcs_old: ChangesetId,
                _bcs_new: &mut BonsaiChangesetMut,
            ) -> Result<(), Error> {
                Ok(())
            }

            async fn into_transaction_hook(
                self: Box<Self>,
                _ctx: &CoreContext,
                changesets: &RebasedChangesets,
            ) -> Result<Box<dyn PushrebaseTransactionHook>, Error> {
                Ok(Box::new(TransactionHook(self.0, changesets.len()))
                    as Box<dyn PushrebaseTransactionHook>)
            }
        }

        struct TransactionHook(RepositoryId, usize);

        #[async_trait]
        impl PushrebaseTransactionHook for TransactionHook {
            async fn populate_transaction(
                &self,
                ctx: &CoreContext,
                txn: Transaction,
            ) -> Result<Transaction, BookmarkTransactionError> {
                let ret = SqlMutableCounters::set_counter_on_txn(
                    ctx,
                    self.0,
                    "batched_hook_changesets",
                    self.1 as i64,
                    None,
                    txn,
                )
                .await?;

                match ret {
                    TransactionResult::Succeeded(txn) => Ok(txn),
                    TransactionResult::Failed => Err(Error::msg("Did not update").into()),
                }
            }
        }

        let ctx = CoreContext::test_mock(fb);
        let factory = TestRepoFactory::new(fb)?;
        let repo: PushrebaseTestRepo = factory.build().await?;
        let (commits, _dag) = Linear::init_repo(fb, &repo).await?;

        let master_cs = commits["K"];
        let root_bcs_id = commits["A"];
        let root_bcs_id_b = commits["B"];

        // Stack A: adds "fileA"
        let bcs_id_a = CreateCommitContext::new(&ctx, &repo, vec![root_bcs_id])
            .add_file("fileA", "content_a")
            .commit()
            .await?;
        let bcs_a = bcs_id_a.load(&ctx, repo.repo_blobstore()).await?;

        // Stack B: adds "fileB"
        let bcs_id_b = CreateCommitContext::new(&ctx, &repo, vec![root_bcs_id_b])
            .add_file("fileB", "content_b")
            .commit()
            .await?;
        let bcs_b = bcs_id_b.load(&ctx, repo.repo_blobstore()).await?;

        // Index both stacks
        let bookmark = master_bookmark();
        let config = PushrebaseFlags::default();
        let PushrebaseRequestIndex {
            changed_files: cf_a,
            changesets: changesets_a,
            head: head_a,
            root: root_a,
        } = index_pushrebase_request(&ctx, &repo, &config, &bookmark, &hashset![bcs_a]).await?;
        let PushrebaseRequestIndex {
            changed_files: cf_b,
            changesets: changesets_b,
            head: head_b,
            root: root_b,
        } = index_pushrebase_request(&ctx, &repo, &config, &bookmark, &hashset![bcs_b]).await?;

        // Build PushrebaseRequests with oneshot channels
        let (tx_a, rx_a) = oneshot::channel();
        let (tx_b, rx_b) = oneshot::channel();

        // Only the first request needs hooks (batched pushrebase uses hooks from requests[0])
        let hook_a: Box<dyn PushrebaseHook> = Box::new(Hook(repo.repo_identity().id()));
        let hook_b: Box<dyn PushrebaseHook> = Box::new(Hook(repo.repo_identity().id()));

        let req_a = PushrebaseRequest {
            changed_files: cf_a,
            changesets: changesets_a,
            head: head_a,
            root: root_a,
            conflict_check_base: root_a,
            carried_merge_file_info: vec![],
            retry_num: PushrebaseRetryNum(0),
            hooks: vec![hook_a],
            response_tx: tx_a,
        };

        let req_b = PushrebaseRequest {
            changed_files: cf_b,
            changesets: changesets_b,
            head: head_b,
            root: root_b,
            conflict_check_base: root_b,
            carried_merge_file_info: vec![],
            retry_num: PushrebaseRetryNum(0),
            hooks: vec![hook_b],
            response_tx: tx_b,
        };

        // Call do_batched_pushrebase
        let requeued =
            do_batched_pushrebase(&ctx, &repo, &config, &bookmark, vec![req_a, req_b]).await;

        // No CAS failures expected
        assert!(requeued.is_empty(), "Expected no re-queued requests");

        // Both receivers should get Ok outcomes
        let outcome_a = rx_a.await.unwrap().map_err(|e| format_err!("{e:?}"))?;
        let outcome_b = rx_b.await.unwrap().map_err(|e| format_err!("{e:?}"))?;

        // outcome_a sees the original bookmark value (it was rebased first)
        assert_eq!(outcome_a.old_bookmark_value, Some(master_cs));
        // outcome_b sees the head after A was rebased (running_head before B's rebase)
        assert_eq!(outcome_b.old_bookmark_value, Some(outcome_a.head));

        // Both should have one rebased changeset
        assert_eq!(outcome_a.rebased_changesets.len(), 1);
        assert_eq!(outcome_b.rebased_changesets.len(), 1);

        // Pushrebase distance should be 10 (A to K in the Linear fixture)
        assert_eq!(outcome_a.pushrebase_distance.0, 10);
        assert_eq!(outcome_b.pushrebase_distance.0, 9);

        // The final bookmark should point to outcome_b's head (second stack lands on top of first)
        let new_master = resolve_cs_id(&ctx, &repo, "master").await?;
        assert_eq!(new_master, outcome_b.head);

        // Verify the hook fired: the transaction hook should have written the total
        // number of rebased changesets (2, one per stack) to the mutable counter
        assert_eq!(
            repo.mutable_counters()
                .get_counter(&ctx, "batched_hook_changesets")
                .await?,
            Some(2),
        );

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn batched_pushrebase_one_conflict(fb: FacebookInit) -> Result<(), Error> {
        init_just_knobs_for_test();
        let ctx = CoreContext::test_mock(fb);
        let (repo, commits, _dag): (PushrebaseTestRepo, _, _) = Linear::get_repo_and_dag(fb).await;

        let root_bcs_id = commits["A"];

        // Stack A: adds a new file (no conflict)
        let bcs_id_a = CreateCommitContext::new(&ctx, &repo, vec![root_bcs_id])
            .add_file("new_file", "content")
            .commit()
            .await?;
        let bcs_a = bcs_id_a.load(&ctx, repo.repo_blobstore()).await?;

        // Stack B: modifies "files" which is also modified by commits B-K (conflict)
        let bcs_id_b = CreateCommitContext::new(&ctx, &repo, vec![root_bcs_id])
            .add_file("files", "conflicting content")
            .commit()
            .await?;
        let bcs_b = bcs_id_b.load(&ctx, repo.repo_blobstore()).await?;

        // Index both stacks
        let bookmark = master_bookmark();
        let config = PushrebaseFlags::default();
        let PushrebaseRequestIndex {
            changed_files: cf_a,
            changesets: changesets_a,
            head: head_a,
            root: root_a,
        } = index_pushrebase_request(&ctx, &repo, &config, &bookmark, &hashset![bcs_a]).await?;
        let PushrebaseRequestIndex {
            changed_files: cf_b,
            changesets: changesets_b,
            head: head_b,
            root: root_b,
        } = index_pushrebase_request(&ctx, &repo, &config, &bookmark, &hashset![bcs_b]).await?;

        let (tx_a, rx_a) = oneshot::channel();
        let (tx_b, rx_b) = oneshot::channel();

        let req_a = PushrebaseRequest {
            changed_files: cf_a,
            changesets: changesets_a,
            head: head_a,
            root: root_a,
            conflict_check_base: root_a,
            carried_merge_file_info: vec![],
            retry_num: PushrebaseRetryNum(0),
            hooks: vec![],
            response_tx: tx_a,
        };

        let req_b = PushrebaseRequest {
            changed_files: cf_b,
            changesets: changesets_b,
            head: head_b,
            root: root_b,
            conflict_check_base: root_b,
            carried_merge_file_info: vec![],
            retry_num: PushrebaseRetryNum(0),
            hooks: vec![],
            response_tx: tx_b,
        };

        let requeued =
            do_batched_pushrebase(&ctx, &repo, &config, &bookmark, vec![req_a, req_b]).await;
        assert!(requeued.is_empty(), "Expected no re-queued requests");

        // Stack A should succeed
        let outcome_a = rx_a.await.unwrap().map_err(|e| format_err!("{e:?}"))?;
        assert_eq!(outcome_a.rebased_changesets.len(), 1);

        // Stack B should fail with conflicts
        let result_b = rx_b.await.unwrap();
        assert!(result_b.is_err(), "Expected stack B to fail with conflicts");
        match result_b.unwrap_err().inner() {
            PushrebaseError::Conflicts(_) => {}
            other => panic!("Expected Conflicts error, got: {other:?}"),
        }

        // Bookmark should still be updated (stack A succeeded)
        let new_master = resolve_cs_id(&ctx, &repo, "master").await?;
        assert_eq!(new_master, outcome_a.head);

        Ok(())
    }

    /// Verify that a rebase failure in one request cannot corrupt hook
    /// state that flows into the bookmark transaction for other requests.
    ///
    /// Setup: three requests [A, B, C] in one batch.  The shared commit
    /// hook records an assignment for every changeset it sees (mimicking
    /// globalrev), then fails on request B's changeset — *after* recording
    /// the phantom assignment.  With the bug, the loop would continue to
    /// C, and `into_transaction_hook` would fire with 3 recorded
    /// assignments but only 2 entries in `all_rebased_changesets`.  A hook
    /// without its own count-check (or one that uses the recorded count
    /// for sequencing) would write corrupt data to the transaction.
    ///
    /// The fix aborts the batch on the first `create_rebased_changesets`
    /// failure, so `into_transaction_hook` is never reached and no data
    /// is written.
    #[mononoke::fbinit_test]
    async fn batched_pushrebase_rebase_failure_prevents_corrupt_hook_data(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        #[derive(Clone)]
        struct TrackingHook(RepositoryId);

        /// Commit hook that records one assignment per changeset it sees,
        /// then fails on the Nth call.  The assignment is recorded
        /// *before* the error check — this is the state contamination.
        struct TrackingCommitHook {
            repo_id: RepositoryId,
            assignments: usize,
            fail_on_call: usize,
        }

        #[async_trait]
        impl PushrebaseHook for TrackingHook {
            async fn in_critical_section(
                &self,
                _ctx: &CoreContext,
                _old_bookmark_value: Option<ChangesetId>,
            ) -> Result<Box<dyn PushrebaseCommitHook>, Error> {
                Ok(Box::new(TrackingCommitHook {
                    repo_id: self.0,
                    assignments: 0,
                    // Requests are processed in order: A(call 1), B(call 2), C(call 3).
                    // Fail on call 2 (request B) after recording its assignment.
                    fail_on_call: 2,
                }))
            }
        }

        #[async_trait]
        impl PushrebaseCommitHook for TrackingCommitHook {
            fn post_rebase_changeset(
                &mut self,
                _bcs_old: ChangesetId,
                _bcs_new: &mut BonsaiChangesetMut,
            ) -> Result<(), Error> {
                // Record the assignment FIRST (mirrors globalrev hook which
                // calls set_on_changeset + insert + increment before returning).
                self.assignments += 1;
                if self.assignments == self.fail_on_call {
                    return Err(anyhow::anyhow!("simulated rebase failure"));
                }
                Ok(())
            }

            async fn into_transaction_hook(
                self: Box<Self>,
                ctx: &CoreContext,
                _changesets: &RebasedChangesets,
            ) -> Result<Box<dyn PushrebaseTransactionHook>, Error> {
                // Write the number of recorded assignments to a mutable
                // counter.  If hook state was contaminated by the failed
                // request, this value is WRONG — it includes a phantom
                // assignment for a changeset that was never committed.
                Ok(Box::new(WriteCountHook {
                    repo_id: self.repo_id,
                    count: self.assignments,
                    ctx: ctx.clone(),
                }))
            }
        }

        struct WriteCountHook {
            repo_id: RepositoryId,
            count: usize,
            ctx: CoreContext,
        }

        #[async_trait]
        impl PushrebaseTransactionHook for WriteCountHook {
            async fn populate_transaction(
                &self,
                _ctx: &CoreContext,
                txn: Transaction,
            ) -> Result<Transaction, BookmarkTransactionError> {
                let ret = SqlMutableCounters::set_counter_on_txn(
                    &self.ctx,
                    self.repo_id,
                    "hook_assignments",
                    self.count as i64,
                    None,
                    txn,
                )
                .await?;
                match ret {
                    TransactionResult::Succeeded(txn) => Ok(txn),
                    TransactionResult::Failed => Err(Error::msg("counter write failed").into()),
                }
            }
        }

        let ctx = CoreContext::test_mock(fb);
        let factory = TestRepoFactory::new(fb)?;
        let repo: PushrebaseTestRepo = factory.build().await?;
        let (commits, _dag) = Linear::init_repo(fb, &repo).await?;

        let root = commits["A"];
        let bookmark = master_bookmark();
        let config = PushrebaseFlags::default();

        // Three non-conflicting stacks, each with one changeset.
        let mut requests = Vec::new();
        let mut receivers = Vec::new();
        for name in ["fileA", "fileB", "fileC"] {
            let bcs_id = CreateCommitContext::new(&ctx, &repo, vec![root])
                .add_file(name, "content")
                .commit()
                .await?;
            let bcs = bcs_id.load(&ctx, repo.repo_blobstore()).await?;
            let idx =
                index_pushrebase_request(&ctx, &repo, &config, &bookmark, &hashset![bcs]).await?;
            let (tx, rx) = oneshot::channel();
            requests.push(PushrebaseRequest {
                changed_files: idx.changed_files,
                changesets: idx.changesets,
                head: idx.head,
                root: idx.root,
                conflict_check_base: idx.root,
                carried_merge_file_info: vec![],
                retry_num: PushrebaseRetryNum(0),
                hooks: vec![Box::new(TrackingHook(repo.repo_identity().id()))],
                response_tx: tx,
            });
            receivers.push(rx);
        }

        let requeued = do_batched_pushrebase(&ctx, &repo, &config, &bookmark, requests).await;

        // Request B (index 1) should have received the hook error.
        let result_b = receivers.remove(1).await.unwrap();
        assert!(result_b.is_err(), "Request B should have failed");

        // Requests A and C should be requeued — NOT resolved with
        // corrupt hook data flowing into the transaction.
        assert_eq!(requeued.len(), 2, "Requests A and C should be requeued");

        // The transaction hook must NOT have fired.  If it did, it would
        // have written a count of 3 (including the phantom assignment
        // from failed request B) — that's data corruption.
        assert_eq!(
            repo.mutable_counters()
                .get_counter(&ctx, "hook_assignments")
                .await?,
            None,
            "into_transaction_hook should not have been reached",
        );

        Ok(())
    }

    fn should_have_conflicts(res: Result<PushrebaseOutcome, PushrebaseError>) {
        match res {
            Err(err) => match err {
                PushrebaseError::Conflicts(_) => {}
                _ => {
                    panic!("pushrebase should have had conflicts");
                }
            },
            Ok(_) => {
                panic!("pushrebase should have failed");
            }
        }
    }

    fn init_just_knobs_for_merge_test() {
        override_just_knobs(JustKnobsInMemory::new(hashmap! {
            "scm/mononoke:pushrebase_dry_run_merge_resolution".to_string() => KnobVal::Bool(false),
            "scm/mononoke:pushrebase_enable_merge_resolution".to_string() => KnobVal::Bool(true),
            "scm/mononoke:pushrebase_merge_resolution_derive_fsnodes".to_string() => KnobVal::Bool(true),
        }));
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_merge_resolution_clean(fb: FacebookInit) -> Result<(), Error> {
        // Test: server and client modify different parts of the same file.
        // With merge resolution enabled, pushrebase should succeed and
        // the resulting file should contain both modifications.
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

        // Create a base commit with a multi-line file
        let base_content = "line1\nline2\nline3\nline4\nline5\n";
        let base = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file.txt", base_content)
            .commit()
            .await?;

        // Server-side commit: modify the first line
        let server_content = "modified_line1\nline2\nline3\nline4\nline5\n";
        let server = CreateCommitContext::new(&ctx, &repo, vec![base])
            .add_file("file.txt", server_content)
            .commit()
            .await?;

        // Set bookmark to the server commit
        let book = BookmarkKey::new("master")?;
        let hg_server = repo.derive_hg_changeset(&ctx, server).await?;
        set_bookmark(ctx.clone(), &repo, &book, &format!("{hg_server}")).await?;

        // Client-side commit (based on base, not server): modify the last line
        let client_content = "line1\nline2\nline3\nline4\nmodified_line5\n";
        let client = CreateCommitContext::new(&ctx, &repo, vec![base])
            .add_file("file.txt", client_content)
            .commit()
            .await?;

        let client_bcs = client.load(&ctx, repo.repo_blobstore()).await?;

        // Enable merge resolution
        init_just_knobs_for_merge_test();

        let result = do_pushrebase_bonsai(
            &ctx,
            &repo,
            &Default::default(),
            &book,
            &hashset![client_bcs],
            &[],
        )
        .await?;

        // Verify the merged content has both modifications
        let result_hg = repo.derive_hg_changeset(&ctx, result.head).await?;
        let expected_content = "modified_line1\nline2\nline3\nline4\nmodified_line5\n";
        ensure_content(
            &ctx,
            result_hg,
            &repo,
            btreemap! {
                "file.txt".to_string() => expected_content.to_string(),
            },
        )
        .await?;

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_merge_resolution_conflict(fb: FacebookInit) -> Result<(), Error> {
        // Test: server and client modify the SAME line of a file.
        // Even with merge resolution enabled, pushrebase should fail
        // because the merge has a true content-level conflict.
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

        // Create a base commit with a multi-line file
        let base_content = "line1\nline2\nline3\n";
        let base = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file.txt", base_content)
            .commit()
            .await?;

        // Server-side commit: modify line 2
        let server_content = "line1\nserver_modified\nline3\n";
        let server = CreateCommitContext::new(&ctx, &repo, vec![base])
            .add_file("file.txt", server_content)
            .commit()
            .await?;

        // Set bookmark to the server commit
        let book = BookmarkKey::new("master")?;
        let hg_server = repo.derive_hg_changeset(&ctx, server).await?;
        set_bookmark(ctx.clone(), &repo, &book, &format!("{hg_server}")).await?;

        // Client-side commit (based on base): also modify line 2 differently
        let client_content = "line1\nclient_modified\nline3\n";
        let client = CreateCommitContext::new(&ctx, &repo, vec![base])
            .add_file("file.txt", client_content)
            .commit()
            .await?;

        let client_bcs = client.load(&ctx, repo.repo_blobstore()).await?;

        // Enable merge resolution
        init_just_knobs_for_merge_test();

        let result = do_pushrebase_bonsai(
            &ctx,
            &repo,
            &Default::default(),
            &book,
            &hashset![client_bcs],
            &[],
        )
        .await;

        // Should fail with conflicts because of overlapping edits
        should_have_conflicts(result);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_merge_resolution_carry_forward_on_retry(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        // Test: On CAS retry, attempt 2 uses a narrow range (S1→S2) that
        // does NOT contain the original conflict. The carried MergedFileInfo
        // from attempt 1 is used via reconciliation.
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

        let base_content = "\
line 1
line 2
line 3
line 4
line 5
line 6
line 7
line 8
";
        let base = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file.txt", base_content)
            .commit()
            .await?;

        // Server commit S1: adds "line 2.1" between line 2 and line 3
        let s1_content = "\
line 1
line 2
line 2.1
line 3
line 4
line 5
line 6
line 7
line 8
";
        let s1 = CreateCommitContext::new(&ctx, &repo, vec![base])
            .add_file("file.txt", s1_content)
            .commit()
            .await?;

        // Server commit S2: unrelated change (different file) — simulates
        // another push that moves the bookmark after S1
        let s2 = CreateCommitContext::new(&ctx, &repo, vec![s1])
            .add_file("unrelated.txt", "unrelated change\n")
            .commit()
            .await?;

        // Set bookmark to S2 (after both server commits)
        let book = BookmarkKey::new("master")?;
        let hg_s2 = repo.derive_hg_changeset(&ctx, s2).await?;
        set_bookmark(ctx.clone(), &repo, &book, &format!("{hg_s2}")).await?;

        // Client commit: adds "line 6.1" between line 6 and line 7
        // (based on base, NOT on server commits)
        let client_content = "\
line 1
line 2
line 3
line 4
line 5
line 6
line 6.1
line 7
line 8
";
        let client = CreateCommitContext::new(&ctx, &repo, vec![base])
            .add_file("file.txt", client_content)
            .commit()
            .await?;

        let client_bcs = client.load(&ctx, repo.repo_blobstore()).await?;

        init_just_knobs_for_merge_test();

        let client_cf = find_changed_files(&ctx, &repo, base, client).await?;

        // --- Attempt 1: full range base → S1 (first bookmark position) ---
        let result1 = check_pushrebase_conflicts(
            &ctx,
            &repo,
            &Default::default(),
            base,
            base,
            s1,
            std::slice::from_ref(&client_bcs),
            &client_cf,
        )
        .await?;
        assert!(
            result1.merged_file_overrides.is_some(),
            "Attempt 1 should detect the conflict and produce merge overrides"
        );

        // --- Attempt 2: narrow range S1 → S2 (only the delta after CAS fail) ---
        // S2 only changes unrelated.txt, so no conflict with client's file.txt
        let result2 = check_pushrebase_conflicts(
            &ctx,
            &repo,
            &Default::default(),
            base,
            s1,
            s2,
            std::slice::from_ref(&client_bcs),
            &client_cf,
        )
        .await?;
        assert!(
            result2.merged_file_overrides.is_none(),
            "Narrow range S1→S2 should have no conflicts (unrelated.txt only)"
        );

        // Simulate carry-forward: attempt 1 produces overrides, CAS fails,
        // attempt 2 sees no new conflicts but carried info is used.
        let carried = result1.merged_file_overrides.clone().unwrap();
        let reconciled = match result2.merged_file_overrides {
            Some(ref delta) => reconcile_merge_file_info(&carried, delta),
            None => carried,
        };

        // Rebase with the reconciled (carried) overrides
        let (new_head, _, rebased_bonsais) = create_rebased_changesets(
            &ctx,
            &repo,
            &Default::default(),
            base,
            client,
            s2,
            &mut [],
            Some(reconciled),
        )
        .await?;
        changesets_creation::save_changesets(&ctx, &repo, rebased_bonsais).await?;

        // Check the file content at the rebased head
        let result_hg = repo.derive_hg_changeset(&ctx, new_head).await?;
        let result_cs = result_hg.load(&ctx, repo.repo_blobstore()).await?;
        let manifest = result_cs.manifestid();
        let file_path = NonRootMPath::new("file.txt")?;
        let file_entry = manifest
            .find_entry(ctx.clone(), repo.repo_blobstore().clone(), file_path.into())
            .await?
            .expect("file.txt should exist");

        let file_content = match file_entry {
            Entry::Leaf((_, filenode_id)) => {
                let content_id = filenode_id
                    .load(&ctx, repo.repo_blobstore())
                    .await?
                    .content_id();
                let bytes =
                    filestore::fetch_concat(repo.repo_blobstore(), &ctx, content_id).await?;
                String::from_utf8(bytes.to_vec())?
            }
            _ => panic!("file.txt should be a file"),
        };

        // Both changes are preserved via carry-forward
        assert!(
            file_content.contains("line 6.1"),
            "rebased commit should have client's line 6.1"
        );
        assert!(
            file_content.contains("line 2.1"),
            "rebased commit should have server's line 2.1. Actual:\n{file_content}",
        );

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_merge_resolution_carry_forward_with_new_server_changes(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        // Test: When the same file is changed in both base→S1 and S1→S2,
        // the carry-forward reconciliation updates server_content_id from
        // the delta so the rebase uses the latest server content.
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

        let base = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("a.txt", "a1\na2\na3\na4\na5\na6\na7\na8\n")
            .add_file("b.txt", "b1\nb2\nb3\nb4\nb5\nb6\nb7\nb8\n")
            .commit()
            .await?;

        // Server commit S1: modifies a.txt line 1 (conflict with client)
        let s1 = CreateCommitContext::new(&ctx, &repo, vec![base])
            .add_file("a.txt", "SERVER_a1\na2\na3\na4\na5\na6\na7\na8\n")
            .commit()
            .await?;

        // Server commit S2: modifies b.txt line 1 (conflict with client)
        let s2 = CreateCommitContext::new(&ctx, &repo, vec![s1])
            .add_file("b.txt", "SERVER_b1\nb2\nb3\nb4\nb5\nb6\nb7\nb8\n")
            .commit()
            .await?;

        let book = BookmarkKey::new("master")?;
        let hg_s2 = repo.derive_hg_changeset(&ctx, s2).await?;
        set_bookmark(ctx.clone(), &repo, &book, &format!("{hg_s2}")).await?;

        // Client modifies both files at the END (non-overlapping with server)
        let client = CreateCommitContext::new(&ctx, &repo, vec![base])
            .add_file("a.txt", "a1\na2\na3\na4\na5\na6\na7\nCLIENT_a8\n")
            .add_file("b.txt", "b1\nb2\nb3\nb4\nb5\nb6\nb7\nCLIENT_b8\n")
            .commit()
            .await?;

        let client_bcs = client.load(&ctx, repo.repo_blobstore()).await?;

        init_just_knobs_for_merge_test();

        let client_cf = find_changed_files(&ctx, &repo, base, client).await?;

        // --- Attempt 1: base → S1 (only a.txt conflict detected) ---
        let result1 = check_pushrebase_conflicts(
            &ctx,
            &repo,
            &Default::default(),
            base,
            base,
            s1,
            std::slice::from_ref(&client_bcs),
            &client_cf,
        )
        .await?;
        assert!(
            result1.merged_file_overrides.is_some(),
            "Attempt 1 should resolve a.txt conflict"
        );
        let carried = result1.merged_file_overrides.unwrap();
        assert_eq!(carried.len(), 1, "Only a.txt should be in carried info");

        // --- Attempt 2: narrow range S1 → S2 (b.txt conflict detected) ---
        let result2 = check_pushrebase_conflicts(
            &ctx,
            &repo,
            &Default::default(),
            base,
            s1,
            s2,
            std::slice::from_ref(&client_bcs),
            &client_cf,
        )
        .await?;
        assert!(
            result2.merged_file_overrides.is_some(),
            "Attempt 2 should resolve b.txt conflict in narrow range"
        );
        let delta = result2.merged_file_overrides.unwrap();
        assert_eq!(delta.len(), 1, "Only b.txt should be in delta info");

        // Reconcile: carried has a.txt, delta has b.txt → union of both
        let reconciled = reconcile_merge_file_info(&carried, &delta);
        assert_eq!(
            reconciled.len(),
            2,
            "Reconciled should have both a.txt and b.txt"
        );

        // Rebase with reconciled overrides
        let (new_head, _, rebased_bonsais) = create_rebased_changesets(
            &ctx,
            &repo,
            &Default::default(),
            base,
            client,
            s2,
            &mut [],
            Some(reconciled),
        )
        .await?;
        changesets_creation::save_changesets(&ctx, &repo, rebased_bonsais).await?;

        // Verify both files have merged content
        let result_hg = repo.derive_hg_changeset(&ctx, new_head).await?;
        ensure_content(
            &ctx,
            result_hg,
            &repo,
            btreemap! {
                "a.txt".to_string() => "SERVER_a1\na2\na3\na4\na5\na6\na7\nCLIENT_a8\n".to_string(),
                "b.txt".to_string() => "SERVER_b1\nb2\nb3\nb4\nb5\nb6\nb7\nCLIENT_b8\n".to_string(),
            },
        )
        .await?;

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_merge_resolution_stack_non_head_conflict(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        // Regression test: 2-commit stack where the FIRST commit (not HEAD)
        // touches a conflicting file. The merge override must be applied to
        // that first commit, not HEAD; otherwise the first commit keeps stale
        // content that reverts the server's changes.
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

        let base_content = "\
line 1
line 2
line 3
line 4
line 5
line 6
line 7
line 8
";
        let base = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file.txt", base_content)
            .add_file("other.txt", "other\n")
            .commit()
            .await?;

        // Server adds "line 2.1" between line 2 and line 3 (top region)
        let server_content = "\
line 1
line 2
line 2.1
line 3
line 4
line 5
line 6
line 7
line 8
";
        let server = CreateCommitContext::new(&ctx, &repo, vec![base])
            .add_file("file.txt", server_content)
            .commit()
            .await?;

        let book = BookmarkKey::new("master")?;
        let hg_server = repo.derive_hg_changeset(&ctx, server).await?;
        set_bookmark(ctx.clone(), &repo, &book, &format!("{hg_server}")).await?;

        // Client commit 1: adds "line 6.1" between line 6 and line 7 (bottom region)
        let client_content_1 = "\
line 1
line 2
line 3
line 4
line 5
line 6
line 6.1
line 7
line 8
";
        let client_1 = CreateCommitContext::new(&ctx, &repo, vec![base])
            .add_file("file.txt", client_content_1)
            .commit()
            .await?;

        // Client commit 2 (HEAD): only touches other.txt, NOT file.txt
        let client_2 = CreateCommitContext::new(&ctx, &repo, vec![client_1])
            .add_file("other.txt", "modified other\n")
            .commit()
            .await?;

        let client_bcs_1 = client_1.load(&ctx, repo.repo_blobstore()).await?;
        let client_bcs_2 = client_2.load(&ctx, repo.repo_blobstore()).await?;

        init_just_knobs_for_merge_test();

        let result = do_pushrebase_bonsai(
            &ctx,
            &repo,
            &Default::default(),
            &book,
            &hashset![client_bcs_1.clone(), client_bcs_2.clone()],
            &[],
        )
        .await?;

        let expected_merged = "\
line 1
line 2
line 2.1
line 3
line 4
line 5
line 6
line 6.1
line 7
line 8
";

        // HEAD has the correct merged content
        let result_hg = repo.derive_hg_changeset(&ctx, result.head).await?;
        ensure_content(
            &ctx,
            result_hg,
            &repo,
            btreemap! {
                "file.txt".to_string() => expected_merged.to_string(),
                "other.txt".to_string() => "modified other\n".to_string(),
            },
        )
        .await?;

        // Read file.txt from the FIRST rebased commit
        let rebased_1 = result
            .rebased_changesets
            .iter()
            .find(|pair| pair.id_old == client_1)
            .map(|pair| pair.id_new)
            .expect("first commit should be in rebased set");

        let rebased_1_hg = repo.derive_hg_changeset(&ctx, rebased_1).await?;
        let rebased_1_cs = rebased_1_hg.load(&ctx, repo.repo_blobstore()).await?;
        let rebased_1_manifest = rebased_1_cs.manifestid();
        let file_path = NonRootMPath::new("file.txt")?;
        let file_entry = rebased_1_manifest
            .find_entry(ctx.clone(), repo.repo_blobstore().clone(), file_path.into())
            .await?
            .expect("file.txt should exist in first rebased commit");

        let file_content = match file_entry {
            Entry::Leaf((_, filenode_id)) => {
                let content_id = filenode_id
                    .load(&ctx, repo.repo_blobstore())
                    .await?
                    .content_id();
                let bytes =
                    filestore::fetch_concat(repo.repo_blobstore(), &ctx, content_id).await?;
                String::from_utf8(bytes.to_vec())?
            }
            _ => panic!("file.txt should be a file"),
        };

        assert!(
            file_content.contains("line 6.1"),
            "first rebased commit should have line 6.1 (client's change)"
        );

        // FIX: With cascading merge, the first rebased commit now has the
        // server's "line 2.1" because the merge is applied per-commit
        // during the rebase, not just to HEAD.
        assert!(
            file_content.contains("line 2.1"),
            "first rebased commit should have server's 'line 2.1' \
             (cascading merge applied per-commit). Actual:\n{file_content}",
        );

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_stack_non_head_conflict_without_merge_resolution(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        // Same scenario but merge resolution DISABLED: pushrebase should fail.
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

        let base_content = "\
line 1
line 2
line 3
line 4
line 5
";
        let base = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file.txt", base_content)
            .add_file("other.txt", "other\n")
            .commit()
            .await?;

        let server_content = "\
line 1
line 2
line 2.1
line 3
line 4
line 5
";
        let server = CreateCommitContext::new(&ctx, &repo, vec![base])
            .add_file("file.txt", server_content)
            .commit()
            .await?;

        let book = BookmarkKey::new("master")?;
        let hg_server = repo.derive_hg_changeset(&ctx, server).await?;
        set_bookmark(ctx.clone(), &repo, &book, &format!("{hg_server}")).await?;

        let client_content = "\
line 1
line 2
line 3
line 4
line 5
line 5.1
";
        let client_1 = CreateCommitContext::new(&ctx, &repo, vec![base])
            .add_file("file.txt", client_content)
            .commit()
            .await?;
        let client_2 = CreateCommitContext::new(&ctx, &repo, vec![client_1])
            .add_file("other.txt", "modified\n")
            .commit()
            .await?;

        let client_bcs_1 = client_1.load(&ctx, repo.repo_blobstore()).await?;
        let client_bcs_2 = client_2.load(&ctx, repo.repo_blobstore()).await?;

        init_just_knobs_for_test();

        let result = do_pushrebase_bonsai(
            &ctx,
            &repo,
            &Default::default(),
            &book,
            &hashset![client_bcs_1, client_bcs_2],
            &[],
        )
        .await;

        should_have_conflicts(result);

        Ok(())
    }

    #[mononoke::test]
    fn reconcile_merge_file_info_basic() {
        use mononoke_types::hash::Blake2;

        let id_a = ContentId::new(Blake2::from_byte_array([1; 32]));
        let id_b = ContentId::new(Blake2::from_byte_array([2; 32]));
        let id_c = ContentId::new(Blake2::from_byte_array([3; 32]));
        let id_d = ContentId::new(Blake2::from_byte_array([4; 32]));

        let make_info = |path: &str, base: ContentId, server: ContentId| -> MergedFileInfo {
            MergedFileInfo {
                path: NonRootMPath::new(path).unwrap(),
                base_content_id: base,
                server_content_id: server,
                file_type: FileType::Regular,
            }
        };

        // Test 1: empty carried + non-empty delta returns delta
        let delta = vec![make_info("f1", id_a, id_b)];
        let result = reconcile_merge_file_info(&[], &delta);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].path, NonRootMPath::new("f1").unwrap());
        assert_eq!(result[0].server_content_id, id_b);

        // Test 2: non-empty carried + empty delta returns carried
        let carried = vec![make_info("f1", id_a, id_b)];
        let result = reconcile_merge_file_info(&carried, &[]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].server_content_id, id_b);

        // Test 3: overlapping path updates server_content_id from delta
        let carried = vec![make_info("f1", id_a, id_b)];
        let delta = vec![make_info("f1", id_a, id_c)];
        let result = reconcile_merge_file_info(&carried, &delta);
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0].server_content_id, id_c,
            "server_content_id should be updated from delta"
        );
        assert_eq!(
            result[0].base_content_id, id_a,
            "base_content_id should remain from carried"
        );

        // Test 4: non-overlapping paths produce union
        let carried = vec![make_info("f1", id_a, id_b)];
        let delta = vec![make_info("f2", id_c, id_d)];
        let result = reconcile_merge_file_info(&carried, &delta);
        assert_eq!(result.len(), 2, "Should have both f1 and f2");
        let has_f1 = result
            .iter()
            .any(|i| i.path == NonRootMPath::new("f1").unwrap());
        let has_f2 = result
            .iter()
            .any(|i| i.path == NonRootMPath::new("f2").unwrap());
        assert!(has_f1, "Should contain f1 from carried");
        assert!(has_f2, "Should contain f2 from delta");
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_merge_resolution_server_deletion_on_retry(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        // Test: If a previously-conflicting file is deleted on the server
        // in the delta range, the narrow-range check should detect the
        // conflict (file deleted vs client modified) and fail.
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

        init_just_knobs_for_merge_test();

        let base_content = "line1\nline2\nline3\nline4\nline5\n";
        let base = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file.txt", base_content)
            .commit()
            .await?;

        // S1: modify first line (resolvable conflict with client)
        let s1_content = "modified_line1\nline2\nline3\nline4\nline5\n";
        let s1 = CreateCommitContext::new(&ctx, &repo, vec![base])
            .add_file("file.txt", s1_content)
            .commit()
            .await?;

        // S2: delete file.txt
        let s2 = CreateCommitContext::new(&ctx, &repo, vec![s1])
            .delete_file("file.txt")
            .commit()
            .await?;

        // Set bookmark to S2
        let book = BookmarkKey::new("master")?;
        let hg_s2 = repo.derive_hg_changeset(&ctx, s2).await?;
        set_bookmark(ctx.clone(), &repo, &book, &format!("{hg_s2}")).await?;

        // Client: modify last line (based on base)
        let client_content = "line1\nline2\nline3\nline4\nmodified_line5\n";
        let client = CreateCommitContext::new(&ctx, &repo, vec![base])
            .add_file("file.txt", client_content)
            .commit()
            .await?;

        let client_bcs = client.load(&ctx, repo.repo_blobstore()).await?;
        let client_cf = find_changed_files(&ctx, &repo, base, client).await?;

        // Attempt 1: base → S1 (detects conflict, produces MergedFileInfo)
        let result1 = check_pushrebase_conflicts(
            &ctx,
            &repo,
            &Default::default(),
            base,
            base,
            s1,
            std::slice::from_ref(&client_bcs),
            &client_cf,
        )
        .await?;
        assert!(
            result1.merged_file_overrides.is_some(),
            "Attempt 1 should resolve the conflict"
        );

        // Attempt 2: S1 → S2 (file deleted — should fail with conflict)
        let result2 = check_pushrebase_conflicts(
            &ctx,
            &repo,
            &Default::default(),
            base,
            s1,
            s2,
            std::slice::from_ref(&client_bcs),
            &client_cf,
        )
        .await;

        // File was deleted on server but client modifies it — irreconcilable
        match result2 {
            Err(PushrebaseError::Conflicts(_)) => { /* expected */ }
            Err(e) => {
                panic!("Expected Conflicts error for file deleted on server, got error: {e}",)
            }
            Ok(_) => panic!("Expected Conflicts error for file deleted on server, but got Ok",),
        }

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_merge_resolution_no_conflict_in_delta(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        // Test: When merge resolution succeeds on attempt 1 and a subsequent
        // unrelated server commit moves the bookmark, the carried MergedFileInfo
        // is correctly used on retry (no conflict in the delta range).
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

        init_just_knobs_for_merge_test();

        let base_content = "line1\nline2\nline3\nline4\nline5\n";
        let base = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file.txt", base_content)
            .commit()
            .await?;

        // S1: modify first line
        let s1_content = "modified_line1\nline2\nline3\nline4\nline5\n";
        let s1 = CreateCommitContext::new(&ctx, &repo, vec![base])
            .add_file("file.txt", s1_content)
            .commit()
            .await?;

        // S2: unrelated change
        let s2 = CreateCommitContext::new(&ctx, &repo, vec![s1])
            .add_file("other.txt", "unrelated\n")
            .commit()
            .await?;

        let book = BookmarkKey::new("master")?;
        let hg_s2 = repo.derive_hg_changeset(&ctx, s2).await?;
        set_bookmark(ctx.clone(), &repo, &book, &format!("{hg_s2}")).await?;

        // Client: modify last line
        let client_content = "line1\nline2\nline3\nline4\nmodified_line5\n";
        let client = CreateCommitContext::new(&ctx, &repo, vec![base])
            .add_file("file.txt", client_content)
            .commit()
            .await?;

        let client_bcs = client.load(&ctx, repo.repo_blobstore()).await?;
        let client_cf = find_changed_files(&ctx, &repo, base, client).await?;

        // Attempt 1: base → S1
        let result1 = check_pushrebase_conflicts(
            &ctx,
            &repo,
            &Default::default(),
            base,
            base,
            s1,
            std::slice::from_ref(&client_bcs),
            &client_cf,
        )
        .await?;
        assert!(
            result1.merged_file_overrides.is_some(),
            "Attempt 1 should detect and resolve the conflict"
        );
        let carried = result1.merged_file_overrides.unwrap();

        // Attempt 2: S1 → S2 (no conflict in delta)
        let result2 = check_pushrebase_conflicts(
            &ctx,
            &repo,
            &Default::default(),
            base,
            s1,
            s2,
            std::slice::from_ref(&client_bcs),
            &client_cf,
        )
        .await?;
        assert!(
            result2.merged_file_overrides.is_none(),
            "No conflicts in narrow range S1→S2"
        );

        // Reconcile: carried info used as-is (no delta)
        let reconciled = carried;

        // Rebase with carried overrides
        let (new_head, _, rebased_bonsais) = create_rebased_changesets(
            &ctx,
            &repo,
            &Default::default(),
            base,
            client,
            s2,
            &mut [],
            Some(reconciled),
        )
        .await?;
        changesets_creation::save_changesets(&ctx, &repo, rebased_bonsais).await?;

        // Verify merged content
        let result_hg = repo.derive_hg_changeset(&ctx, new_head).await?;
        let result_cs = result_hg.load(&ctx, repo.repo_blobstore()).await?;
        let manifest = result_cs.manifestid();
        let file_path = NonRootMPath::new("file.txt")?;
        let file_entry = manifest
            .find_entry(ctx.clone(), repo.repo_blobstore().clone(), file_path.into())
            .await?
            .expect("file.txt should exist");

        let file_content = match file_entry {
            Entry::Leaf((_, filenode_id)) => {
                let content_id = filenode_id
                    .load(&ctx, repo.repo_blobstore())
                    .await?
                    .content_id();
                let bytes =
                    filestore::fetch_concat(repo.repo_blobstore(), &ctx, content_id).await?;
                String::from_utf8(bytes.to_vec())?
            }
            _ => panic!("file.txt should be a file"),
        };

        assert!(
            file_content.contains("modified_line1"),
            "should have server's change"
        );
        assert!(
            file_content.contains("modified_line5"),
            "should have client's change"
        );

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn batched_pushrebase_merge_resolution(fb: FacebookInit) -> Result<(), Error> {
        // Test: batched pushrebase resolves merge conflicts when server and
        // client modify different parts of the same file.
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

        init_just_knobs_for_merge_test();

        let base_content = "line1\nline2\nline3\nline4\nline5\n";
        let base = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file.txt", base_content)
            .commit()
            .await?;

        // Server commit: modify first line
        let server_content = "modified_line1\nline2\nline3\nline4\nline5\n";
        let server = CreateCommitContext::new(&ctx, &repo, vec![base])
            .add_file("file.txt", server_content)
            .commit()
            .await?;

        // Set bookmark to server commit
        let bookmark = BookmarkKey::new("master")?;
        let hg_server = repo.derive_hg_changeset(&ctx, server).await?;
        set_bookmark(ctx.clone(), &repo, &bookmark, &format!("{hg_server}")).await?;

        // Client: modify last line (based on base)
        let client_content = "line1\nline2\nline3\nline4\nmodified_line5\n";
        let client_cs_id = CreateCommitContext::new(&ctx, &repo, vec![base])
            .add_file("file.txt", client_content)
            .commit()
            .await?;

        let client_bcs = client_cs_id.load(&ctx, repo.repo_blobstore()).await?;
        let config = PushrebaseFlags::default();
        let idx = index_pushrebase_request(&ctx, &repo, &config, &bookmark, &hashset![client_bcs])
            .await?;

        let (tx, rx) = oneshot::channel();
        let request = PushrebaseRequest {
            changed_files: idx.changed_files,
            changesets: idx.changesets,
            head: idx.head,
            root: idx.root,
            conflict_check_base: idx.root,
            carried_merge_file_info: vec![],
            retry_num: PushrebaseRetryNum(0),
            hooks: vec![],
            response_tx: tx,
        };

        let requeued = do_batched_pushrebase(&ctx, &repo, &config, &bookmark, vec![request]).await;
        assert!(requeued.is_empty(), "Should not be requeued");

        let outcome = rx.await.unwrap().unwrap();
        assert!(
            outcome.merge_resolved_paths.is_some(),
            "Should report merge resolved paths"
        );
        let resolved_paths = outcome.merge_resolved_paths.unwrap();
        assert_eq!(resolved_paths.len(), 1);
        assert_eq!(resolved_paths[0], NonRootMPath::new("file.txt")?);

        // Verify merged content
        let result_hg = repo.derive_hg_changeset(&ctx, outcome.head).await?;
        let result_cs = result_hg.load(&ctx, repo.repo_blobstore()).await?;
        let manifest = result_cs.manifestid();
        let file_path = NonRootMPath::new("file.txt")?;
        let file_entry = manifest
            .find_entry(ctx.clone(), repo.repo_blobstore().clone(), file_path.into())
            .await?
            .expect("file.txt should exist");

        let file_content = match file_entry {
            Entry::Leaf((_, filenode_id)) => {
                let content_id = filenode_id
                    .load(&ctx, repo.repo_blobstore())
                    .await?
                    .content_id();
                let bytes =
                    filestore::fetch_concat(repo.repo_blobstore(), &ctx, content_id).await?;
                String::from_utf8(bytes.to_vec())?
            }
            _ => panic!("file.txt should be a file"),
        };

        assert!(
            file_content.contains("modified_line1"),
            "should have server's change"
        );
        assert!(
            file_content.contains("modified_line5"),
            "should have client's change"
        );

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn batched_pushrebase_merge_resolution_carry_forward(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        // Test: when batched pushrebase is re-queued after CAS failure,
        // carried_merge_file_info is preserved and reconciled on retry.
        // We simulate this by setting conflict_check_base to S1 and
        // providing carried MergedFileInfo from a prior attempt.
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

        init_just_knobs_for_merge_test();

        let base_content = "line1\nline2\nline3\nline4\nline5\n";
        let base = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file.txt", base_content)
            .commit()
            .await?;

        // S1: modify first line (conflict with client)
        let s1_content = "modified_line1\nline2\nline3\nline4\nline5\n";
        let s1 = CreateCommitContext::new(&ctx, &repo, vec![base])
            .add_file("file.txt", s1_content)
            .commit()
            .await?;

        // S2: unrelated change (no conflict in delta S1→S2)
        let s2 = CreateCommitContext::new(&ctx, &repo, vec![s1])
            .add_file("other.txt", "unrelated\n")
            .commit()
            .await?;

        let bookmark = BookmarkKey::new("master")?;
        let hg_s2 = repo.derive_hg_changeset(&ctx, s2).await?;
        set_bookmark(ctx.clone(), &repo, &bookmark, &format!("{hg_s2}")).await?;

        // Client: modify last line
        let client_content = "line1\nline2\nline3\nline4\nmodified_line5\n";
        let client_cs_id = CreateCommitContext::new(&ctx, &repo, vec![base])
            .add_file("file.txt", client_content)
            .commit()
            .await?;

        let client_bcs = client_cs_id.load(&ctx, repo.repo_blobstore()).await?;
        let config = PushrebaseFlags::default();
        let idx = index_pushrebase_request(&ctx, &repo, &config, &bookmark, &hashset![client_bcs])
            .await?;

        // First, get MergedFileInfo from a base→S1 check (simulating attempt 1)
        let result1 = check_pushrebase_conflicts(
            &ctx,
            &repo,
            &Default::default(),
            base,
            base,
            s1,
            std::slice::from_ref(idx.changesets.first().unwrap()),
            &idx.changed_files,
        )
        .await?;
        let carried = result1
            .merged_file_overrides
            .expect("Should have overrides from attempt 1");

        // Now simulate a retry: conflict_check_base = S1, carried info from attempt 1
        let (tx, rx) = oneshot::channel();
        let request = PushrebaseRequest {
            changed_files: idx.changed_files,
            changesets: idx.changesets,
            head: idx.head,
            root: idx.root,
            conflict_check_base: s1,
            carried_merge_file_info: carried,
            retry_num: PushrebaseRetryNum(1),
            hooks: vec![],
            response_tx: tx,
        };

        let requeued = do_batched_pushrebase(&ctx, &repo, &config, &bookmark, vec![request]).await;
        assert!(requeued.is_empty(), "Should not be requeued");

        let outcome = rx.await.unwrap().unwrap();
        assert!(
            outcome.merge_resolved_paths.is_some(),
            "Should report merge resolved paths via carry-forward"
        );

        // Verify merged content
        let result_hg = repo.derive_hg_changeset(&ctx, outcome.head).await?;
        let result_cs = result_hg.load(&ctx, repo.repo_blobstore()).await?;
        let manifest = result_cs.manifestid();
        let file_path = NonRootMPath::new("file.txt")?;
        let file_entry = manifest
            .find_entry(ctx.clone(), repo.repo_blobstore().clone(), file_path.into())
            .await?
            .expect("file.txt should exist");

        let file_content = match file_entry {
            Entry::Leaf((_, filenode_id)) => {
                let content_id = filenode_id
                    .load(&ctx, repo.repo_blobstore())
                    .await?
                    .content_id();
                let bytes =
                    filestore::fetch_concat(repo.repo_blobstore(), &ctx, content_id).await?;
                String::from_utf8(bytes.to_vec())?
            }
            _ => panic!("file.txt should be a file"),
        };

        assert!(
            file_content.contains("modified_line1"),
            "should have server's change via carry-forward"
        );
        assert!(
            file_content.contains("modified_line5"),
            "should have client's change"
        );

        Ok(())
    }

    fn init_just_knobs_for_noop_rejection_test(reject: bool) {
        override_just_knobs(JustKnobsInMemory::new(hashmap! {
            "scm/mononoke:pushrebase_dry_run_merge_resolution".to_string() => KnobVal::Bool(false),
            "scm/mononoke:pushrebase_enable_merge_resolution".to_string() => KnobVal::Bool(true),
            "scm/mononoke:pushrebase_merge_resolution_derive_fsnodes".to_string() => KnobVal::Bool(true),
            "scm/mononoke:pushrebase_reject_noop_merge_commits".to_string() => KnobVal::Bool(reject),
        }));
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_noop_merge_detected_only_when_jk_off(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        // JK off: client wrote identical content to server. The commit should
        // still land as a no-op (current Phase 1 dry-run behavior). Detection
        // is logged but no rejection happens.
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

        let base = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file.txt", "line1\nline2\nline3\n")
            .commit()
            .await?;

        let shared_content = "line1\nSHARED_EDIT\nline3\n";
        let server = CreateCommitContext::new(&ctx, &repo, vec![base])
            .add_file("file.txt", shared_content)
            .commit()
            .await?;

        let book = BookmarkKey::new("master")?;
        let hg_server = repo.derive_hg_changeset(&ctx, server).await?;
        set_bookmark(ctx.clone(), &repo, &book, &format!("{hg_server}")).await?;

        // Client wrote IDENTICAL content (same `shared_content`) — duplicate change.
        let client = CreateCommitContext::new(&ctx, &repo, vec![base])
            .add_file("file.txt", shared_content)
            .commit()
            .await?;
        let client_bcs = client.load(&ctx, repo.repo_blobstore()).await?;

        init_just_knobs_for_noop_rejection_test(false);

        // Should succeed (JK off → detection only, no rejection).
        let result = do_pushrebase_bonsai(
            &ctx,
            &repo,
            &Default::default(),
            &book,
            &hashset![client_bcs],
            &[],
        )
        .await?;

        assert!(result.rebased_changesets.len() == 1, "one commit rebased");
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_noop_merge_rejected_single_file(fb: FacebookInit) -> Result<(), Error> {
        // JK on: single-file duplicate-content commit → rejected with Conflicts
        // on that path, matching pre-merge-resolution behavior.
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

        let base = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file.txt", "line1\nline2\nline3\n")
            .commit()
            .await?;

        let shared_content = "line1\nSHARED_EDIT\nline3\n";
        let server = CreateCommitContext::new(&ctx, &repo, vec![base])
            .add_file("file.txt", shared_content)
            .commit()
            .await?;

        let book = BookmarkKey::new("master")?;
        let hg_server = repo.derive_hg_changeset(&ctx, server).await?;
        set_bookmark(ctx.clone(), &repo, &book, &format!("{hg_server}")).await?;

        let client = CreateCommitContext::new(&ctx, &repo, vec![base])
            .add_file("file.txt", shared_content)
            .commit()
            .await?;
        let client_bcs = client.load(&ctx, repo.repo_blobstore()).await?;

        init_just_knobs_for_noop_rejection_test(true);

        let result = do_pushrebase_bonsai(
            &ctx,
            &repo,
            &Default::default(),
            &book,
            &hashset![client_bcs],
            &[],
        )
        .await;

        match result {
            Err(PushrebaseError::Conflicts(conflicts)) => {
                assert_eq!(
                    conflicts.len(),
                    1,
                    "Should report one conflict for the single duplicate path"
                );
                let path_str = format!("{}", conflicts[0].left);
                assert_eq!(
                    path_str, "file.txt",
                    "Conflict should name the duplicate file"
                );
            }
            other => panic!("Expected Conflicts error, got: {other:?}"),
        }
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_noop_merge_mixed_commit_lands(fb: FacebookInit) -> Result<(), Error> {
        // NEGATIVE test: commit has both a duplicate-content file AND a
        // non-conflicting real change. The commit has a real net change so
        // it must NOT be flagged as no-op — should land normally.
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

        // Base has only the file that will conflict
        let base = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("dup.txt", "line1\nline2\nline3\n")
            .commit()
            .await?;

        let shared_content = "line1\nSHARED_EDIT\nline3\n";
        let server = CreateCommitContext::new(&ctx, &repo, vec![base])
            .add_file("dup.txt", shared_content)
            .commit()
            .await?;

        let book = BookmarkKey::new("master")?;
        let hg_server = repo.derive_hg_changeset(&ctx, server).await?;
        set_bookmark(ctx.clone(), &repo, &book, &format!("{hg_server}")).await?;

        // Client: duplicate edit to dup.txt + real new file `new.txt`
        let client = CreateCommitContext::new(&ctx, &repo, vec![base])
            .add_file("dup.txt", shared_content)
            .add_file("new.txt", "brand new content\n")
            .commit()
            .await?;
        let client_bcs = client.load(&ctx, repo.repo_blobstore()).await?;

        init_just_knobs_for_noop_rejection_test(true);

        // Should succeed despite the duplicate file_change because new.txt
        // is a real change.
        let result = do_pushrebase_bonsai(
            &ctx,
            &repo,
            &Default::default(),
            &book,
            &hashset![client_bcs],
            &[],
        )
        .await?;

        assert_eq!(
            result.rebased_changesets.len(),
            1,
            "One commit should be rebased — mixed commit must not be rejected"
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_noop_merge_in_stack_rejects_entire_stack(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        // JK on: 2-commit stack where the FIRST commit becomes a no-op (its
        // only file_change is a duplicate). The entire stack should be
        // rejected, not just the no-op commit.
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

        let base = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("dup.txt", "line1\nline2\nline3\n")
            .commit()
            .await?;

        let shared_content = "line1\nSHARED_EDIT\nline3\n";
        let server = CreateCommitContext::new(&ctx, &repo, vec![base])
            .add_file("dup.txt", shared_content)
            .commit()
            .await?;

        let book = BookmarkKey::new("master")?;
        let hg_server = repo.derive_hg_changeset(&ctx, server).await?;
        set_bookmark(ctx.clone(), &repo, &book, &format!("{hg_server}")).await?;

        // Client commit 1: duplicate edit to dup.txt only — would become no-op
        let c1 = CreateCommitContext::new(&ctx, &repo, vec![base])
            .add_file("dup.txt", shared_content)
            .commit()
            .await?;
        // Client commit 2 (HEAD): real change to a different file
        let c2 = CreateCommitContext::new(&ctx, &repo, vec![c1])
            .add_file("other.txt", "real content\n")
            .commit()
            .await?;
        let c1_bcs = c1.load(&ctx, repo.repo_blobstore()).await?;
        let c2_bcs = c2.load(&ctx, repo.repo_blobstore()).await?;

        init_just_knobs_for_noop_rejection_test(true);

        let result = do_pushrebase_bonsai(
            &ctx,
            &repo,
            &Default::default(),
            &book,
            &hashset![c1_bcs, c2_bcs],
            &[],
        )
        .await;

        match result {
            Err(PushrebaseError::Conflicts(conflicts)) => {
                let path_strs: HashSet<String> =
                    conflicts.iter().map(|c| format!("{}", c.left)).collect();
                assert!(
                    path_strs.contains("dup.txt"),
                    "Conflicts should include dup.txt from the no-op c1"
                );
            }
            other => panic!("Expected Conflicts error rejecting the entire stack, got: {other:?}"),
        }
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_noop_merge_rejected_all_files_duplicate(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        // JK on: client commit touches two files (A and B); server made the
        // identical edits to both. Every file_change is a duplicate, so the
        // commit becomes a no-op and must be rejected with conflicts on both
        // paths.
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

        let base = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("a.txt", "line1\nline2\nline3\n")
            .add_file("b.txt", "alpha\nbeta\ngamma\n")
            .commit()
            .await?;

        let shared_a = "line1\nA_EDIT\nline3\n";
        let shared_b = "alpha\nB_EDIT\ngamma\n";
        let server = CreateCommitContext::new(&ctx, &repo, vec![base])
            .add_file("a.txt", shared_a)
            .add_file("b.txt", shared_b)
            .commit()
            .await?;

        let book = BookmarkKey::new("master")?;
        let hg_server = repo.derive_hg_changeset(&ctx, server).await?;
        set_bookmark(ctx.clone(), &repo, &book, &format!("{hg_server}")).await?;

        // Client wrote IDENTICAL content to both files.
        let client = CreateCommitContext::new(&ctx, &repo, vec![base])
            .add_file("a.txt", shared_a)
            .add_file("b.txt", shared_b)
            .commit()
            .await?;
        let client_bcs = client.load(&ctx, repo.repo_blobstore()).await?;

        init_just_knobs_for_noop_rejection_test(true);

        let result = do_pushrebase_bonsai(
            &ctx,
            &repo,
            &Default::default(),
            &book,
            &hashset![client_bcs],
            &[],
        )
        .await;

        match result {
            Err(PushrebaseError::Conflicts(conflicts)) => {
                let path_strs: HashSet<String> =
                    conflicts.iter().map(|c| format!("{}", c.left)).collect();
                assert!(
                    path_strs.contains("a.txt") && path_strs.contains("b.txt"),
                    "Conflicts must include both duplicate paths, got: {path_strs:?}"
                );
            }
            other => panic!("Expected Conflicts error, got: {other:?}"),
        }
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_noop_merge_genuine_merge_not_flagged(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        // NEGATIVE test: client and server edited DIFFERENT lines of the same
        // file. local_content_id != other_id, so our new check is not
        // triggered. Cascading merge resolves cleanly, override ≠ other, the
        // commit makes a real net change, and is not classified as no-op even
        // when the JK is on.
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

        let base = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file.txt", "line1\nline2\nline3\n")
            .commit()
            .await?;

        // Server edits line 3 only.
        let server_content = "line1\nline2\nSERVER_EDIT\n";
        let server = CreateCommitContext::new(&ctx, &repo, vec![base])
            .add_file("file.txt", server_content)
            .commit()
            .await?;

        let book = BookmarkKey::new("master")?;
        let hg_server = repo.derive_hg_changeset(&ctx, server).await?;
        set_bookmark(ctx.clone(), &repo, &book, &format!("{hg_server}")).await?;

        // Client edits line 1 only — non-overlapping with server.
        let client_content = "CLIENT_EDIT\nline2\nline3\n";
        let client = CreateCommitContext::new(&ctx, &repo, vec![base])
            .add_file("file.txt", client_content)
            .commit()
            .await?;
        let client_bcs = client.load(&ctx, repo.repo_blobstore()).await?;

        init_just_knobs_for_noop_rejection_test(true);

        // Should succeed — genuine 3-way merge, no duplicate content detected.
        let result = do_pushrebase_bonsai(
            &ctx,
            &repo,
            &Default::default(),
            &book,
            &hashset![client_bcs],
            &[],
        )
        .await?;

        assert_eq!(
            result.rebased_changesets.len(),
            1,
            "Genuine merge must land — no duplicate paths detected"
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_noop_merge_local_equals_base_not_flagged(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        // NEGATIVE test: server edited file.txt away from base then edited it
        // BACK to the original base content. So path is in merge_paths
        // (server touched it), but base_id == other_id (server's net change
        // is zero). Client's edit goes through the existing
        // `base_id == other_id` short-circuit, NOT our new
        // `local_content_id == other_id` check, so duplicate_paths stays
        // empty and the commit is not flagged as no-op.
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

        let base_content = "line1\nline2\nline3\n";
        let base = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file.txt", base_content)
            .commit()
            .await?;

        // Server commit 1: edit away from base.
        let server1 = CreateCommitContext::new(&ctx, &repo, vec![base])
            .add_file("file.txt", "line1\nSERVER_INTERMEDIATE\nline3\n")
            .commit()
            .await?;

        // Server commit 2: revert back to base content. base_id == other_id
        // for this path now (root and current_master agree on content).
        let server2 = CreateCommitContext::new(&ctx, &repo, vec![server1])
            .add_file("file.txt", base_content)
            .commit()
            .await?;

        let book = BookmarkKey::new("master")?;
        let hg_server2 = repo.derive_hg_changeset(&ctx, server2).await?;
        set_bookmark(ctx.clone(), &repo, &book, &format!("{hg_server2}")).await?;

        // Client edits file.txt to brand-new content.
        let client = CreateCommitContext::new(&ctx, &repo, vec![base])
            .add_file("file.txt", "line1\nCLIENT_EDIT\nline3\n")
            .commit()
            .await?;
        let client_bcs = client.load(&ctx, repo.repo_blobstore()).await?;

        init_just_knobs_for_noop_rejection_test(true);

        // Should succeed — base==other path is hit, no duplicate detected.
        let result = do_pushrebase_bonsai(
            &ctx,
            &repo,
            &Default::default(),
            &book,
            &hashset![client_bcs],
            &[],
        )
        .await?;

        assert_eq!(
            result.rebased_changesets.len(),
            1,
            "base==other branch must not trip our duplicate-content detection"
        );
        Ok(())
    }

    fn pessimistic_config() -> PushrebaseFlags {
        PushrebaseFlags {
            pessimistic_locking_bookmarks: vec![master_bookmark()],
            ..Default::default()
        }
    }

    fn init_just_knobs_for_pessimistic_test() {
        override_just_knobs(JustKnobsInMemory::new(hashmap! {
            "scm/mononoke:pushrebase_dry_run_merge_resolution".to_string() => KnobVal::Bool(false),
            "scm/mononoke:pushrebase_enable_merge_resolution".to_string() => KnobVal::Bool(false),
            "scm/mononoke:pushrebase_merge_resolution_derive_fsnodes".to_string() => KnobVal::Bool(true),
            "scm/mononoke:per_bookmark_locking".to_string() => KnobVal::Bool(true),
        }));
    }

    // NOTE: Full end-to-end pessimistic pushrebase cannot be tested with
    // SQLite unit tests because TestRepoFactory shares a single SQLite
    // connection across all facets. LockedBookmarkTransaction holds the
    // connection open during rebase, and save_changesets ->
    // CommitGraphWriter::add_many tries to acquire the same connection,
    // causing a deadlock. Full E2E is covered by integration tests (MySQL).

    #[mononoke::fbinit_test]
    async fn pessimistic_pushrebase_conflict(fb: FacebookInit) -> Result<(), Error> {
        init_just_knobs_for_pessimistic_test();
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;

        let book = master_bookmark();
        bookmark(&ctx, &repo, book.clone())
            .set_to("a5ffa77602a066db7d5cfb9fb5823a0895717c5a")
            .await?;

        let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
        let p = repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(&ctx, root)
            .await?
            .ok_or_else(|| Error::msg("Root is missing"))?;

        let bcs_id = CreateCommitContext::new(&ctx, &repo, vec![p])
            .add_file("files", "conflicting content")
            .commit()
            .await?;
        let bcs = bcs_id.load(&ctx, repo.repo_blobstore()).await?;

        let config = pessimistic_config();
        let result = do_pushrebase_bonsai(&ctx, &repo, &config, &book, &hashset![bcs], &[]).await;

        should_have_conflicts(result);
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pessimistic_dispatch_selection(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;

        let book = master_bookmark();
        let other_book = BookmarkKey::new("other_bookmark")?;
        let config = pessimistic_config();
        let repo_id_str = repo.repo_identity().id().to_string();

        init_just_knobs_for_test();
        let use_pessimistic = justknobs::eval(
            "scm/mononoke:per_bookmark_locking",
            None,
            Some(&repo_id_str),
        ) && config.pessimistic_locking_bookmarks.contains(&book);
        assert!(!use_pessimistic, "should be optimistic when knob is off");

        init_just_knobs_for_pessimistic_test();
        let use_pessimistic = justknobs::eval(
            "scm/mononoke:per_bookmark_locking",
            None,
            Some(&repo_id_str),
        ) && config.pessimistic_locking_bookmarks.contains(&other_book);
        assert!(
            !use_pessimistic,
            "should be optimistic when bookmark not in pessimistic list"
        );

        let use_pessimistic = justknobs::eval(
            "scm/mononoke:per_bookmark_locking",
            None,
            Some(&repo_id_str),
        ) && config.pessimistic_locking_bookmarks.contains(&book);
        assert!(
            use_pessimistic,
            "should be pessimistic when knob is on and bookmark is in list"
        );

        let config_empty = PushrebaseFlags::default();
        let use_pessimistic = justknobs::eval(
            "scm/mononoke:per_bookmark_locking",
            None,
            Some(&repo_id_str),
        ) && config_empty.pessimistic_locking_bookmarks.contains(&book);
        assert!(
            !use_pessimistic,
            "should be optimistic with empty pessimistic_locking_bookmarks"
        );

        drop(ctx);
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pessimistic_locked_transaction_lifecycle(fb: FacebookInit) -> Result<(), Error> {
        init_just_knobs_for_pessimistic_test();
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

        let book = master_bookmark();

        let root_cs = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file", "content")
            .commit()
            .await?;

        bookmark(&ctx, &repo, book.clone()).set_to(root_cs).await?;

        let child_cs = CreateCommitContext::new(&ctx, &repo, vec![root_cs])
            .add_file("file2", "content2")
            .commit()
            .await?;

        let sql_bookmarks = repo.sql_bookmarks();
        let locked_txn = sql_bookmarks.start_locked_transaction(&ctx, &book).await?;

        assert_eq!(locked_txn.current_value(), Some(root_cs));

        let log_id = locked_txn
            .commit(&ctx, child_cs, BookmarkUpdateReason::Pushrebase, vec![])
            .await?;

        assert!(log_id.is_some(), "CAS should succeed under the lock");

        let new_value = repo
            .bookmarks()
            .get(ctx.clone(), &book, bookmarks::Freshness::MostRecent)
            .await?;
        assert_eq!(new_value, Some(child_cs));

        Ok(())
    }

    // Bookmark points at a server commit that edited line 1; client commit
    // edits line 5. Path-level conflict that MR can resolve, vanilla can't.
    async fn setup_non_overlapping_conflict(
        ctx: &CoreContext,
        repo: &PushrebaseTestRepo,
    ) -> Result<(BonsaiChangeset, BookmarkKey), Error> {
        let base_content = "line1\nline2\nline3\nline4\nline5\n";
        let base = CreateCommitContext::new_root(ctx, repo)
            .add_file("file.txt", base_content)
            .commit()
            .await?;

        let server_content = "modified_line1\nline2\nline3\nline4\nline5\n";
        let server = CreateCommitContext::new(ctx, repo, vec![base])
            .add_file("file.txt", server_content)
            .commit()
            .await?;

        let book = BookmarkKey::new("master")?;
        let hg_server = repo.derive_hg_changeset(ctx, server).await?;
        set_bookmark(ctx.clone(), repo, &book, &format!("{hg_server}")).await?;

        let client_content = "line1\nline2\nline3\nline4\nmodified_line5\n";
        let client = CreateCommitContext::new(ctx, repo, vec![base])
            .add_file("file.txt", client_content)
            .commit()
            .await?;
        let client_bcs = client.load(ctx, repo.repo_blobstore()).await?;

        Ok((client_bcs, book))
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_merge_resolution_override_forces_off(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        // JK on, override off → must conflict.
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;
        let (client_bcs, book) = setup_non_overlapping_conflict(&ctx, &repo).await?;

        init_just_knobs_for_merge_test();

        let config = PushrebaseFlags {
            merge_resolution_override: MergeResolutionOverride::ForceOff,
            ..Default::default()
        };

        let result =
            do_pushrebase_bonsai(&ctx, &repo, &config, &book, &hashset![client_bcs], &[]).await;

        should_have_conflicts(result);
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_merge_resolution_override_forces_on(fb: FacebookInit) -> Result<(), Error> {
        // JK off, override on → must merge cleanly.
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;
        let (client_bcs, book) = setup_non_overlapping_conflict(&ctx, &repo).await?;

        override_just_knobs(JustKnobsInMemory::new(hashmap! {
            "scm/mononoke:pushrebase_enable_merge_resolution".to_string() => KnobVal::Bool(false),
            "scm/mononoke:pushrebase_merge_resolution_derive_fsnodes".to_string() => KnobVal::Bool(true),
        }));

        let config = PushrebaseFlags {
            merge_resolution_override: MergeResolutionOverride::ForceOn,
            ..Default::default()
        };

        let result =
            do_pushrebase_bonsai(&ctx, &repo, &config, &book, &hashset![client_bcs], &[]).await?;

        let result_hg = repo.derive_hg_changeset(&ctx, result.head).await?;
        let expected_content = "modified_line1\nline2\nline3\nline4\nmodified_line5\n";
        ensure_content(
            &ctx,
            result_hg,
            &repo,
            btreemap! {
                "file.txt".to_string() => expected_content.to_string(),
            },
        )
        .await?;
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn pushrebase_merge_resolution_override_none_falls_through(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        // No override, JK off → must conflict (historical path).
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;
        let (client_bcs, book) = setup_non_overlapping_conflict(&ctx, &repo).await?;

        override_just_knobs(JustKnobsInMemory::new(hashmap! {
            "scm/mononoke:pushrebase_enable_merge_resolution".to_string() => KnobVal::Bool(false),
            "scm/mononoke:pushrebase_merge_resolution_derive_fsnodes".to_string() => KnobVal::Bool(true),
        }));

        let config = PushrebaseFlags {
            merge_resolution_override: MergeResolutionOverride::UseJk,
            ..Default::default()
        };

        let result =
            do_pushrebase_bonsai(&ctx, &repo, &config, &book, &hashset![client_bcs], &[]).await;

        should_have_conflicts(result);
        Ok(())
    }
}
