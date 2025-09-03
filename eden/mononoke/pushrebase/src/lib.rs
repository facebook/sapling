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

use std::cmp::Ordering;
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
use bonsai_hg_mapping::BonsaiHgMappingRef;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkUpdateLogId;
use bookmarks::BookmarkUpdateReason;
use bookmarks::BookmarksRef;
use commit_graph::CommitGraphRef;
use commit_graph::CommitGraphWriterRef;
use context::CoreContext;
use filenodes_derivation::FilenodesOnlyPublic;
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
use mononoke_types::DateTime;
use mononoke_types::DerivableType;
use mononoke_types::FileChange;
use mononoke_types::FileType;
use mononoke_types::GitLfs;
use mononoke_types::MPath;
use mononoke_types::Timestamp;
use mononoke_types::check_case_conflicts;
use pushrebase_hook::PushrebaseCommitHook;
use pushrebase_hook::PushrebaseHook;
use pushrebase_hook::PushrebaseTransactionHook;
use pushrebase_hook::RebasedChangesets;
use repo_blobstore::RepoBlobstoreArc;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentityRef;
use slog::info;
use stats::prelude::*;
use thiserror::Error;

define_stats! {
    prefix = "mononoke.pushrebase";
    // Clowntown: This is actually nanoseconds (ns), not microseconds (us)
    critical_section_success_duration_us: dynamic_timeseries("{}.critical_section_success_duration_us", (reponame: String); Average, Sum, Count),
    critical_section_failure_duration_us: dynamic_timeseries("{}.critical_section_failure_duration_us", (reponame: String); Average, Sum, Count),
    critical_section_retries_failed: dynamic_timeseries("{}.critical_section_retries_failed", (reponame: String); Average, Sum),
    commits_rebased: dynamic_timeseries("{}.commits_rebased", (reponame: String); Average, Sum, Count),
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
        "Pushrebase validation failed to validate commit {source_cs_id} (rebased to {rebased_cs_id})"
    )]
    ValidationError {
        source_cs_id: ChangesetId,
        rebased_cs_id: ChangesetId,
        #[source]
        err: Error,
    },
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

impl PushrebaseRetryNum {
    fn is_first(&self) -> bool {
        self.0 == 0
    }
}

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
}

