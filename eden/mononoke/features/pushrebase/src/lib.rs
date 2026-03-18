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
use std::sync::Arc;
use std::time::Instant;

use anyhow::Error;
use anyhow::Result;
use anyhow::format_err;
use blobrepo_utils::convert_diff_result_into_file_change_for_diamond_merge;
use blobstore::Loadable;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkUpdateLogId;
use bookmarks::BookmarkUpdateReason;
use bookmarks::BookmarksRef;
use bytes::Bytes;
use commit_graph::CommitGraphRef;
use commit_graph::CommitGraphWriterRef;
use context::CoreContext;
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
use mononoke_types::Timestamp;
use mononoke_types::check_case_conflicts;
use mononoke_types::find_path_conflicts;
use mononoke_types::fsnode::FsnodeFile;
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
use three_way_merge::MergeResult;
use three_way_merge::merge_text;
use tokio::sync::oneshot;
use tracing::info;

define_stats! {
    prefix = "mononoke.pushrebase";
    // Clowntown: This is actually nanoseconds (ns), not microseconds (us)
    critical_section_success_duration_us: dynamic_timeseries("{}.critical_section_success_duration_us", (reponame: String); Average, Sum, Count),
    critical_section_failure_duration_us: dynamic_timeseries("{}.critical_section_failure_duration_us", (reponame: String); Average, Sum, Count),
    critical_section_retries_failed: dynamic_timeseries("{}.critical_section_retries_failed", (reponame: String); Average, Sum),
    commits_rebased: dynamic_timeseries("{}.commits_rebased", (reponame: String); Average, Sum, Count),
    conflict_rejections: dynamic_timeseries("{}.conflict_rejections", (reponame: String); Count),
    conflict_files_count: dynamic_timeseries("{}.conflict_files_count", (reponame: String); Average, Sum, Count),
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

#[derive(Debug, Clone, Copy)]
pub struct PushrebaseRetryNum(pub usize);

#[derive(Debug, Clone, Copy)]
pub struct PushrebaseDistance(pub usize);

impl PushrebaseDistance {
    fn add(&self, value: usize) -> Self {
        let PushrebaseDistance(prev) = self;
        PushrebaseDistance(prev + value)
    }
}

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

/// Does a pushrebase of a list of commits `pushed` onto `onto_bookmark`
/// The commits from the pushed set should already be committed to the blobrepo
/// Returns updated bookmark value.
pub async fn do_pushrebase_bonsai(
    ctx: &CoreContext,
    repo: &impl Repo,
    config: &PushrebaseFlags,
    onto_bookmark: &BookmarkKey,
    pushed: &HashSet<BonsaiChangeset>,
    prepushrebase_hooks: &[Box<dyn PushrebaseHook>],
) -> Result<PushrebaseOutcome, PushrebaseError> {
    let PushrebaseRequestIndex {
        changed_files: client_cf,
        changesets: client_bcs,
        head,
        root,
    } = index_pushrebase_request(ctx, repo, config, onto_bookmark, pushed).await?;

    let res = rebase_in_loop(
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
    .await?;

    Ok(res)
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
    repo: &impl Repo,
    config: &PushrebaseFlags,
    onto_bookmark: &BookmarkKey,
    requests: Vec<PushrebaseRequest>,
) -> Vec<PushrebaseRequest> {
    let should_log = config.monitoring_bookmark.as_deref() == Some(onto_bookmark.as_str());
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
    let mut pending: Vec<(PushrebaseRequest, ChangesetId, usize, Option<ChangesetId>)> = vec![];
    let mut running_head = old_bookmark_value;
    let mut all_rebased_changesets: RebasedChangesets = Default::default();
    let mut all_rebased_bonsais: Vec<BonsaiChangeset> = Vec::new();

    let mut requests_iter = requests.into_iter();
    while let Some(request) = requests_iter.next() {
        let bookmark_val = old_bookmark_value.unwrap_or(request.conflict_check_base);
        let merged_file_overrides = match check_pushrebase_conflicts(
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
            Ok(result) => result.merged_file_overrides,
            Err(e) => {
                let _ = request.response_tx.send(Err(SharedError::from(e)));
                continue;
            }
        };

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
            merged_file_overrides,
        )
        .await;

        match rebase_result {
            Ok((new_head, rebased, rebased_bonsais)) => {
                all_rebased_changesets.extend(rebased);
                all_rebased_bonsais.extend(rebased_bonsais);
                running_head = Some(new_head);
                pending.push((
                    request,
                    new_head,
                    pushrebase_distance,
                    request_old_bookmark_value,
                ));
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
                    .map(|(req, _, _, _)| req)
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
        for (req, _, _, _) in pending {
            let _ = req.response_tx.send(Err(shared.clone()));
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
            for (req, _, _, _) in pending {
                let _ = req.response_tx.send(Err(shared.clone()));
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
            if should_log {
                STATS::critical_section_success_duration_us
                    .add_value(critical_section_duration_us, repo_args.clone());
                STATS::commits_rebased.add_value(all_rebased_pairs.len() as i64, repo_args);
            }

            for (req, new_head, distance, req_old_bookmark_value) in pending {
                let stack_pairs: Vec<PushrebaseChangesetPair> = all_rebased_pairs
                    .iter()
                    .filter(|pair| {
                        req.changesets
                            .iter()
                            .any(|cs| cs.get_changeset_id() == pair.id_old)
                    })
                    .cloned()
                    .collect();

                let _ = req.response_tx.send(Ok(PushrebaseOutcome {
                    old_bookmark_value: Some(req_old_bookmark_value.unwrap_or(req.root)),
                    head: new_head,
                    retry_num: req.retry_num,
                    rebased_changesets: stack_pairs,
                    pushrebase_distance: PushrebaseDistance(distance),
                    log_id,
                    merge_resolved_paths: None,
                }));
            }
            vec![]
        }
        Ok(None) => {
            // CAS failed — update conflict_check_base and return for re-queue.
            if should_log {
                STATS::critical_section_failure_duration_us
                    .add_value(critical_section_duration_us, repo_args);
            }
            pending
                .into_iter()
                .map(|(mut req, _, _, _)| {
                    req.conflict_check_base = old_bookmark_value.unwrap_or(req.conflict_check_base);
                    req.retry_num = PushrebaseRetryNum(req.retry_num.0 + 1);
                    req
                })
                .collect()
        }
        Err(e) => {
            let shared = SharedError::from(e);
            for (req, _, _, _) in pending {
                let _ = req.response_tx.send(Err(shared.clone()));
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

/// Result of conflict checking in pushrebase.
struct ConflictCheckResult {
    /// Number of server-side changesets (used for pushrebase_distance tracking).
    server_changeset_count: usize,
    /// If merge resolution succeeded, the merged file changes to apply.
    /// `None` means no conflicts or merge resolution was not attempted.
    merged_file_overrides: Option<Vec<(NonRootMPath, FileChange)>>,
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
        }),
        Err(PushrebaseError::Conflicts(conflicts)) => {
            let reponame = repo.repo_identity().name();
            STATS::conflict_rejections.add_value(1, (reponame.to_string(),));
            STATS::conflict_files_count.add_value(conflicts.len() as i64, (reponame.to_string(),));

            let merge_enabled = justknobs::eval(
                "scm/mononoke:pushrebase_enable_merge_resolution",
                None,
                Some(reponame),
            )?;
            let max_merge_conflicts: usize = justknobs::get_as::<usize>(
                "scm/mononoke:pushrebase_max_merge_conflicts",
                Some(reponame),
            )?;
            let max_merge_file_size: u64 = justknobs::get_as::<u64>(
                "scm/mononoke:pushrebase_max_merge_file_size",
                Some(reponame),
            )?;
            let merge_result = if merge_enabled {
                let derive_fsnodes: bool = justknobs::eval(
                    "scm/mononoke:pushrebase_merge_resolution_derive_fsnodes",
                    None,
                    Some(reponame),
                )?;
                Some(
                    attempt_merge_resolution(
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
                    .await,
                )
            } else {
                None
            };

            match merge_result {
                Some(Ok(merged_changes)) => Ok(ConflictCheckResult {
                    server_changeset_count: server_bcs_len,
                    merged_file_overrides: Some(merged_changes),
                }),
                _ => {
                    // Log merge failure reason if merge was attempted
                    if let Some(Err(ref err)) = merge_result {
                        ctx.scuba()
                            .clone()
                            .add("merge_resolution_outcome", format!("{}", err))
                            .log_with_msg("Pushrebase merge resolution failed", None);
                    }
                    // Run dry-run merge if enabled (for logging/observability)
                    let dry_run_enabled = justknobs::eval(
                        "scm/mononoke:pushrebase_dry_run_merge_resolution",
                        None,
                        Some(reponame),
                    )?;
                    if dry_run_enabled {
                        let derive_fsnodes: bool = justknobs::eval(
                            "scm/mononoke:pushrebase_merge_resolution_derive_fsnodes",
                            None,
                            Some(reponame),
                        )?;
                        dry_run_merge_resolution(
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
    let mut latest_rebase_attempt = root;
    let mut pushrebase_distance = PushrebaseDistance(0);

    let repo_args = (repo.repo_identity().name().to_string(),);
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
        pushrebase_distance = pushrebase_distance.add(conflict_result.server_changeset_count);

        // Extract merged paths for observability before overrides are consumed
        let merge_resolved_paths = conflict_result
            .merged_file_overrides
            .as_ref()
            .map(|overrides| overrides.iter().map(|(path, _)| path.clone()).collect());

        let rebase_outcome = do_rebase(
            ctx,
            repo,
            config,
            root,
            head,
            old_bookmark_value,
            onto_bookmark,
            hooks,
            conflict_result.merged_file_overrides,
        )
        .await?;
        // CRITICAL SECTION END: Right after writing new value of bookmark

        let critical_section_duration_us: i64 = start_critical_section
            .elapsed()
            .as_nanos()
            .try_into()
            .unwrap_or(i64::MAX);
        if let Some((head, log_id, rebased_changesets)) = rebase_outcome {
            if should_log {
                STATS::critical_section_success_duration_us
                    .add_value(critical_section_duration_us, repo_args.clone());
                STATS::critical_section_retries_failed
                    .add_value(retry_num.0 as i64, repo_args.clone());
                STATS::commits_rebased
                    .add_value(rebased_changesets.len() as i64, repo_args.clone());
            }
            let res = PushrebaseOutcome {
                old_bookmark_value: Some(old_bookmark_value.unwrap_or(root)),
                head,
                retry_num,
                rebased_changesets,
                pushrebase_distance,
                log_id,
                merge_resolved_paths,
            };
            return Ok(res);
        } else if should_log {
            STATS::critical_section_failure_duration_us
                .add_value(critical_section_duration_us, repo_args.clone());
        }

        latest_rebase_attempt = old_bookmark_value.unwrap_or(root);
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
    merged_file_overrides: Option<Vec<(NonRootMPath, FileChange)>>,
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
                            "`range_stream` produced invalid result for: ({}, {})",
                            descendant, ancestor,
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

/// Fetch fsnode file entry for a given path from a changeset's fsnode manifest.
/// Returns the FsnodeFile which provides access to content_id and file_type.
async fn fetch_fsnode_file(
    ctx: &CoreContext,
    repo: &impl Repo,
    cs_id: ChangesetId,
    path: &NonRootMPath,
) -> Result<Option<FsnodeFile>> {
    use manifest::Entry;

    let root_fsnode_id = repo
        .repo_derived_data()
        .derive::<RootFsnodeId>(ctx, cs_id, DerivationPriority::LOW)
        .await?;

    let mf_id = root_fsnode_id.fsnode_id().clone();
    let entry = mf_id
        .find_entry(
            ctx.clone(),
            repo.repo_blobstore().clone(),
            path.clone().into(),
        )
        .await?;

    match entry {
        Some(Entry::Leaf(fsnode_file)) => Ok(Some(fsnode_file)),
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
/// Fetches the base (root) version from fsnode manifests. The other
/// (server-side) content is passed directly as `other_content_id` to avoid
/// expensive fsnode derivation in the critical section — callers obtain it
/// from the bonsai changesets instead.
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
    // Fetch base fsnode entry (only derivation needed — root is pre-derived)
    let base_fsnode = match fetch_fsnode_file(ctx, repo, root, path).await {
        Ok(Some(f)) => f,
        Ok(None) => {
            return FileMergeOutcome::Skipped(format!("file {} not found in base", path));
        }
        Err(e) => return FileMergeOutcome::Error(e),
    };

    // Validate file types if expected type is provided
    if let Some(local_type) = expected_file_type {
        let base_type = *base_fsnode.file_type();
        if base_type != local_type {
            return FileMergeOutcome::Skipped(format!(
                "file {} has type mismatch: base={:?}, local={:?}",
                path, base_type, local_type,
            ));
        }
    }

    // Fetch all three file contents concurrently
    let (base_bytes, local_bytes, other_bytes) = futures::join!(
        filestore::fetch_concat(repo.repo_blobstore(), ctx, *base_fsnode.content_id()),
        filestore::fetch_concat(repo.repo_blobstore(), ctx, local_content_id),
        filestore::fetch_concat(repo.repo_blobstore(), ctx, other_content_id),
    );

    match (base_bytes, local_bytes, other_bytes) {
        (Ok(base), Ok(local), Ok(other)) => match merge_text(&base, &local, &other) {
            MergeResult::Clean(merged) => FileMergeOutcome::Clean(Bytes::from(merged)),
            MergeResult::Conflict(desc) => {
                FileMergeOutcome::Conflict(format!("file {}: {}", path, desc))
            }
        },
        (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => FileMergeOutcome::Error(e),
    }
}

/// Attempt a dry-run 3-way merge on conflicting files, logging outcomes
/// to Scuba without changing pushrebase behavior.
///
/// This fetches file content for each conflicting path from the common
/// ancestor, pushed changeset, and bookmark head, then runs merge_text
/// to determine if the conflict would be resolvable.
async fn dry_run_merge_resolution(
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
                    "{}: not a tracked change in pushed changeset",
                    non_root_path,
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
                    "{}: not a tracked change in bookmark head",
                    non_root_path,
                ));
                continue;
            }
        };

        // Fail early if any file exceeds the size limit — we can't resolve
        // all conflicts if we have to skip a file, so the entire merge would fail.
        if local_fc.size() > max_file_size || server_fc.size() > max_file_size {
            ctx.scuba()
                .clone()
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
            FileMergeOutcome::Conflict(_) => {
                all_clean = false;
                conflict_count += 1;
            }
            FileMergeOutcome::Skipped(reason) => {
                all_clean = false;
                skipped_count += 1;
                skip_reasons.push(reason);
            }
            FileMergeOutcome::Error(err) => {
                all_clean = false;
                error_count += 1;
                error_reasons.push(format!("{:#}", err));
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
    /// At least one file has a true content-level conflict.
    #[error("unresolvable conflict: {0}")]
    UnresolvableConflict(String),
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

/// Attempt content-level merge resolution for conflicting files.
///
/// For each conflicting path, fetches file content from the common ancestor
/// (root), the pushed changeset, and the bookmark head (from server bonsai
/// changesets), then runs a 3-way merge. Returns a map of merged file changes
/// to apply to the rebased changeset.
///
/// The server-side content is obtained directly from the bonsai changesets
/// rather than deriving fsnodes, to avoid expensive derivation in the
/// critical section of pushrebase.
///
/// Fails if any file has a true content-level conflict, or if any file
/// cannot be merged (missing, binary, type change, copy info, too large).
async fn attempt_merge_resolution(
    ctx: &CoreContext,
    repo: &impl Repo,
    conflicts: &[PushrebaseConflict],
    root: ChangesetId,
    server_bcs: &[BonsaiChangeset],
    client_bcs: &[BonsaiChangeset],
    max_conflicts: usize,
    max_file_size: u64,
    derive_fsnodes: bool,
) -> Result<Vec<(NonRootMPath, FileChange)>, MergeResolutionError> {
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

        // Get the client file change for this path
        let client_fc = match client_changes.get(&non_root_path) {
            Some(FileChange::Change(tc)) => tc,
            _ => {
                return Err(MergeResolutionError::Skipped(format!(
                    "file {} not a tracked change in pushed changeset",
                    path,
                )));
            }
        };

        // Skip files with copy info
        if client_fc.copy_from().is_some() {
            return Err(MergeResolutionError::Skipped(format!(
                "file {} has copy-from info",
                path,
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
                    "file {} not a tracked change in bookmark head",
                    path,
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

        match try_merge_file(
            ctx,
            repo,
            root,
            &non_root_path,
            client_fc.content_id(),
            server_fc.content_id(),
            Some(local_file_type),
        )
        .await
        {
            FileMergeOutcome::Clean(merged_bytes) => {
                // Store the merged content
                let size = merged_bytes.len() as u64;
                let meta = filestore::store(
                    repo.repo_blobstore(),
                    *repo.filestore_config(),
                    ctx,
                    &filestore::StoreRequest::new(size),
                    stream::once(future::ok(merged_bytes)),
                )
                .await
                .map_err(MergeResolutionError::InternalError)?;

                let file_change = FileChange::tracked(
                    meta.content_id,
                    local_file_type,
                    meta.total_size,
                    None, // no copy info for merged files
                    GitLfs::FullContent,
                );

                merged_file_changes.push((non_root_path, file_change));
            }
            FileMergeOutcome::Conflict(description) => {
                return Err(MergeResolutionError::UnresolvableConflict(description));
            }
            FileMergeOutcome::Skipped(reason) => {
                return Err(MergeResolutionError::Skipped(reason));
            }
            FileMergeOutcome::Error(err) => {
                return Err(MergeResolutionError::InternalError(err));
            }
        }
    }

    // Log success
    ctx.scuba()
        .clone()
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
    merged_file_overrides: Option<Vec<(NonRootMPath, FileChange)>>,
) -> Result<(ChangesetId, RebasedChangesets, Vec<BonsaiChangeset>), PushrebaseError> {
    let rebased_set = find_rebased_set(ctx, repo, root, head).await?;

    let rebased_set_ids: HashSet<_> = rebased_set.iter().map(|cs| cs.get_changeset_id()).collect();

    let date = if config.rewritedates {
        Some(Timestamp::now())
    } else {
        None
    };

    // rebased_set already sorted in reverse topological order, which guarantees
    // that all required nodes will be updated by the time they are needed

    // Create a fake timestamp, it doesn't matter what timestamp root has

    let mut remapping = hashmap! { root => (onto, Timestamp::now()) };
    let mut rebased = Vec::new();
    for bcs_old in rebased_set {
        let id_old = bcs_old.get_changeset_id();
        // Only apply merged file overrides to the head changeset
        let overrides_for_this = if id_old == head {
            merged_file_overrides.as_ref()
        } else {
            None
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
            overrides_for_this,
        )
        .await?;
        let timestamp = Timestamp::from(*bcs_new.author_date());
        remapping.insert(id_old, (bcs_new.get_changeset_id(), timestamp));
        rebased.push(bcs_new);
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

    let hooks = Arc::new(hooks);

    let sql_txn_hook = move |ctx, mut sql_txn| {
        let hooks = hooks.clone();
        async move {
            for hook in hooks.iter() {
                sql_txn = hook.populate_transaction(&ctx, sql_txn).await?
            }
            Ok(sql_txn)
        }
        .boxed()
    };

    let maybe_log_id = txn
        .commit_with_hooks(vec![Arc::new(sql_txn_hook)])
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
    use rand::Rng;
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
        repo: &(impl Repo + BonsaiHgMappingRef),
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
            .ok_or_else(|| Error::msg(format_err!("Head not found: {:?}", cs_id)))?;

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
        repo: &(impl Repo + BonsaiHgMappingRef),
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
            .map_err(|err| format_err!("{:?}", err))
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
        .map_err(|err| format_err!("{:?}", err))
        .await?;

        let master_val = resolve_cs_id(&ctx, &repo, "master").await?;
        let key = format!("{}", master_val);
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
        .map_err(|err| format_err!("{:?}", err))
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
                let file = format!("f{}", index);
                let content = format!("{}", index);
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
            let us = rand::rng().random_range(0..100);
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

            let f = format!("file{}", i);
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

            let f = format!("file{}", i);
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
            let r = format!("{}", merge_hg_cs_id);
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
            let r = format!("{}", hg_cs);
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
            let r = format!("{}", hg_cs);
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
            let r = format!("{}", hg_cs);
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
        .map_err(|err| format_err!("{:?}", err))
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
                    "unexpected result: expected ForceFailPushrebase error, found {:?}",
                    err
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
        .map_err(|err| format_err!("{:?}", err))
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
        let outcome_a = rx_a.await.unwrap().map_err(|e| format_err!("{:?}", e))?;
        let outcome_b = rx_b.await.unwrap().map_err(|e| format_err!("{:?}", e))?;

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
            retry_num: PushrebaseRetryNum(0),
            hooks: vec![],
            response_tx: tx_b,
        };

        let requeued =
            do_batched_pushrebase(&ctx, &repo, &config, &bookmark, vec![req_a, req_b]).await;
        assert!(requeued.is_empty(), "Expected no re-queued requests");

        // Stack A should succeed
        let outcome_a = rx_a.await.unwrap().map_err(|e| format_err!("{:?}", e))?;
        assert_eq!(outcome_a.rebased_changesets.len(), 1);

        // Stack B should fail with conflicts
        let result_b = rx_b.await.unwrap();
        assert!(result_b.is_err(), "Expected stack B to fail with conflicts");
        match result_b.unwrap_err().inner() {
            PushrebaseError::Conflicts(_) => {}
            other => panic!("Expected Conflicts error, got: {:?}", other),
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
        set_bookmark(ctx.clone(), &repo, &book, &format!("{}", hg_server)).await?;

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
        set_bookmark(ctx.clone(), &repo, &book, &format!("{}", hg_server)).await?;

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
}