pub trait Repo = BonsaiHgMappingRef
    + BookmarksRef
    + RepoBlobstoreArc
    + RepoDerivedDataRef
    + RepoIdentityRef
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
    let head = find_only_head_or_fail(pushed)?;
    let roots = find_roots(pushed);

    let root = find_closest_root(ctx, repo, config, onto_bookmark, &roots).await?;

    let (mut client_cf, client_bcs) = try_join(
        find_changed_files(ctx, repo, root, head),
        fetch_bonsai_range_ancestor_not_included(ctx, repo, root, head),
    )
    .await?;

    client_cf.extend(find_subtree_changes(&client_bcs)?);

    // Normally filenodes (and all other types of derived data) are generated on the first
    // read. However if too many commits are pushed (e.g. when a new repo is merged-in) then
    // first read might be too slow. To prevent that the function below returns an error if too
    // many commits are missing filenodes.
    check_filenodes_backfilled(ctx, repo, &head, config.not_generated_filenodes_limit).await?;

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
        .count_underived::<FilenodesOnlyPublic>(ctx, *head, Some(limit))
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

        let server_bcs = fetch_bonsai_range_ancestor_not_included(
            ctx,
            repo,
            latest_rebase_attempt,
            old_bookmark_value.unwrap_or(root),
        )
        .await?;
        pushrebase_distance = pushrebase_distance.add(server_bcs.len());

        for bcs in server_bcs.iter() {
            if should_fail_pushrebase(bcs) {
                return Err(PushrebaseError::ForceFailPushrebase(bcs.get_changeset_id()));
            }
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

        let mut server_cf = find_changed_files(
            ctx,
            repo,
            latest_rebase_attempt,
            old_bookmark_value.unwrap_or(root),
        )
        .await?;

        server_cf.extend(find_subtree_changes(&server_bcs)?);

        intersect_changed_files(server_cf, client_cf.clone())?;

        let rebase_outcome = do_rebase(
            ctx,
            repo,
            config,
            root,
            head,
            old_bookmark_value,
            onto_bookmark,
            hooks,
            retry_num,
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
    retry_num: PushrebaseRetryNum,
) -> Result<
    Option<(
        ChangesetId,
        BookmarkUpdateLogId,
        Vec<PushrebaseChangesetPair>,
    )>,
    PushrebaseError,
> {
    let (new_head, rebased_changesets) = create_rebased_changesets(
        ctx,
        repo,
        config,
        root,
        head,
        old_bookmark_value.unwrap_or(root),
        &mut hooks,
    )
    .await?;

    for (old_id, (new_id, _)) in &rebased_changesets {
        maybe_validate_commit(ctx, repo, old_id, new_id, retry_num).await?;
    }

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

async fn maybe_validate_commit(
    ctx: &CoreContext,
    repo: &impl Repo,
    old_id: &ChangesetId,
    bcs_id: &ChangesetId,
    retry_num: PushrebaseRetryNum,
) -> Result<(), PushrebaseError> {
    // Validation is expensive, so do it only once
    if !retry_num.is_first() {
        return Ok(());
    }

    let bcs = bcs_id
        .load(ctx, repo.repo_blobstore())
        .map_err(Error::from)
        .await?;
    if !bcs.is_merge() {
        return Ok(());
    }

    // Generate hg changeset to check that this rebased bonsai commit
    // is valid.
    repo.derive_hg_changeset(ctx, *bcs_id)
        .map_err(|err| PushrebaseError::ValidationError {
            source_cs_id: *old_id,
            rebased_cs_id: *bcs_id,
            err,
        })
        .await?;

    // FIXME: it would also be great to do a manifest diff for old_id
    // and rebased bcs_id and check that this diffs are the same.
    // However the caveat here is that we are not sure if diffs are the same
    // in practice - in some cases Mononoke generates hg filenodes
    // that are different from what mercurial would have generated.
    Ok(())
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

    let roots = roots.keys().map(|root| {
        let repo = &repo;

        async move {
            let root_gen = repo
                .commit_graph()
                .changeset_generation(ctx, *root)
                .await
                .map_err(|_| PushrebaseError::from(PushrebaseInternalError::RootNotFound(*root)))?;

            Result::<_, PushrebaseError>::Ok((*root, root_gen))
        }
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
            info!(ctx.logger(), "pushrebase depth: {}", depth);
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
    Ok(
        repo.commit_graph()
            .range_stream(ctx, ancestor, descendant)
            .await?
            .filter(|cs_id| future::ready(cs_id != &ancestor))
            .map(|res| async move {
                Result::<_, Error>::Ok(res.load(ctx, repo.repo_blobstore()).await?)
            })
            .buffered(100)
            .try_collect::<Vec<_>>()
            .await?,
    )
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
        .map(|bcs_id| async move {
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
        .map(|(id, bcs)| {
            let ids = &ids;

            async move {
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
    let mut left = {
        let mut left = left;
        left.sort_unstable();
        left.into_iter()
    };
    let mut right = {
        let mut right = right;
        right.sort_unstable();
        right.into_iter()
    };

    let mut conflicts = Vec::new();
    let mut state = (left.next(), right.next());
    loop {
        state = match state {
            (Some(l), Some(r)) => match l.cmp(&r) {
                Ordering::Equal => {
                    conflicts.push(PushrebaseConflict::new(l.clone(), r.clone()));
                    (left.next(), right.next())
                }
                Ordering::Less => {
                    if l.is_prefix_of(&r) {
                        conflicts.push(PushrebaseConflict::new(l.clone(), r.clone()));
                    }
                    (left.next(), Some(r))
                }
                Ordering::Greater => {
                    if r.is_prefix_of(&l) {
                        conflicts.push(PushrebaseConflict::new(l.clone(), r.clone()));
                    }
                    (Some(l), right.next())
                }
            },
            _ => break,
        };
    }

    if conflicts.is_empty() {
        Ok(())
    } else {
        Err(PushrebaseError::Conflicts(conflicts))
    }
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
) -> Result<(ChangesetId, RebasedChangesets), PushrebaseError> {
    let rebased_set = find_rebased_set(ctx, repo, root, head).await?;

    let rebased_set_ids: HashSet<_> = rebased_set
        .clone()
        .into_iter()
        .map(|cs| cs.get_changeset_id())
        .collect();

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
        let bcs_new = rebase_changeset(
            ctx.clone(),
            bcs_old,
            &remapping,
            date.as_ref(),
            &root,
            &onto,
            repo,
            &rebased_set_ids,
            hooks,
        )
        .await?;
        let timestamp = Timestamp::from(*bcs_new.author_date());
        remapping.insert(id_old, (bcs_new.get_changeset_id(), timestamp));
        rebased.push(bcs_new);
    }

    changesets_creation::save_changesets(ctx, repo, rebased).await?;
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
    ))
}

async fn rebase_changeset(
    ctx: CoreContext,
    bcs: BonsaiChangeset,
    remapping: &HashMap<ChangesetId, (ChangesetId, Timestamp)>,
    timestamp: Option<&Timestamp>,
    root: &ChangesetId,
    onto: &ChangesetId,
    repo: &impl Repo,
    rebased_set: &HashSet<ChangesetId>,
    hooks: &mut [Box<dyn PushrebaseCommitHook>],
) -> Result<BonsaiChangeset> {
    let orig_cs_id = bcs.get_changeset_id();
    let new_file_changes =
        generate_additional_bonsai_file_changes(&ctx, &bcs, root, onto, repo, rebased_set).await?;
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
                .try_filter_map(|(path, _)| async move { Ok(Option::<NonRootMPath>::from(path)) })
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
    use bookmarks::BookmarkTransactionError;
    use bookmarks::Bookmarks;
    use cloned::cloned;
    use commit_graph::CommitGraph;
    use commit_graph::CommitGraphWriter;
    use fbinit::FacebookInit;
    use filestore::FilestoreConfig;
    use filestore::FilestoreConfigRef;
    use fixtures::Linear;
    use fixtures::ManyFilesDirs;
    use fixtures::MergeEven;
    use fixtures::TestRepoFixture;
    use futures::future::TryFutureExt;
    use futures::future::try_join_all;
    use futures::stream;
    use futures::stream::TryStreamExt;
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
        repo: &impl Repo,
        commit_ids: &HashSet<HgChangesetId>,
    ) -> Result<HashSet<BonsaiChangeset>, PushrebaseError> {
        let futs = commit_ids.iter().map(|hg_cs_id| {
            let hg_cs_id = *hg_cs_id;
            async move {
                let bcs_id = repo
                    .bonsai_hg_mapping()
                    .get_bonsai_from_hg(ctx, hg_cs_id)
                    .await?
                    .ok_or_else(|| {
                        Error::from(PushrebaseInternalError::BonsaiNotFoundForHgChangeset(
                            hg_cs_id,
                        ))
                    })?;

                let bcs = bcs_id
                    .load(ctx, repo.repo_blobstore())
                    .await
                    .context("While initial bonsai changesets fetching")?;

                Result::<_, Error>::Ok(bcs)
            }
        });

        let ret = try_join_all(futs).await?.into_iter().collect();
        Ok(ret)
    }

    async fn do_pushrebase(
        ctx: &CoreContext,
        repo: &impl Repo,
        config: &PushrebaseFlags,
        onto_bookmark: &BookmarkKey,
        pushed_set: &HashSet<HgChangesetId>,
    ) -> Result<PushrebaseOutcome, PushrebaseError> {
        let pushed = fetch_bonsai_changesets(ctx, repo, pushed_set).await?;

        let res = do_pushrebase_bonsai(ctx, repo, config, onto_bookmark, &pushed, &[]).await?;

        Ok(res)
    }

    async fn set_bookmark(
        ctx: CoreContext,
        repo: &impl Repo,
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
        repo: &(impl Repo + FilestoreConfigRef),
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
    fn pushrebase_one_commit(fb: FacebookInit) -> Result<(), Error> {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async move {
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
        })
    }

    #[mononoke::fbinit_test]
    fn pushrebase_one_commit_transaction_hook(fb: FacebookInit) -> Result<(), Error> {
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

        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async move {
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
        })
    }

    #[mononoke::fbinit_test]
    fn pushrebase_stack(fb: FacebookInit) -> Result<(), Error> {
        let runtime = tokio::runtime::Runtime::new().unwrap();

        runtime.block_on(async move {
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
        })
    }

    #[mononoke::fbinit_test]
    fn pushrebase_stack_with_renames(fb: FacebookInit) -> Result<(), Error> {
        let runtime = tokio::runtime::Runtime::new().unwrap();

        runtime.block_on(async move {
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
        })
    }

    #[mononoke::fbinit_test]
    fn pushrebase_multi_root(fb: FacebookInit) -> Result<(), Error> {
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
        let runtime = tokio::runtime::Runtime::new().unwrap();

        runtime.block_on(async move {
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
        })
    }

    #[mononoke::fbinit_test]
    fn pushrebase_conflict(fb: FacebookInit) -> Result<(), Error> {
        let runtime = tokio::runtime::Runtime::new().unwrap();

        runtime.block_on(async move {
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
        })
    }

    #[mononoke::fbinit_test]
    fn pushrebase_caseconflicting_rename(fb: FacebookInit) -> Result<(), Error> {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async move {
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
        })
    }

    #[mononoke::fbinit_test]
    fn pushrebase_caseconflicting_dirs(fb: FacebookInit) -> Result<(), Error> {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async move {
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
        })
    }

    #[mononoke::fbinit_test]
    fn pushrebase_recursion_limit(fb: FacebookInit) -> Result<(), Error> {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async move {
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
                .try_fold((root, vec![]), |(head, mut bcss), index| {
                    let ctx = &ctx;
                    let repo = &repo;
                    async move {
                        let file = format!("f{}", index);
                        let content = format!("{}", index);
                        let bcs = CreateCommitContext::new(ctx, &repo, vec![head])
                            .add_file(file.as_str(), content)
                            .commit()
                            .await?;
                        bcss.push(bcs);
                        Result::<_, Error>::Ok((bcs, bcss))
                    }
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
        })
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
    fn pushrebase_case_conflict(fb: FacebookInit) -> Result<(), Error> {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async move {
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
        })
    }
    #[mononoke::fbinit_test]

    fn pushrebase_case_conflict_exclusion(fb: FacebookInit) -> Result<(), Error> {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async move {
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
        })
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

    #[mononoke::fbinit_test]
    fn pushrebase_executable_bit_change(fb: FacebookInit) -> Result<(), Error> {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async move {
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
        })
    }

    async fn count_commits_between(
        ctx: CoreContext,
        repo: &impl Repo,
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
            let us = rand::thread_rng().gen_range(0..100);
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
    fn pushrebase_simultaneously(fb: FacebookInit) -> Result<(), Error> {
        let runtime = tokio::runtime::Runtime::new().unwrap();

        runtime.block_on(async move {
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

            let previous_master =
                HgChangesetId::from_str("a5ffa77602a066db7d5cfb9fb5823a0895717c5a")?;
            let commits_between = count_commits_between(ctx, &repo, previous_master, book).await?;

            // `- 1` because range_stream is inclusive
            assert_eq!(commits_between - 1, num_pushes);

            Ok(())
        })
    }

    #[mononoke::fbinit_test]
    fn pushrebase_create_new_bookmark(fb: FacebookInit) -> Result<(), Error> {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async move {
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
        })
    }

    #[mononoke::fbinit_test]
    fn pushrebase_simultaneously_and_create_new(fb: FacebookInit) -> Result<(), Error> {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async move {
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
        })
    }

    #[mononoke::fbinit_test]
    fn pushrebase_one_commit_with_bundle_id(fb: FacebookInit) -> Result<(), Error> {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async move {
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
        })
    }

    #[mononoke::fbinit_test]
    fn forbid_p2_root_rebases(fb: FacebookInit) -> Result<(), Error> {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async move {
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
        })
    }

    #[mononoke::fbinit_test]
    fn pushrebase_over_merge(fb: FacebookInit) -> Result<(), Error> {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async move {
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
        })
    }

    #[mononoke::fbinit_test]
    fn pushrebase_over_merge_even(fb: FacebookInit) -> Result<(), Error> {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async move {
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
        })
    }

    #[mononoke::fbinit_test]
    fn pushrebase_of_branch_merge(fb: FacebookInit) -> Result<(), Error> {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async move {
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

            let bcs_id_first_merge =
                CreateCommitContext::new(&ctx, &repo, vec![bcs_id_p1, bcs_id_p2])
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
        })
    }

    #[mononoke::fbinit_test]
    fn pushrebase_of_branch_merge_with_removal(fb: FacebookInit) -> Result<(), Error> {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async move {
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
        })
    }

    #[mononoke::fbinit_test]
    fn pushrebase_of_branch_merge_with_rename(fb: FacebookInit) -> Result<(), Error> {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async move {
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
            let bcs_id_pre_master =
                CreateCommitContext::new(&ctx, &repo, vec![bcs_id_pre_pre_master])
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
        })
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
    async fn test_commit_validation(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: PushrebaseTestRepo = test_repo_factory::build_empty(ctx.fb).await?;

        // Pushrebase hook that deletes "base" file from the list of file changes
        struct InvalidPushrebaseHook {}

        #[async_trait]
        impl PushrebaseHook for InvalidPushrebaseHook {
            async fn in_critical_section(
                &self,
                _ctx: &CoreContext,
                _old_bookmark_value: Option<ChangesetId>,
            ) -> Result<Box<dyn PushrebaseCommitHook>, Error> {
                Ok(Box::new(InvalidPushrebaseHook {}))
            }
        }

        #[async_trait]
        impl PushrebaseCommitHook for InvalidPushrebaseHook {
            fn post_rebase_changeset(
                &mut self,
                _bcs_old: ChangesetId,
                bcs_new: &mut BonsaiChangesetMut,
            ) -> Result<(), Error> {
                bcs_new.file_changes.remove(&NonRootMPath::new("base")?);
                Ok(())
            }

            async fn into_transaction_hook(
                self: Box<Self>,
                _ctx: &CoreContext,
                _changesets: &RebasedChangesets,
            ) -> Result<Box<dyn PushrebaseTransactionHook>, Error> {
                Ok(self)
            }
        }

        #[async_trait]
        impl PushrebaseTransactionHook for InvalidPushrebaseHook {
            async fn populate_transaction(
                &self,
                _ctx: &CoreContext,
                txn: Transaction,
            ) -> Result<Transaction, BookmarkTransactionError> {
                Ok(txn)
            }
        }

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
            .add_file("base", "base2")
            .commit()
            .await?;

        bookmark(&ctx, &repo, "master")
            .set_to(bcs_id_master)
            .await?;

        let hook: Box<dyn PushrebaseHook> = Box::new(InvalidPushrebaseHook {});
        let hooks = [hook];

        let bcs_merge = bcs_id_merge.load(&ctx, repo.repo_blobstore()).await?;

        let book = master_bookmark();
        let res = do_pushrebase_bonsai(
            &ctx,
            &repo,
            &Default::default(),
            &book,
            &hashset![bcs_merge.clone()],
            &hooks,
        )
        .await;

        match res {
            Err(PushrebaseError::ValidationError { .. }) => Ok(()),
            Err(err) => Err(err.into()),
            Ok(_) => Err(format_err!("should have failed")),
        }
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
}
