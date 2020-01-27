/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

/// Mononoke pushrebase implementation. The main goal of pushrebase is to decrease push contention.
/// Commits that client pushed are rebased on top of `onto_bookmark` on the server
///
///  Client
///
///     O <- `onto` on client, potentially outdated
///     |
///     O  O <- pushed set (in this case just one commit)
///     | /
///     O <- root
///
///  Server
///
///     O  <- update `onto` bookmark, pointing at the pushed commit
///     |
///     O  <- `onto` bookmark on the server before the push
///     |
///     O
///     |
///     O
///     |
///     O <- root
///
///  Terminology:
///  *onto bookmark* - bookmark that is the destination of the rebase, for example "master"
///
///  *pushed set* - a set of commits that client has sent us.
///  Note: all pushed set MUST be committed before doing pushrebase
///  Note: pushed set MUST contain only one head
///  Note: not all commits from pushed set maybe rebased on top of onto bookmark. See *rebased set*
///
///  *root* - parents of pushed set that are not in the pushed set (see graphs above)
///
///  *rebased set* - subset of pushed set that will be rebased on top of onto bookmark
///  Note: Usually rebased set == pushed set. However in case of merges it may differ
///
/// Pushrebase supports hooks, which can be used to modify rebased Bonsai commits as well as
/// sideload database updates in the transaction that moves forward the bookmark. See hooks.rs for
/// more information on those;
use anyhow::{Context, Error, Result};
use blobrepo::{save_bonsai_changesets, BlobRepo};
use blobrepo_utils::convert_diff_result_into_file_change_for_diamond_merge;
use bookmarks::{BookmarkName, BookmarkUpdateReason, BundleReplayData};
use cloned::cloned;
use context::CoreContext;
use futures::{stream, Future, Stream};
use futures_ext::{BoxFuture, FutureExt as Futures01FutureExt, StreamExt as Futures01StreamExt};
use futures_preview::{
    compat::{Future01CompatExt, Stream01CompatExt},
    future::{try_join, try_join_all, FutureExt, TryFutureExt},
    stream::{StreamExt, TryStream, TryStreamExt},
};
use manifest::{bonsai_diff, BonsaiDiffFileChange, ManifestOps};
use maplit::hashmap;
use mercurial_types::{HgChangesetId, HgFileNodeId, HgManifestId, MPath};
use metaconfig_types::PushrebaseFlags;
use mononoke_types::{
    check_case_conflicts, BonsaiChangeset, ChangesetId, DateTime, FileChange, RawBundle2Id,
    Timestamp,
};
use revset::RangeNodeStream;
use slog::info;
use std::cmp::{max, Ordering};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::iter::FromIterator;
use std::sync::Arc;
use thiserror::Error;

pub use hook::{PushrebaseCommitHook, PushrebaseHook, PushrebaseTransactionHook};

mod hook;

const MAX_REBASE_ATTEMPTS: usize = 100;

pub const MUTATION_KEYS: &[&str] = &["mutpred", "mutuser", "mutdate", "mutop", "mutsplit"];

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Bonsai not found for hg changeset: Z{0:?}")]
    BonsaiNotFoundForHgChangeset(HgChangesetId),
    #[error("Pushrebase onto bookmark not found: {0:?}")]
    PushrebaseBookmarkNotFound(BookmarkName),
    #[error("Only one head is allowed in pushed set")]
    PushrebaseTooManyHeads,
    #[error("No common pushrebase root for {0}, all possible roots: {1:?}")]
    PushrebaseNoCommonRoot(BookmarkName, HashSet<ChangesetId>),
    #[error("Internal error: root changeset {0} not found")]
    RootNotFound(ChangesetId),
    #[error("No pushrebase roots found")]
    NoRoots,
    #[error("Pushrebase failed after too many unsuccessful rebases")]
    TooManyRebaseAttempts,
    #[error("Forbid pushrebase because root ({0}) is not a p1 of {1} bookmark")]
    P2RootRebaseForbidden(HgChangesetId, BookmarkName),
    #[error("internal error: unexpected file conflicts when adding new file changes to {0}")]
    NewFileChangesConflict(ChangesetId),
}

#[derive(Debug, Error)]
pub enum PushrebaseError {
    #[error("Conflicts while pushrebasing: {0:?}")]
    Conflicts(Vec<PushrebaseConflict>),
    #[error("Potential case conflict while pushrebasing: {0:?}")]
    PotentialCaseConflict(MPath),
    #[error("Pushrebase over merge")]
    RebaseOverMerge,
    #[error("Root is too far behind")]
    RootTooFarBehind,
    #[error("Internal error: {0:?}")]
    Error(#[source] Error),
}

type CsIdConvertor =
    Arc<dyn Fn(ChangesetId) -> BoxFuture<HgChangesetId, Error> + Send + Sync + 'static>;
/// Struct that contains data for hg sync replay
#[derive(Clone)]
pub struct HgReplayData {
    // Handle of the bundle2 id that was sent by the client and saved to the blobstore
    bundle2_id: RawBundle2Id,
    // Get hg changeset id from bonsai changeset id. Normally it should just do a simple lookup
    // however it might return other hg changesets if push redirector is used
    cs_id_convertor: CsIdConvertor,
}

impl HgReplayData {
    pub fn new_with_simple_convertor(
        ctx: CoreContext,
        bundle2_id: RawBundle2Id,
        repo: BlobRepo,
    ) -> Self {
        let cs_id_convertor: Arc<
            dyn Fn(ChangesetId) -> BoxFuture<HgChangesetId, Error> + Send + Sync + 'static,
        > = Arc::new({
            cloned!(ctx, repo);
            move |cs_id| {
                repo.get_hg_from_bonsai_changeset(ctx.clone(), cs_id)
                    .boxify()
            }
        });

        Self {
            bundle2_id,
            cs_id_convertor,
        }
    }

    pub fn override_convertor(&mut self, cs_id_convertor: CsIdConvertor) {
        self.cs_id_convertor = cs_id_convertor;
    }

    pub fn get_raw_bundle2_id(&self) -> RawBundle2Id {
        self.bundle2_id.clone()
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PushrebaseConflict {
    left: MPath,
    right: MPath,
}

impl PushrebaseConflict {
    fn new(left: MPath, right: MPath) -> Self {
        PushrebaseConflict { left, right }
    }
}

impl From<Error> for PushrebaseError {
    fn from(error: Error) -> Self {
        PushrebaseError::Error(error)
    }
}

impl From<ErrorKind> for PushrebaseError {
    fn from(error: ErrorKind) -> Self {
        PushrebaseError::Error(error.into())
    }
}

pub type RebasedChangesets = HashMap<ChangesetId, (ChangesetId, Timestamp)>;

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

#[derive(Debug, Clone)]
pub struct PushrebaseSuccessResult {
    pub head: ChangesetId,
    pub retry_num: usize,
    pub rebased_changesets: Vec<PushrebaseChangesetPair>,
}

#[derive(Clone)]
pub struct OntoBookmarkParams {
    pub bookmark: BookmarkName,
}

impl OntoBookmarkParams {
    pub fn new(bookmark: BookmarkName) -> Self {
        Self { bookmark }
    }
}

/// Does a pushrebase of a list of commits `pushed` onto `onto_bookmark`
/// The commits from the pushed set should already be committed to the blobrepo
/// Returns updated bookmark value.
pub async fn do_pushrebase_bonsai(
    ctx: &CoreContext,
    repo: &BlobRepo,
    config: &PushrebaseFlags,
    onto_bookmark: &OntoBookmarkParams,
    pushed: &HashSet<BonsaiChangeset>,
    maybe_hg_replay_data: &Option<HgReplayData>,
    prepushrebase_hooks: &[Box<dyn PushrebaseHook>],
) -> Result<PushrebaseSuccessResult, PushrebaseError> {
    let head = find_only_head_or_fail(&pushed)?;
    let roots = find_roots(&pushed);

    let root = find_closest_root(&ctx, &repo, &config, &onto_bookmark, &roots).await?;

    let (client_cf, client_bcs) = try_join(
        find_changed_files(&ctx, &repo, root, head),
        fetch_bonsai_range(ctx, &repo, root, head),
    )
    .await?;

    backfill_filenodes(
        &ctx,
        &repo,
        pushed.into_iter().filter_map({
            cloned!(client_bcs);
            move |bcs| {
                if !client_bcs.contains(&bcs) {
                    Some(bcs.get_changeset_id())
                } else {
                    None
                }
            }
        }),
    )
    .await?;

    let res = rebase_in_loop(
        ctx,
        repo,
        config,
        onto_bookmark,
        head,
        root,
        client_cf,
        &client_bcs,
        maybe_hg_replay_data,
        prepushrebase_hooks,
    )
    .await?;

    backfill_filenodes(
        &ctx,
        &repo,
        res.rebased_changesets
            .clone()
            .into_iter()
            .filter_map(|pair| {
                if pair.id_old == pair.id_new {
                    Some(pair.id_old.clone())
                } else {
                    None
                }
            }),
    )
    .await?;

    Ok(res)
}

// We have a hack that intentionally doesn't generate filenodes for "pushed" set of commits.
// The reason we have it is the following:
// 1) "pushed" set of commits are draft commits. If we generate filenodes for them then
//    linknodes will point to draft commit
// 2) After the pushrebase new public commits will be created, but they will have the same filenodes
//    which will point to draft commits.
//
// The hack mentioned above solves the problem of having linknodes pointing to draft commits.
// However it creates a new one - in some case (most notably in merges) some of the commits are
// not rebased, and they might miss filenodes completely
//
//   O <- onto
//  ...
//   |  P  <- This commit will be rebased on top of "onto", so new filenodes will be generated
//   | /
//   O
//   | \
//  ... P <- this commit WILL NOT be rebased on top of "onto". That means that some filenodes
//           might be missing.
//
// The function below can be used to backfill filenodes for these commits.
async fn backfill_filenodes<'a>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    to_backfill: impl IntoIterator<Item = ChangesetId>,
) -> Result<(), Error> {
    let mut futs = vec![];

    for bcs_id in to_backfill {
        let fut = async move {
            let hg_cs_id_fut = repo
                .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id)
                .compat();
            let bcs_fut = repo.get_bonsai_changeset(ctx.clone(), bcs_id).compat();
            let (hg_cs_id, bcs) = try_join(hg_cs_id_fut, bcs_fut).await?;

            let parents = bcs.parents().map(|p| id_to_manifestid(&ctx, &repo, p));

            let parent_mfs = try_join_all(parents).await?;

            let (_, incomplete_filenodes) = repo
                .get_manifest_from_bonsai(ctx.clone(), bcs, parent_mfs)
                .compat()
                .await?;

            incomplete_filenodes
                .upload(ctx.clone(), hg_cs_id, &repo)
                .compat()
                .await
        };

        futs.push(fut);
    }

    try_join_all(futs)
        .await
        .context("While backfilling filenodes")?;

    Ok(())
}

async fn rebase_in_loop(
    ctx: &CoreContext,
    repo: &BlobRepo,
    config: &PushrebaseFlags,
    onto_bookmark: &OntoBookmarkParams,
    head: ChangesetId,
    root: ChangesetId,
    client_cf: Vec<MPath>,
    client_bcs: &Vec<BonsaiChangeset>,
    maybe_hg_replay_data: &Option<HgReplayData>,
    prepushrebase_hooks: &[Box<dyn PushrebaseHook>],
) -> Result<PushrebaseSuccessResult, PushrebaseError> {
    let mut latest_rebase_attempt = root;

    for retry_num in 0..MAX_REBASE_ATTEMPTS {
        let hooks = try_join_all(
            prepushrebase_hooks
                .iter()
                .map(|h| h.prepushrebase().map_err(PushrebaseError::from)),
        );

        let (hooks, bookmark_val) =
            try_join(hooks, get_onto_bookmark_value(&ctx, &repo, &onto_bookmark)).await?;

        let server_bcs = fetch_bonsai_range(
            &ctx,
            &repo,
            latest_rebase_attempt,
            bookmark_val.unwrap_or(root),
        )
        .await?;

        if config.casefolding_check {
            let conflict =
                check_case_conflicts(server_bcs.iter().rev().chain(client_bcs.iter().rev()));
            if let Some(conflict) = conflict {
                return Err(PushrebaseError::PotentialCaseConflict(conflict));
            }
        }

        let server_cf = find_changed_files(
            &ctx,
            &repo,
            latest_rebase_attempt,
            bookmark_val.unwrap_or(root),
        )
        .await?;

        // TODO: Avoid this clone
        intersect_changed_files(server_cf, client_cf.clone())?;

        let rebase_outcome = do_rebase(
            &ctx,
            &repo,
            &config,
            root,
            head,
            bookmark_val,
            &onto_bookmark,
            maybe_hg_replay_data,
            hooks,
        )
        .await?;

        if let Some((head, rebased_changesets)) = rebase_outcome {
            let res = PushrebaseSuccessResult {
                head,
                retry_num,
                rebased_changesets,
            };
            return Ok(res);
        }

        latest_rebase_attempt = bookmark_val.unwrap_or(root);
    }

    Err(ErrorKind::TooManyRebaseAttempts.into())
}

async fn do_rebase(
    ctx: &CoreContext,
    repo: &BlobRepo,
    config: &PushrebaseFlags,
    root: ChangesetId,
    head: ChangesetId,
    bookmark_val: Option<ChangesetId>,
    onto_bookmark: &OntoBookmarkParams,
    maybe_hg_replay_data: &Option<HgReplayData>,
    mut hooks: Vec<Box<dyn PushrebaseCommitHook>>,
) -> Result<Option<(ChangesetId, Vec<PushrebaseChangesetPair>)>, PushrebaseError> {
    let (new_head, rebased_changesets) = create_rebased_changesets(
        &ctx,
        &repo,
        config,
        root,
        head,
        bookmark_val.unwrap_or(root),
        &mut hooks,
    )
    .await?;

    let hooks = hooks
        .into_iter()
        .map(|h| h.into_transaction_hook(&rebased_changesets))
        .collect::<Result<Vec<_>, Error>>()?;

    try_move_bookmark(
        ctx.clone(),
        &repo,
        &onto_bookmark,
        bookmark_val,
        new_head,
        &maybe_hg_replay_data,
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
            ErrorKind::PushrebaseTooManyHeads.into(),
        ))
    }
}

/// Reperesents index of current child with regards to its parent
#[derive(Clone, Copy, PartialEq, Eq)]
struct ChildIndex(usize);

fn find_roots(commits: &HashSet<BonsaiChangeset>) -> HashMap<ChangesetId, ChildIndex> {
    let commits_set: HashSet<_> =
        HashSet::from_iter(commits.iter().map(|commit| commit.get_changeset_id()));
    let mut roots = HashMap::new();
    for commit in commits {
        for (index, parent) in commit.parents().enumerate() {
            if !commits_set.contains(&parent) {
                let ChildIndex(ref mut max_index) =
                    roots.entry(parent.clone()).or_insert(ChildIndex(0));
                *max_index = max(index, *max_index);
            }
        }
    }
    roots
}

async fn find_closest_root(
    ctx: &CoreContext,
    repo: &BlobRepo,
    config: &PushrebaseFlags,
    bookmark: &OntoBookmarkParams,
    roots: &HashMap<ChangesetId, ChildIndex>,
) -> Result<ChangesetId, PushrebaseError> {
    let maybe_id = get_bookmark_value(&ctx, &repo, &bookmark.bookmark).await?;

    if let Some(id) = maybe_id {
        return find_closest_ancestor_root(&ctx, &repo, &config, &bookmark.bookmark, &roots, id)
            .await;
    }

    let roots = roots.iter().map(|(root, _)| {
        let repo = &repo;

        async move {
            let gen_num = repo
                .get_generation_number_by_bonsai(ctx.clone(), *root)
                .compat()
                .await?
                .ok_or(PushrebaseError::from(ErrorKind::RootNotFound(*root)))?;

            Result::<_, PushrebaseError>::Ok((*root, gen_num))
        }
    });

    let roots = try_join_all(roots).await?;

    let (cs_id, _) = roots
        .into_iter()
        .max_by_key(|(_, gen_num)| gen_num.clone())
        .ok_or(PushrebaseError::from(ErrorKind::NoRoots))?;

    Ok(cs_id)
}

async fn find_closest_ancestor_root(
    ctx: &CoreContext,
    repo: &BlobRepo,
    config: &PushrebaseFlags,
    bookmark: &BookmarkName,
    roots: &HashMap<ChangesetId, ChildIndex>,
    onto_bookmark_cs_id: ChangesetId,
) -> Result<ChangesetId, PushrebaseError> {
    let mut queue = VecDeque::new();
    queue.push_back(onto_bookmark_cs_id);

    let mut queued = HashSet::new();
    let mut depth = 0;

    loop {
        if depth > 0 && depth % 1000 == 0 {
            info!(ctx.logger(), "pushrebase recursion depth: {}", depth);
        }

        if let Some(recursion_limit) = config.recursion_limit {
            if depth >= recursion_limit {
                return Err(PushrebaseError::RootTooFarBehind);
            }
        }

        depth += 1;

        let id = queue.pop_front().ok_or_else(|| {
            PushrebaseError::Error(
                ErrorKind::PushrebaseNoCommonRoot(
                    bookmark.clone(),
                    roots.keys().cloned().collect(),
                )
                .into(),
            )
        })?;

        if let Some(index) = roots.get(&id) {
            if config.forbid_p2_root_rebases && *index != ChildIndex(0) {
                let hgcs = repo
                    .get_hg_from_bonsai_changeset(ctx.clone(), id)
                    .compat()
                    .await?;

                return Err(PushrebaseError::Error(
                    ErrorKind::P2RootRebaseForbidden(hgcs, bookmark.clone()).into(),
                ));
            }

            return Ok(id);
        }

        let parents = repo
            .get_changeset_parents_by_bonsai(ctx.clone(), id)
            .compat()
            .await?;

        queue.extend(parents.into_iter().filter(|p| queued.insert(*p)));
    }
}

/// find changed files by comparing manifests of `ancestor` and `descendant`
async fn find_changed_files_between_manifests(
    ctx: &CoreContext,
    repo: &BlobRepo,
    ancestor: ChangesetId,
    descendant: ChangesetId,
) -> Result<Vec<MPath>, PushrebaseError> {
    let paths = find_bonsai_diff(ctx, repo, ancestor, descendant)
        .await?
        .map_ok(|diff| match diff {
            BonsaiDiffFileChange::Changed(path, ..)
            | BonsaiDiffFileChange::ChangedReusedId(path, ..)
            | BonsaiDiffFileChange::Deleted(path) => path,
        })
        .try_collect()
        .await?;

    Ok(paths)
}

async fn find_bonsai_diff(
    ctx: &CoreContext,
    repo: &BlobRepo,
    ancestor: ChangesetId,
    descendant: ChangesetId,
) -> Result<impl TryStream<Ok = BonsaiDiffFileChange<HgFileNodeId>, Error = Error>> {
    let (d_mf, a_mf) = try_join(
        id_to_manifestid(&ctx, &repo, descendant),
        id_to_manifestid(&ctx, &repo, ancestor),
    )
    .await?;

    let stream = bonsai_diff(
        ctx.clone(),
        repo.get_blobstore(),
        d_mf,
        Some(a_mf).into_iter().collect(),
    )
    .compat();

    Ok(stream)
}

async fn id_to_manifestid(
    ctx: &CoreContext,
    repo: &BlobRepo,
    bcs_id: ChangesetId,
) -> Result<HgManifestId, Error> {
    let hg_cs_id = repo
        .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id)
        .compat()
        .await?;
    let hg_cs = repo
        .get_changeset_by_changesetid(ctx.clone(), hg_cs_id)
        .compat()
        .await?;
    Ok(hg_cs.manifestid())
}

// from larger generation number to smaller
async fn fetch_bonsai_range(
    ctx: &CoreContext,
    repo: &BlobRepo,
    ancestor: ChangesetId,
    descendant: ChangesetId,
) -> Result<Vec<BonsaiChangeset>, PushrebaseError> {
    let stream = RangeNodeStream::new(
        ctx.clone(),
        repo.get_changeset_fetcher(),
        ancestor,
        descendant,
    )
    .compat();

    let nodes = stream
        .map(|res| {
            async move {
                match res {
                    Ok(bcs_id) => {
                        repo.get_bonsai_changeset(ctx.clone(), bcs_id)
                            .compat()
                            .await
                    }
                    Err(e) => Err(e),
                }
            }
        })
        .buffered(100)
        .try_collect::<Vec<_>>()
        .await?;

    Ok(nodes)
}

async fn find_changed_files(
    ctx: &CoreContext,
    repo: &BlobRepo,
    ancestor: ChangesetId,
    descendant: ChangesetId,
) -> Result<Vec<MPath>, PushrebaseError> {
    let stream = RangeNodeStream::new(
        ctx.clone(),
        repo.get_changeset_fetcher(),
        ancestor,
        descendant,
    )
    .compat();

    let id_to_bcs = stream
        .map(|res| {
            async move {
                match res {
                    Ok(bcs_id) => {
                        let bcs = repo
                            .get_bonsai_changeset(ctx.clone(), bcs_id)
                            .compat()
                            .await?;

                        Ok((bcs_id, bcs))
                    }
                    Err(e) => Err(e),
                }
            }
        })
        .buffered(100)
        .try_collect::<HashMap<_, _>>()
        .await?;

    let ids: HashSet<_> = id_to_bcs.iter().map(|(id, _)| *id).collect();

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
                                find_changed_files_between_manifests(&ctx, &repo, id, *p_id).await
                            }
                            (None, None) => panic!(
                                "`RangeNodeStream` produced invalid result for: ({}, {})",
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
                    .flat_map(|v| v)
                    .collect::<HashSet<_>>()  // compute union
                    .into_iter()
                    .collect::<Vec<_>>();
    file_changes_union.sort_unstable();

    Ok(file_changes_union)
}

fn extract_conflict_files_from_bonsai_changeset(bcs: BonsaiChangeset) -> Vec<MPath> {
    bcs.file_changes()
        .map(|(path, maybe_file_change)| {
            let mut v = vec![];
            if let Some(file_change) = maybe_file_change {
                if let Some((copy_from_path, _)) = file_change.copy_from() {
                    v.push(copy_from_path.clone());
                }
            }
            v.push(path.clone());
            v.into_iter()
        })
        .flatten()
        .collect::<Vec<MPath>>()
}

/// `left` and `right` are considerered to be conflit free, if none of the element from `left`
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

/// Returns Some(ChangesetId) if bookmarks exists.
/// Returns None if bookmarks does not exists
async fn get_onto_bookmark_value(
    ctx: &CoreContext,
    repo: &BlobRepo,
    onto_bookmark: &OntoBookmarkParams,
) -> Result<Option<ChangesetId>, PushrebaseError> {
    let maybe_cs_id = get_bookmark_value(&ctx, &repo, &onto_bookmark.bookmark).await?;

    Ok(maybe_cs_id)
}

async fn get_bookmark_value(
    ctx: &CoreContext,
    repo: &BlobRepo,
    bookmark_name: &BookmarkName,
) -> Result<Option<ChangesetId>, PushrebaseError> {
    let maybe_cs_id = repo
        .get_bonsai_bookmark(ctx.clone(), bookmark_name)
        .compat()
        .await?;

    Ok(maybe_cs_id)
}

async fn create_rebased_changesets(
    ctx: &CoreContext,
    repo: &BlobRepo,
    config: &PushrebaseFlags,
    root: ChangesetId,
    head: ChangesetId,
    onto: ChangesetId,
    hooks: &mut [Box<dyn PushrebaseCommitHook>],
) -> Result<(ChangesetId, RebasedChangesets), PushrebaseError> {
    let rebased_set = find_rebased_set(&ctx, &repo, root, head).await?;

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
            &repo,
            &rebased_set_ids,
            hooks,
        )
        .await?;
        let timestamp = Timestamp::from(*bcs_new.author_date());
        remapping.insert(id_old, (bcs_new.get_changeset_id(), timestamp));
        rebased.push(bcs_new);
    }

    save_bonsai_changesets(rebased, ctx.clone(), repo.clone())
        .map(move |_| {
            (
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
            )
        })
        .from_err()
        .compat()
        .await
}

async fn rebase_changeset(
    ctx: CoreContext, // TODO
    bcs: BonsaiChangeset,
    remapping: &HashMap<ChangesetId, (ChangesetId, Timestamp)>,
    timestamp: Option<&Timestamp>,
    root: &ChangesetId,
    onto: &ChangesetId,
    repo: &BlobRepo,
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
            let tz_offset_secs = bcs.author_date.tz_offset_secs();
            let newdate = DateTime::from_timestamp(timestamp.timestamp_seconds(), tz_offset_secs)?;
            bcs.author_date = newdate;
        }
        None => (),
    }

    // Mutation information from the original commit must be stripped.
    for key in MUTATION_KEYS {
        bcs.extra.remove(*key);
    }

    // Copy information in bonsai changeset contains a commit parent. So parent changes, then
    // copy information for all copied/moved files needs to be updated
    let mut file_changes: BTreeMap<_, _> = bcs
        .file_changes
        .into_iter()
        .map(|(path, file_change_opt)| {
            (
                path,
                file_change_opt.map(|file_change| {
                    FileChange::new(
                        file_change.content_id().clone(),
                        file_change.file_type(),
                        file_change.size(),
                        file_change.copy_from().map(|(path, cs)| {
                            (
                                path.clone(),
                                remapping.get(cs).map(|(cs, _)| cs).cloned().unwrap_or(*cs),
                            )
                        }),
                    )
                }),
            )
        })
        .collect();

    let new_file_paths: HashSet<_> =
        HashSet::from_iter(new_file_changes.iter().map(|(path, _)| path));
    for (path, _) in &file_changes {
        if new_file_paths.contains(path) {
            return Err(ErrorKind::NewFileChangesConflict(orig_cs_id).into());
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
// and bcs (in the example above one of the file changes will be the file change for "file.txt").
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
    repo: &BlobRepo,
    rebased_set: &HashSet<ChangesetId>,
) -> Result<Vec<(MPath, Option<FileChange>)>> {
    let cs_id = bcs.get_changeset_id();
    let parents: Vec<_> = bcs.parents().collect();

    if parents.len() > 1 && parents.iter().any(|p| !rebased_set.contains(p)) {
        let bonsai_diff = find_bonsai_diff(&ctx, &repo, *root, *onto)
            .await?
            .try_collect::<Vec<_>>()
            .await?;

        let mf_id = id_to_manifestid(ctx, repo, cs_id).await?;

        let mut paths = vec![];
        for res in &bonsai_diff {
            match res {
                BonsaiDiffFileChange::Changed(path, ..)
                | BonsaiDiffFileChange::ChangedReusedId(path, ..) => {
                    paths.push(path.clone());
                }
                BonsaiDiffFileChange::Deleted(path) => {
                    paths.push(path.clone());
                }
            }
        }

        // If a file is not present in the `cs_id`, then no need to add it to the new_file_changes.
        // This is done in order to not add unnecessary file changes if they are guaranteed to
        // not have conflicts.
        // Consider the following case:
        //
        // o <- onto
        // |
        // A  <- adds file.txt
        // |
        // |   C <-  commit C doesn't have file.txt, so no conflicts possible after pushrebase
        // | / |
        // o   D
        // | /
        // B
        //
        let stale_entries = mf_id
            .find_entries(ctx.clone(), repo.get_blobstore(), paths)
            .filter_map(|(path, _)| path)
            .collect_to::<HashSet<_>>()
            .compat()
            .await?;

        let mut new_file_changes = vec![];
        for res in bonsai_diff {
            match res {
                BonsaiDiffFileChange::Changed(ref path, ..)
                | BonsaiDiffFileChange::ChangedReusedId(ref path, ..)
                | BonsaiDiffFileChange::Deleted(ref path) => {
                    if !stale_entries.contains(path) {
                        continue;
                    }
                }
            }

            new_file_changes.push(convert_diff_result_into_file_change_for_diamond_merge(
                ctx.clone(),
                &repo,
                res,
            ));
        }

        stream::futures_unordered(new_file_changes)
            .collect()
            .compat()
            .await
    } else {
        Ok(vec![])
    }
}

// Order - from lowest generation number to highest
async fn find_rebased_set(
    ctx: &CoreContext,
    repo: &BlobRepo,
    root: ChangesetId,
    head: ChangesetId,
) -> Result<Vec<BonsaiChangeset>, PushrebaseError> {
    let nodes = fetch_bonsai_range(&ctx, &repo, root, head).await?;

    let nodes = nodes
        .into_iter()
        .filter(|node| node.get_changeset_id() != root)
        .rev()
        .collect();

    Ok(nodes)
}

async fn try_move_bookmark(
    ctx: CoreContext,
    repo: &BlobRepo,
    bookmark: &OntoBookmarkParams,
    old_value: Option<ChangesetId>,
    new_value: ChangesetId,
    maybe_hg_replay_data: &Option<HgReplayData>,
    rebased_changesets: RebasedChangesets,
    hooks: Vec<Box<dyn PushrebaseTransactionHook>>,
) -> Result<Option<(ChangesetId, Vec<PushrebaseChangesetPair>)>, PushrebaseError> {
    let bookmark_name = &bookmark.bookmark;
    let mut txn = repo.update_bookmark_transaction(ctx);

    let reason = create_bookmark_update_reason(maybe_hg_replay_data, &rebased_changesets).await?;

    match old_value {
        Some(old_value) => {
            txn.update(&bookmark_name, new_value, old_value, reason)?;
        }
        None => {
            txn.create(&bookmark_name, new_value, reason)?;
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
            .compat()
            .boxify()
    };

    let success = txn
        .commit_with_hook(Arc::new(sql_txn_hook))
        .compat()
        .await?;

    let ret = if success {
        Some((new_value, rebased_changesets_into_pairs(rebased_changesets)))
    } else {
        None
    };

    Ok(ret)
}

async fn create_bookmark_update_reason(
    maybe_hg_replay_data: &Option<HgReplayData>,
    rebased_changesets: &RebasedChangesets,
) -> Result<BookmarkUpdateReason, Error> {
    let hg_replay_data = match maybe_hg_replay_data {
        Some(hg_replay_data) => hg_replay_data,
        None => {
            return Ok(BookmarkUpdateReason::Pushrebase {
                bundle_replay_data: None,
            });
        }
    };

    let HgReplayData {
        bundle2_id,
        cs_id_convertor,
    } = hg_replay_data;

    let bundle_replay_data = BundleReplayData::new(*bundle2_id);

    let timestamps = rebased_changesets.iter().map({
        |(id_old, (_, timestamp))| {
            async move {
                let hg_cs_id = cs_id_convertor(*id_old).compat().await?;
                Result::<_, Error>::Ok((hg_cs_id, *timestamp))
            }
        }
    });

    let timestamps = try_join_all(timestamps).await?.into_iter().collect();

    let reason = BookmarkUpdateReason::Pushrebase {
        bundle_replay_data: Some(bundle_replay_data.with_timestamps(timestamps)),
    };

    Ok(reason)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::format_err;
    use async_trait::async_trait;
    use blobrepo::DangerousOverride;
    use bookmarks::{BookmarkTransactionError, Bookmarks};
    use dbbookmarks::SqlBookmarks;
    use fbinit::FacebookInit;
    use fixtures::{linear, many_files_dirs, merge_even};
    use futures_preview::{
        compat::Future01CompatExt,
        future::{try_join_all, TryFutureExt},
        stream::{self, TryStreamExt},
    };
    use manifest::{Entry, ManifestOps};
    use maplit::{btreemap, hashmap, hashset};
    use mononoke_types::FileType;
    use mononoke_types::{BonsaiChangesetMut, RepositoryId};
    use mononoke_types_mocks::hash::AS;
    use mutable_counters::{MutableCounters, SqlMutableCounters};
    use sql::{rusqlite::Connection as SqliteConnection, Connection, Transaction};
    use sql_ext::{SqlConstructors, TransactionResult};
    use std::{collections::BTreeMap, str::FromStr};
    use tests_utils::{bookmark, resolve_cs_id, CreateCommitContext};

    async fn fetch_bonsai_changesets(
        ctx: &CoreContext,
        repo: &BlobRepo,
        commit_ids: &HashSet<HgChangesetId>,
    ) -> Result<HashSet<BonsaiChangeset>, PushrebaseError> {
        let futs = commit_ids.iter().map(|hg_cs_id| {
            let hg_cs_id = *hg_cs_id;
            async move {
                let bcs_id = repo
                    .get_bonsai_from_hg(ctx.clone(), hg_cs_id)
                    .compat()
                    .await?
                    .ok_or(Error::from(ErrorKind::BonsaiNotFoundForHgChangeset(
                        hg_cs_id,
                    )))?;

                let bcs = repo
                    .get_bonsai_changeset(ctx.clone(), bcs_id)
                    .compat()
                    .await
                    .context("While intitial bonsai changesets fetching")?;

                Result::<_, Error>::Ok(bcs)
            }
        });

        let ret = try_join_all(futs).await?.into_iter().collect();
        Ok(ret)
    }

    async fn do_pushrebase(
        ctx: &CoreContext,
        repo: &BlobRepo,
        config: &PushrebaseFlags,
        onto_bookmark: &OntoBookmarkParams,
        pushed_set: &HashSet<HgChangesetId>,
        maybe_hg_replay_data: &Option<HgReplayData>,
    ) -> Result<PushrebaseSuccessResult, PushrebaseError> {
        let pushed = fetch_bonsai_changesets(&ctx, &repo, &pushed_set).await?;

        let res = do_pushrebase_bonsai(
            &ctx,
            &repo,
            &config,
            &onto_bookmark,
            &pushed,
            &maybe_hg_replay_data,
            &vec![],
        )
        .await?;

        Ok(res)
    }

    async fn set_bookmark(
        ctx: CoreContext,
        repo: BlobRepo,
        book: &BookmarkName,
        cs_id: &str,
    ) -> Result<(), Error> {
        let head = HgChangesetId::from_str(cs_id)?;
        let head = repo
            .get_bonsai_from_hg(ctx.clone(), head)
            .compat()
            .await?
            .ok_or(Error::msg(format_err!("Head not found: {:?}", cs_id)))?;

        let mut txn = repo.update_bookmark_transaction(ctx);
        txn.force_set(
            &book,
            head,
            BookmarkUpdateReason::TestMove {
                bundle_replay_data: None,
            },
        )?;
        txn.commit().compat().await?;
        Ok(())
    }

    fn make_paths(paths: &[&str]) -> Vec<MPath> {
        let paths: Result<_, _> = paths.into_iter().map(MPath::new).collect();
        paths.unwrap()
    }

    fn master_bookmark() -> OntoBookmarkParams {
        let book = BookmarkName::new("master").unwrap();
        let book = OntoBookmarkParams::new(book);
        book
    }

    async fn push_and_verify(
        ctx: &CoreContext,
        repo: &BlobRepo,
        parent: ChangesetId,
        bookmark: &OntoBookmarkParams,
        content: BTreeMap<&str, Option<&str>>,
        should_succeed: bool,
    ) -> Result<(), Error> {
        let mut commit_ctx = CreateCommitContext::new(&ctx, &repo, vec![parent]);

        for (path, maybe_content) in content.iter() {
            let path: &str = path.as_ref();
            commit_ctx = match maybe_content {
                Some(content) => commit_ctx.add_file(path, content),
                None => commit_ctx.delete_file(path),
            };
        }

        let cs_id = commit_ctx.commit().await?;

        let hgcss = hashset![
            repo.get_hg_from_bonsai_changeset(ctx.clone(), cs_id)
                .compat()
                .await?
        ];

        let res = do_pushrebase(
            &ctx,
            &repo,
            &PushrebaseFlags::default(),
            &bookmark,
            &hgcss,
            &None,
        )
        .await;

        if should_succeed {
            assert!(res.is_ok());
        } else {
            should_have_conflicts(res);
        }

        Ok(())
    }

    #[fbinit::test]
    fn pushrebase_one_commit(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();
        runtime.block_on(
            async move {
                let ctx = CoreContext::test_mock(fb);
                let repo = linear::getrepo(fb);
                // Bottom commit of the repo
                let parents = vec!["2d7d4ba9ce0a6ffd222de7785b249ead9c51c536"];
                let bcs_id = CreateCommitContext::new(&ctx, &repo, parents)
                    .add_file("file", "content")
                    .commit()
                    .await?;

                let hg_cs = repo
                    .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id)
                    .compat()
                    .await?;

                let book = master_bookmark();
                bookmark(&ctx, &repo, book.bookmark.clone())
                    .set_to("a5ffa77602a066db7d5cfb9fb5823a0895717c5a")
                    .await?;

                do_pushrebase(
                    &ctx,
                    &repo,
                    &Default::default(),
                    &book,
                    &hashset![hg_cs],
                    &None,
                )
                .map_err(|err| format_err!("{:?}", err))
                .await?;
                Ok(())
            }
                .boxed()
                .compat(),
        )
    }

    // Initializes bookmarks and mutable_counters on the "same db" i.e. on the same
    // sqlite connection
    async fn init_bookmarks_mutable_counters(
    ) -> Result<(Arc<dyn Bookmarks>, Arc<SqlMutableCounters>), Error> {
        let con = SqliteConnection::open_in_memory()?;
        con.execute_batch(SqlMutableCounters::get_up_query())?;
        con.execute_batch(SqlBookmarks::get_up_query())?;

        let con = Connection::with_sqlite(con);
        let bookmarks = Arc::new(SqlBookmarks::from_connections(
            con.clone(),
            con.clone(),
            con.clone(),
        )) as Arc<dyn Bookmarks>;
        let mutable_counters = Arc::new(SqlMutableCounters::from_connections(
            con.clone(),
            con.clone(),
            con.clone(),
        ));

        Ok((bookmarks, mutable_counters))
    }

    #[fbinit::test]
    fn pushrebase_one_commit_transaction_hook(fb: FacebookInit) -> Result<(), Error> {
        #[derive(Copy, Clone)]
        struct Hook(RepositoryId);

        #[async_trait]
        impl PushrebaseHook for Hook {
            async fn prepushrebase(&self) -> Result<Box<dyn PushrebaseCommitHook>, Error> {
                Ok(Box::new(*self) as Box<dyn PushrebaseCommitHook>)
            }
        }

        impl PushrebaseCommitHook for Hook {
            fn post_rebase_changeset(
                &mut self,
                _bcs_old: ChangesetId,
                _bcs_new: &mut BonsaiChangesetMut,
            ) -> Result<(), Error> {
                Ok(())
            }

            fn into_transaction_hook(
                self: Box<Self>,
                changesets: &RebasedChangesets,
            ) -> Result<Box<dyn PushrebaseTransactionHook>, Error> {
                let (_, (cs_id, _)) = changesets
                    .into_iter()
                    .next()
                    .ok_or(Error::msg("No rebased changeset"))?;
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
                    SqlMutableCounters::set_counter_on_txn(ctx.clone(), self.0, &key, 1, None, txn)
                        .compat()
                        .await?;

                match ret {
                    TransactionResult::Succeeded(txn) => Ok(txn),
                    TransactionResult::Failed => Err(Error::msg("Did not update").into()),
                }
            }
        }

        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();
        runtime.block_on(
            async move {
                let ctx = CoreContext::test_mock(fb);
                let repo = linear::getrepo(fb);
                // Bottom commit of the repo
                let parents = vec!["2d7d4ba9ce0a6ffd222de7785b249ead9c51c536"];
                let bcs_id = CreateCommitContext::new(&ctx, &repo, parents)
                    .add_file("file", "content")
                    .commit()
                    .await?;

                let (bookmarks, mutable_counters) = init_bookmarks_mutable_counters().await?;
                let repo = repo.dangerous_override(|_| bookmarks);

                let bcs = repo
                    .get_bonsai_changeset(ctx.clone(), bcs_id)
                    .compat()
                    .await?;

                let repoid = repo.get_repoid();
                let mut book = master_bookmark();

                bookmark(&ctx, &repo, book.bookmark.clone())
                    .set_to("a5ffa77602a066db7d5cfb9fb5823a0895717c5a")
                    .await?;

                let hook: Box<dyn PushrebaseHook> = Box::new(Hook(repo.get_repoid()));
                let hooks = vec![hook];

                do_pushrebase_bonsai(
                    &ctx,
                    &repo,
                    &Default::default(),
                    &book,
                    &hashset![bcs.clone()],
                    &None,
                    &hooks[..],
                )
                .map_err(|err| format_err!("{:?}", err))
                .await?;

                let master_val = resolve_cs_id(&ctx, &repo, "master").await?;
                let key = format!("{}", master_val);
                assert_eq!(
                    mutable_counters
                        .get_counter(ctx.clone(), repoid, &key)
                        .compat()
                        .await?,
                    Some(1),
                );

                // Now do the same with another non-existent bookmark,
                // make sure cs id is created.
                book.bookmark = BookmarkName::new("newbook")?;
                do_pushrebase_bonsai(
                    &ctx,
                    &repo,
                    &Default::default(),
                    &book,
                    &hashset![bcs],
                    &None,
                    &hooks[..],
                )
                .map_err(|err| format_err!("{:?}", err))
                .await?;

                let key = format!("{}", resolve_cs_id(&ctx, &repo, "newbook").await?);
                assert_eq!(
                    mutable_counters
                        .get_counter(ctx.clone(), repoid, &key)
                        .compat()
                        .await?,
                    Some(1),
                );
                Ok(())
            }
                .boxed()
                .compat(),
        )
    }

    #[fbinit::test]
    fn pushrebase_stack(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();

        runtime.block_on(
            async move {
                let ctx = CoreContext::test_mock(fb);
                let repo = linear::getrepo(fb);
                // Bottom commit of the repo
                let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
                let p = repo
                    .get_bonsai_from_hg(ctx.clone(), root)
                    .compat()
                    .await?
                    .ok_or(Error::msg("Root is missing"))?;
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
                    repo.clone(),
                    &book.bookmark,
                    "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
                )
                .await?;

                let hg_cs_1 = repo
                    .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_1)
                    .compat()
                    .await?;
                let hg_cs_2 = repo
                    .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_2)
                    .compat()
                    .await?;
                do_pushrebase(
                    &ctx,
                    &repo,
                    &Default::default(),
                    &book,
                    &hashset![hg_cs_1, hg_cs_2],
                    &None,
                )
                .await?;
                Ok(())
            }
                .boxed()
                .compat(),
        )
    }

    #[fbinit::test]
    fn pushrebase_stack_with_renames(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();

        runtime.block_on(
            async move {
                let ctx = CoreContext::test_mock(fb);
                let repo = linear::getrepo(fb);
                // Bottom commit of the repo
                let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
                let p = repo
                    .get_bonsai_from_hg(ctx.clone(), root)
                    .compat()
                    .await?
                    .ok_or(Error::msg("p is missing"))?;
                let bcs_id_1 = CreateCommitContext::new(&ctx, &repo, vec![p])
                    .add_file("file", "content")
                    .commit()
                    .await?;
                let bcs_id_2 = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_1])
                    .add_file_with_copy_info("file_renamed", "content", (bcs_id_1, "file"))?
                    .commit()
                    .await?;

                assert_eq!(
                    find_changed_files(&ctx, &repo, p, bcs_id_2).await?,
                    make_paths(&["file", "file_renamed"]),
                );

                let book = master_bookmark();
                set_bookmark(
                    ctx.clone(),
                    repo.clone(),
                    &book.bookmark,
                    "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
                )
                .await?;

                let hg_cs_1 = repo
                    .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_1)
                    .compat()
                    .await?;
                let hg_cs_2 = repo
                    .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_2)
                    .compat()
                    .await?;
                do_pushrebase(
                    &ctx,
                    &repo,
                    &Default::default(),
                    &book,
                    &hashset![hg_cs_1, hg_cs_2],
                    &None,
                )
                .await?;

                Ok(())
            }
                .boxed()
                .compat(),
        )
    }

    #[fbinit::test]
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
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();

        runtime.block_on(
            async move {
                let ctx = CoreContext::test_mock(fb);
                let repo = linear::getrepo(fb);
                let config = PushrebaseFlags::default();

                let root0 = repo
                    .get_bonsai_from_hg(
                        ctx.clone(),
                        HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?,
                    )
                    .compat()
                    .await?
                    .ok_or(Error::msg("root0 is missing"))?;

                let root1 = repo
                    .get_bonsai_from_hg(
                        ctx.clone(),
                        HgChangesetId::from_str("607314ef579bd2407752361ba1b0c1729d08b281")?,
                    )
                    .compat()
                    .await?
                    .ok_or(Error::msg("root0 is missing"))?;

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
                    repo.clone(),
                    &book.bookmark,
                    "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
                )
                .await?;
                let bcs_id_master = repo
                    .get_bonsai_from_hg(
                        ctx.clone(),
                        HgChangesetId::from_str("a5ffa77602a066db7d5cfb9fb5823a0895717c5a")?,
                    )
                    .compat()
                    .await?
                    .ok_or(Error::msg("bcs_id_master is missing"))?;

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

                let hg_cs_1 = repo
                    .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_1)
                    .compat()
                    .await?;
                let hg_cs_2 = repo
                    .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_2)
                    .compat()
                    .await?;
                let hg_cs_3 = repo
                    .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_3)
                    .compat()
                    .await?;
                let bcs_id_rebased = do_pushrebase(
                    &ctx,
                    &repo,
                    &config,
                    &book,
                    &hashset![hg_cs_1, hg_cs_2, hg_cs_3],
                    &None,
                )
                .await?;

                // should only rebase {bcs2, bcs3}
                let rebased =
                    find_rebased_set(&ctx, &repo, bcs_id_master, bcs_id_rebased.head).await?;
                assert_eq!(rebased.len(), 2);
                let bcs2 = &rebased[0];
                let bcs3 = &rebased[1];

                // bcs3 parent correctly updated and contains only {bcs2}
                assert_eq!(
                    bcs3.parents().collect::<Vec<_>>(),
                    vec![bcs2.get_changeset_id()]
                );

                // bcs2 parents cotains old bcs1 and old master bookmark
                assert_eq!(
                    bcs2.parents().collect::<HashSet<_>>(),
                    hashset! { bcs_id_1, bcs_id_master },
                );
                Ok(())
            }
                .boxed()
                .compat(),
        )
    }

    #[fbinit::test]
    fn pushrebase_conflict(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();

        runtime.block_on(
            async move {
                let ctx = CoreContext::test_mock(fb);
                let repo = linear::getrepo(fb);
                let root = repo
                    .get_bonsai_from_hg(
                        ctx.clone(),
                        HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?,
                    )
                    .compat()
                    .await?
                    .ok_or(Error::msg("Root is missing"))?;

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
                    repo.clone(),
                    &book.bookmark,
                    "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
                )
                .await?;

                let hg_cs_1 = repo
                    .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_1)
                    .compat()
                    .await?;
                let hg_cs_2 = repo
                    .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_2)
                    .compat()
                    .await?;
                let hg_cs_3 = repo
                    .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_3)
                    .compat()
                    .await?;
                let result = do_pushrebase(
                    &ctx,
                    &repo,
                    &Default::default(),
                    &book,
                    &hashset![hg_cs_1, hg_cs_2, hg_cs_3],
                    &None,
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
                .boxed()
                .compat(),
        )
    }

    #[fbinit::test]
    fn pushrebase_caseconflicting_rename(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();
        runtime.block_on(
            async move {
                let ctx = CoreContext::test_mock(fb);
                let repo = linear::getrepo(fb);
                let root = repo
                    .get_bonsai_from_hg(
                        ctx.clone(),
                        HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?,
                    )
                    .compat()
                    .await?
                    .ok_or(Error::msg("Root is missing"))?;

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
                    repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_1)
                        .compat()
                        .await?,
                    repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_2)
                        .compat()
                        .await?,
                    repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_3)
                        .compat()
                        .await?,
                ];

                let book = master_bookmark();
                set_bookmark(
                    ctx.clone(),
                    repo.clone(),
                    &book.bookmark,
                    "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
                )
                .await?;

                do_pushrebase(&ctx, &repo, &Default::default(), &book, &hgcss, &None).await?;

                Ok(())
            }
                .boxed()
                .compat(),
        )
    }

    #[fbinit::test]
    fn pushrebase_caseconflicting_dirs(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();
        runtime.block_on(
            async move {
                let ctx = CoreContext::test_mock(fb);
                let repo = linear::getrepo(fb);
                let root = repo
                    .get_bonsai_from_hg(
                        ctx.clone(),
                        HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?,
                    )
                    .compat()
                    .await?
                    .ok_or(Error::msg("Root is missing"))?;

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
                    repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_1)
                        .compat()
                        .await?,
                    repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_2)
                        .compat()
                        .await?,
                ];

                let book = master_bookmark();
                set_bookmark(
                    ctx.clone(),
                    repo.clone(),
                    &book.bookmark,
                    "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
                )
                .await?;

                do_pushrebase(&ctx, &repo, &Default::default(), &book, &hgcss, &None).await?;

                Ok(())
            }
                .boxed()
                .compat(),
        )
    }

    #[fbinit::test]
    fn pushrebase_recursion_limit(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();
        runtime.block_on(
            async move {
                let ctx = CoreContext::test_mock(fb);
                let repo = linear::getrepo(fb);
                let root = repo
                    .get_bonsai_from_hg(
                        ctx.clone(),
                        HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?,
                    )
                    .compat()
                    .await?
                    .ok_or(Error::msg("Root is missing"))?;

                // create a lot of commits
                let (_, bcss) = stream::iter((0..128usize).map(Ok))
                    .try_fold((root, vec![]), |(head, mut bcss), index| {
                        let ctx = &ctx;
                        let repo = &repo;
                        async move {
                            let file = format!("f{}", index);
                            let content = format!("{}", index);
                            let bcs = CreateCommitContext::new(&ctx, &repo, vec![head])
                                .add_file(&file, content)
                                .commit()
                                .await?;
                            bcss.push(bcs);
                            Result::<_, Error>::Ok((bcs, bcss))
                        }
                    })
                    .await?;

                let hgcss = try_join_all(bcss.iter().map(|bcs| {
                    repo.get_hg_from_bonsai_changeset(ctx.clone(), *bcs)
                        .compat()
                }))
                .await?;
                let book = master_bookmark();
                set_bookmark(
                    ctx.clone(),
                    repo.clone(),
                    &book.bookmark,
                    "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
                )
                .await?;
                do_pushrebase(
                    &ctx,
                    &repo,
                    &Default::default(),
                    &book.clone(),
                    &hgcss.into_iter().collect(),
                    &None,
                )
                .await?;

                let bcs = CreateCommitContext::new(&ctx, &repo, vec![root])
                    .add_file("file", "data")
                    .commit()
                    .await?;

                let hgcss = hashset![
                    repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs)
                        .compat()
                        .await?
                ];

                // try rebase with small recursion limit
                let config = PushrebaseFlags {
                    recursion_limit: Some(128),
                    ..Default::default()
                };
                let result = do_pushrebase(&ctx, &repo, &config, &book, &hgcss, &None).await;
                match result {
                    Err(PushrebaseError::RootTooFarBehind) => (),
                    _ => panic!("push-rebase should have failed because root too far behind"),
                }

                let config = PushrebaseFlags {
                    recursion_limit: Some(256),
                    ..Default::default()
                };
                do_pushrebase(&ctx, &repo, &config, &book, &hgcss, &None).await?;

                Ok(())
            }
                .boxed()
                .compat(),
        )
    }

    #[fbinit::test]
    fn pushrebase_rewritedates(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();
        runtime.block_on(
            async move {
                let ctx = CoreContext::test_mock(fb);
                let repo = linear::getrepo(fb);
                let root = repo
                    .get_bonsai_from_hg(
                        ctx.clone(),
                        HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?,
                    )
                    .compat()
                    .await?
                    .ok_or(Error::msg("Root is missing"))?;
                let book = master_bookmark();
                let bcs = CreateCommitContext::new(&ctx, &repo, vec![root])
                    .add_file("file", "data")
                    .commit()
                    .await?;
                let hgcss = hashset![
                    repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs)
                        .compat()
                        .await?
                ];

                set_bookmark(
                    ctx.clone(),
                    repo.clone(),
                    &book.bookmark,
                    "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
                )
                .await?;
                let config = PushrebaseFlags {
                    rewritedates: false,
                    ..Default::default()
                };
                let bcs_keep_date =
                    do_pushrebase(&ctx, &repo, &config, &book, &hgcss, &None).await?;

                set_bookmark(
                    ctx.clone(),
                    repo.clone(),
                    &book.bookmark,
                    "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
                )
                .await?;
                let config = PushrebaseFlags {
                    rewritedates: true,
                    ..Default::default()
                };
                let bcs_rewrite_date =
                    do_pushrebase(&ctx, &repo, &config, &book, &hgcss, &None).await?;

                let bcs = repo.get_bonsai_changeset(ctx.clone(), bcs).compat().await?;
                let bcs_keep_date = repo
                    .get_bonsai_changeset(ctx.clone(), bcs_keep_date.head)
                    .compat()
                    .await?;
                let bcs_rewrite_date = repo
                    .get_bonsai_changeset(ctx.clone(), bcs_rewrite_date.head)
                    .compat()
                    .await?;

                assert_eq!(bcs.author_date(), bcs_keep_date.author_date());
                assert!(bcs.author_date() < bcs_rewrite_date.author_date());

                Ok(())
            }
                .boxed()
                .compat(),
        )
    }

    #[fbinit::test]
    fn pushrebase_case_conflict(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();
        runtime.block_on(
            async move {
                let ctx = CoreContext::test_mock(fb);
                let repo = many_files_dirs::getrepo(fb);
                let root = repo
                    .get_bonsai_from_hg(
                        ctx.clone(),
                        HgChangesetId::from_str("5a28e25f924a5d209b82ce0713d8d83e68982bc8")?,
                    )
                    .compat()
                    .await?
                    .ok_or(Error::msg("Root is missing"))?;

                let bcs = CreateCommitContext::new(&ctx, &repo, vec![root])
                    .add_file("Dir1/file_1_in_dir1", "data")
                    .commit()
                    .await?;

                let hgcss = hashset![
                    repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs)
                        .compat()
                        .await?
                ];

                let book = master_bookmark();
                set_bookmark(
                    ctx.clone(),
                    repo.clone(),
                    &book.bookmark,
                    "2f866e7e549760934e31bf0420a873f65100ad63",
                )
                .await?;

                let result =
                    do_pushrebase(&ctx, &repo, &Default::default(), &book, &hgcss, &None).await;
                match result {
                    Err(PushrebaseError::PotentialCaseConflict(conflict)) => {
                        assert_eq!(conflict, MPath::new("Dir1/file_1_in_dir1")?)
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
                    &None,
                )
                .await?;

                Ok(())
            }
                .boxed()
                .compat(),
        )
    }

    #[test]
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

    #[fbinit::test]
    fn pushrebase_executable_bit_change(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();
        runtime.block_on(
            async move {
                let ctx = CoreContext::test_mock(fb);
                let repo = linear::getrepo(fb);
                let path_1 = MPath::new("1")?;

                let root_hg = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
                let root_cs = repo
                    .get_changeset_by_changesetid(ctx.clone(), root_hg)
                    .compat()
                    .await?;
                let root_1_id = repo
                    .find_files_in_manifest(ctx.clone(), root_cs.manifestid(), vec![path_1.clone()])
                    .compat()
                    .await?
                    .get(&path_1)
                    .copied()
                    .ok_or(Error::msg("path_1 missing in manifest"))?;

                // crate filechange with with same content as "1" but set executable bit
                let root = repo
                    .get_bonsai_from_hg(ctx.clone(), root_hg)
                    .compat()
                    .await?
                    .ok_or(Error::msg("Root missing"))?;
                let root_bcs = repo
                    .get_bonsai_changeset(ctx.clone(), root)
                    .compat()
                    .await?;
                let file_1 = root_bcs
                    .file_changes()
                    .find(|(path, _)| path == &&path_1)
                    .ok_or(Error::msg("path_1 missing in file_changes"))?
                    .1
                    .ok_or(Error::msg("path_1 change info missing"))?
                    .clone();
                assert_eq!(file_1.file_type(), FileType::Regular);
                let file_1_exec = FileChange::new(
                    file_1.content_id(),
                    FileType::Executable,
                    file_1.size(),
                    /* copy_from */ None,
                );

                let bcs = CreateCommitContext::new(&ctx, &repo, vec![root])
                    .add_file_change(path_1.to_string(), file_1_exec.clone())
                    .commit()
                    .await?;

                let hgcss = hashset![
                    repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs)
                        .compat()
                        .await?
                ];

                let book = master_bookmark();
                set_bookmark(
                    ctx.clone(),
                    repo.clone(),
                    &book.bookmark,
                    "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
                )
                .await?;

                let result =
                    do_pushrebase(&ctx, &repo, &Default::default(), &book, &hgcss, &None).await?;
                let result_bcs = repo
                    .get_bonsai_changeset(ctx.clone(), result.head)
                    .compat()
                    .await?;
                let file_1_result = result_bcs
                    .file_changes()
                    .find(|(path, _)| path == &&path_1)
                    .ok_or(Error::msg("path_1 missing in file_changes"))?
                    .1
                    .ok_or(Error::msg("path_1 change info missing"))?;
                assert_eq!(file_1_result, &file_1_exec);

                let result_hg = repo
                    .get_hg_from_bonsai_changeset(ctx.clone(), result.head)
                    .compat()
                    .await?;
                let result_cs = repo
                    .get_changeset_by_changesetid(ctx.clone(), result_hg)
                    .compat()
                    .await?;
                let result_1_id = repo
                    .find_files_in_manifest(
                        ctx.clone(),
                        result_cs.manifestid(),
                        vec![path_1.clone()],
                    )
                    .compat()
                    .await?
                    .get(&path_1)
                    .copied()
                    .ok_or(Error::msg("path_1 mising in manifest"))?;

                // `result_1_id` should be equal to `root_1_id`, because executable flag
                // is not a part of file envelope
                assert_eq!(root_1_id, result_1_id);

                Ok(())
            }
                .boxed()
                .compat(),
        )
    }

    async fn count_commits_between(
        ctx: CoreContext,
        repo: BlobRepo,
        ancestor: HgChangesetId,
        descendant: BookmarkName,
    ) -> Result<usize, Error> {
        let ancestor = repo
            .get_bonsai_from_hg(ctx.clone(), ancestor)
            .compat()
            .await?
            .ok_or(Error::msg("ancestor not found"))?;

        let descendant = repo
            .get_bookmark(ctx.clone(), &descendant)
            .compat()
            .await?
            .ok_or(Error::msg("bookmark not found"))?;

        let descendant = repo
            .get_bonsai_from_hg(ctx.clone(), descendant)
            .compat()
            .await?
            .ok_or(Error::msg("bonsai not found"))?;

        let n = RangeNodeStream::new(ctx, repo.get_changeset_fetcher(), ancestor, descendant)
            .compat()
            .try_collect::<Vec<_>>()
            .await?
            .len();

        Ok(n)
    }

    #[fbinit::test]
    fn pushrebase_simultaneously(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();

        runtime.block_on(
            async move {
                let ctx = CoreContext::test_mock(fb);
                let repo = linear::getrepo(fb);
                // Bottom commit of the repo
                let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
                let p = repo
                    .get_bonsai_from_hg(ctx.clone(), root)
                    .compat()
                    .await?
                    .ok_or(Error::msg("Root is missing"))?;
                let parents = vec![p];

                let book = master_bookmark();
                set_bookmark(
                    ctx.clone(),
                    repo.clone(),
                    &book.bookmark,
                    "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
                )
                .await?;

                let num_pushes = 10;
                let mut futs = vec![];
                for i in 0..num_pushes {
                    let f = format!("file{}", i);
                    let bcs_id = CreateCommitContext::new(&ctx, &repo, parents.clone())
                        .add_file(&f, "content")
                        .commit()
                        .await?;
                    let hg_cs = repo
                        .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id)
                        .compat()
                        .await?;

                    let fut = tokio::task::spawn({
                        cloned!(ctx, repo, book, hg_cs);
                        async move {
                            do_pushrebase(
                                &ctx,
                                &repo,
                                &Default::default(),
                                &book,
                                &hashset![hg_cs],
                                &None,
                            )
                            .await
                        }
                            .boxed()
                    });

                    futs.push(fut);
                }

                let res = try_join_all(futs).await?;
                let mut has_retry_num_bigger_1 = false;
                for r in res {
                    if r?.retry_num > 1 {
                        has_retry_num_bigger_1 = true;
                    }
                }

                assert!(has_retry_num_bigger_1);

                let previous_master =
                    HgChangesetId::from_str("a5ffa77602a066db7d5cfb9fb5823a0895717c5a")?;
                let commits_between =
                    count_commits_between(ctx, repo, previous_master, book.bookmark).await?;

                // `- 1` because RangeNodeStream is inclusive
                assert_eq!(commits_between - 1, num_pushes);

                Ok(())
            }
                .boxed()
                .compat(),
        )
    }

    #[fbinit::test]
    fn pushrebase_create_new_bookmark(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();
        runtime.block_on(
            async move {
                let ctx = CoreContext::test_mock(fb);
                let repo = linear::getrepo(fb);
                // Bottom commit of the repo
                let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
                let p = repo
                    .get_bonsai_from_hg(ctx.clone(), root)
                    .compat()
                    .await?
                    .ok_or(Error::msg("Root is missing"))?;
                let parents = vec![p];

                let bcs_id = CreateCommitContext::new(&ctx, &repo, parents)
                    .add_file("file", "content")
                    .commit()
                    .await?;

                let hg_cs = repo
                    .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id)
                    .compat()
                    .await?;

                let book = BookmarkName::new("newbook")?;
                let book = OntoBookmarkParams::new(book);
                do_pushrebase(
                    &ctx,
                    &repo,
                    &Default::default(),
                    &book,
                    &hashset![hg_cs],
                    &None,
                )
                .await?;
                Ok(())
            }
                .boxed()
                .compat(),
        )
    }

    #[fbinit::test]
    fn pushrebase_simultaneously_and_create_new(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();
        runtime.block_on(
            async move {
                let ctx = CoreContext::test_mock(fb);
                let repo = linear::getrepo(fb);
                // Bottom commit of the repo
                let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
                let p = repo
                    .get_bonsai_from_hg(ctx.clone(), root)
                    .compat()
                    .await?
                    .ok_or(Error::msg("Root is missing"))?;
                let parents = vec![p];

                let book = BookmarkName::new("newbook")?;
                let book = OntoBookmarkParams::new(book);

                let num_pushes = 10;
                let mut futs = vec![];
                for i in 0..num_pushes {
                    let f = format!("file{}", i);
                    let bcs_id = CreateCommitContext::new(&ctx, &repo, parents.clone())
                        .add_file(&f, "content")
                        .commit()
                        .await?;
                    let hg_cs = repo
                        .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id)
                        .compat()
                        .await?;

                    let fut = tokio::task::spawn({
                        cloned!(ctx, repo, book, hg_cs);
                        async move {
                            do_pushrebase(
                                &ctx,
                                &repo,
                                &Default::default(),
                                &book,
                                &hashset![hg_cs],
                                &None,
                            )
                            .await
                        }
                            .boxed()
                    });

                    futs.push(fut);
                }

                let res = try_join_all(futs).await?;
                let mut has_retry_num_bigger_1 = false;
                for r in res {
                    if r?.retry_num > 1 {
                        has_retry_num_bigger_1 = true;
                    }
                }

                assert!(has_retry_num_bigger_1);

                let commits_between = count_commits_between(ctx, repo, root, book.bookmark).await?;
                // `- 1` because RangeNodeStream is inclusive
                assert_eq!(commits_between - 1, num_pushes);

                Ok(())
            }
                .boxed()
                .compat(),
        )
    }

    #[fbinit::test]
    fn pushrebase_one_commit_with_bundle_id(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();
        runtime.block_on(
            async move {
                let ctx = CoreContext::test_mock(fb);
                let repo = linear::getrepo(fb);
                // Bottom commit of the repo
                let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
                let p = repo
                    .get_bonsai_from_hg(ctx.clone(), root)
                    .compat()
                    .await?
                    .ok_or(Error::msg("Root is missing"))?;
                let parents = vec![p];

                let bcs_id = CreateCommitContext::new(&ctx, &repo, parents)
                    .add_file("file", "content")
                    .commit()
                    .await?;
                let hg_cs = repo
                    .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id)
                    .compat()
                    .await?;

                let book = master_bookmark();
                set_bookmark(
                    ctx.clone(),
                    repo.clone(),
                    &book.bookmark,
                    "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
                )
                .await?;

                do_pushrebase(
                    &ctx,
                    &repo,
                    &Default::default(),
                    &book,
                    &hashset![hg_cs],
                    &Some(HgReplayData::new_with_simple_convertor(
                        ctx.clone(),
                        RawBundle2Id::new(AS),
                        repo.clone(),
                    )),
                )
                .await?;

                Ok(())
            }
                .boxed()
                .compat(),
        )
    }

    #[fbinit::test]
    fn pushrebase_timezone(fb: FacebookInit) -> Result<(), Error> {
        // We shouldn't change timezone even if timestamp changes

        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();
        runtime.block_on(
            async move {
                let ctx = CoreContext::test_mock(fb);
                let repo = linear::getrepo(fb);
                // Bottom commit of the repo
                let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
                let p = repo
                    .get_bonsai_from_hg(ctx.clone(), root)
                    .compat()
                    .await?
                    .ok_or(Error::msg("Root is missing"))?;
                let parents = vec![p];

                let tz_offset_secs = 100;
                let bcs_id = CreateCommitContext::new(&ctx, &repo, parents)
                    .add_file("file", "content")
                    .set_author_date(DateTime::from_timestamp(0, 100)?)
                    .commit()
                    .await?;
                let hg_cs = repo
                    .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id)
                    .compat()
                    .await?;

                let book = master_bookmark();
                set_bookmark(
                    ctx.clone(),
                    repo.clone(),
                    &book.bookmark,
                    "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
                )
                .await?;

                let config = PushrebaseFlags {
                    rewritedates: true,
                    ..Default::default()
                };
                let bcs_rewrite_date = do_pushrebase(
                    &ctx,
                    &repo,
                    &config,
                    &book,
                    &hashset![hg_cs],
                    &Some(HgReplayData::new_with_simple_convertor(
                        ctx.clone(),
                        RawBundle2Id::new(AS),
                        repo.clone(),
                    )),
                )
                .await?;

                let bcs_rewrite_date = repo
                    .get_bonsai_changeset(ctx.clone(), bcs_rewrite_date.head)
                    .compat()
                    .await?;

                assert_eq!(
                    bcs_rewrite_date.author_date().tz_offset_secs(),
                    tz_offset_secs
                );

                Ok(())
            }
                .boxed()
                .compat(),
        )
    }

    #[fbinit::test]
    fn forbid_p2_root_rebases(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();
        runtime.block_on(
            async move {
                let ctx = CoreContext::test_mock(fb);
                let repo = linear::getrepo(fb);

                let root = repo
                    .get_bonsai_from_hg(
                        ctx.clone(),
                        HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?,
                    )
                    .compat()
                    .await?
                    .ok_or(Error::msg("Root is missing"))?;

                let bcs_id_0 = CreateCommitContext::new_root(&ctx, &repo)
                    .add_file("merge_file", "merge content")
                    .commit()
                    .await?;
                let bcs_id_1 = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_0, root])
                    .add_file("file", "content")
                    .commit()
                    .await?;
                let hgcss = hashset![
                    repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_0)
                        .compat()
                        .await?,
                    repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_1)
                        .compat()
                        .await?,
                ];

                let book = master_bookmark();
                set_bookmark(
                    ctx.clone(),
                    repo.clone(),
                    &book.bookmark,
                    "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
                )
                .await?;

                let config_forbid_p2 = PushrebaseFlags {
                    forbid_p2_root_rebases: true,
                    ..Default::default()
                };

                assert!(
                    do_pushrebase(&ctx, &repo, &config_forbid_p2, &book, &hgcss, &None,)
                        .await
                        .is_err()
                );

                let config_allow_p2 = PushrebaseFlags {
                    forbid_p2_root_rebases: false,
                    ..Default::default()
                };

                do_pushrebase(&ctx, &repo, &config_allow_p2, &book, &hgcss, &None).await?;

                Ok(())
            }
                .boxed()
                .compat(),
        )
    }

    #[fbinit::test]
    fn pushrebase_over_merge(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();
        runtime.block_on(
            async move {
                let ctx = CoreContext::test_mock(fb);
                let repo = blobrepo_factory::new_memblob_empty(None)?;

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

                let merge_hg_cs_id = repo
                    .get_hg_from_bonsai_changeset(ctx.clone(), merge)
                    .compat()
                    .await?;

                set_bookmark(ctx.clone(), repo.clone(), &book.bookmark, &{
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
                .boxed()
                .compat(),
        )
    }

    #[fbinit::test]
    fn pushrebase_over_merge_even(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();
        runtime.block_on(
            async move {
                let ctx = CoreContext::test_mock(fb);
                let repo = merge_even::getrepo(fb);

                // 4dcf230cd2f20577cb3e88ba52b73b376a2b3f69 - is a merge commit,
                // 3cda5c78aa35f0f5b09780d971197b51cad4613a is one of the ancestors
                let root = repo
                    .get_bonsai_from_hg(
                        ctx.clone(),
                        HgChangesetId::from_str("3cda5c78aa35f0f5b09780d971197b51cad4613a")?,
                    )
                    .compat()
                    .await?
                    .ok_or(Error::msg("Root is missing"))?;

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

                let hgcss = hashset![
                    repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_should_fail)
                        .compat()
                        .await?
                ];

                let res = do_pushrebase(
                    &ctx,
                    &repo,
                    &PushrebaseFlags::default(),
                    &book,
                    &hgcss,
                    &None,
                )
                .await;

                should_have_conflicts(res);
                let hgcss = hashset![
                    repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_should_succeed)
                        .compat()
                        .await?,
                ];

                do_pushrebase(
                    &ctx,
                    &repo,
                    &PushrebaseFlags::default(),
                    &book,
                    &hgcss,
                    &None,
                )
                .await?;

                Ok(())
            }
                .boxed()
                .compat(),
        )
    }

    #[fbinit::test]
    fn pushrebase_of_branch_merge(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();
        runtime.block_on(
            async move {
                let ctx = CoreContext::test_mock(fb);
                let repo = blobrepo_factory::new_memblob_empty(None)?;

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

                let hg_cs = repo
                    .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_master)
                    .compat()
                    .await?;

                let book = master_bookmark();
                set_bookmark(ctx.clone(), repo.clone(), &book.bookmark, &{
                    // https://github.com/rust-lang/rust/pull/64856
                    let r = format!("{}", hg_cs);
                    r
                })
                .await?;

                let hgcss = hashset![
                    repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_first_merge)
                        .compat()
                        .await?,
                    repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_second_merge)
                        .compat()
                        .await?,
                ];

                do_pushrebase(
                    &ctx,
                    &repo,
                    &PushrebaseFlags::default(),
                    &book,
                    &hgcss,
                    &None,
                )
                .await?;

                let new_master = get_bookmark_value(&ctx, &repo, &BookmarkName::new("master")?)
                    .await?
                    .ok_or(Error::msg("master not set"))?;

                let master_hg = repo
                    .get_hg_from_bonsai_changeset(ctx.clone(), new_master)
                    .compat()
                    .await?;

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
                .boxed()
                .compat(),
        )
    }

    #[fbinit::test]
    fn pushrebase_of_branch_merge_with_removal(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();
        runtime.block_on(
            async move {
                let ctx = CoreContext::test_mock(fb);
                let repo = blobrepo_factory::new_memblob_empty(None)?;

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

                let bcs_id_merge =
                    CreateCommitContext::new(&ctx, &repo, vec![bcs_id_p1, bcs_id_p2])
                        .add_file("merge", "merge")
                        .commit()
                        .await?;

                // Modify base file again
                let bcs_id_master = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_p1])
                    .delete_file("base")
                    .add_file("anotherfile", "anotherfile")
                    .commit()
                    .await?;

                let hg_cs = repo
                    .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_master)
                    .compat()
                    .await?;

                let book = master_bookmark();
                set_bookmark(ctx.clone(), repo.clone(), &book.bookmark, &{
                    // https://github.com/rust-lang/rust/pull/64856
                    let r = format!("{}", hg_cs);
                    r
                })
                .await?;

                let hgcss = hashset![
                    repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_merge)
                        .compat()
                        .await?,
                ];

                do_pushrebase(
                    &ctx,
                    &repo,
                    &PushrebaseFlags::default(),
                    &book,
                    &hgcss,
                    &None,
                )
                .await?;

                let new_master = get_bookmark_value(&ctx, &repo, &BookmarkName::new("master")?)
                    .await?
                    .ok_or(Error::msg("master not set"))?;

                let master_hg = repo
                    .get_hg_from_bonsai_changeset(ctx.clone(), new_master)
                    .compat()
                    .await?;

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
                .boxed()
                .compat(),
        )
    }

    #[fbinit::test]
    fn pushrebase_of_branch_merge_with_rename(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();
        runtime.block_on(
            async move {
                let ctx = CoreContext::test_mock(fb);
                let repo = blobrepo_factory::new_memblob_empty(None)?;

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

                let bcs_id_merge =
                    CreateCommitContext::new(&ctx, &repo, vec![bcs_id_p1, bcs_id_p2])
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
                        .add_file_with_copy_info(
                            "base",
                            "somecontent",
                            (bcs_id_pre_pre_master, "p1"),
                        )?
                        .commit()
                        .await?;

                let bcs_id_master = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_pre_master])
                    .add_file("somefile", "somecontent")
                    .commit()
                    .await?;

                let hg_cs = repo
                    .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_master)
                    .compat()
                    .await?;

                let book = master_bookmark();
                set_bookmark(ctx.clone(), repo.clone(), &book.bookmark, &{
                    // https://github.com/rust-lang/rust/pull/64856
                    let r = format!("{}", hg_cs);
                    r
                })
                .await?;

                let hgcss = hashset![
                    repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_merge)
                        .compat()
                        .await?
                ];

                do_pushrebase(
                    &ctx,
                    &repo,
                    &PushrebaseFlags::default(),
                    &book,
                    &hgcss,
                    &None,
                )
                .await?;

                let new_master =
                    get_bookmark_value(&ctx, &repo.clone(), &BookmarkName::new("master")?)
                        .await?
                        .ok_or(Error::msg("master is not set"))?;

                let master_hg = repo
                    .get_hg_from_bonsai_changeset(ctx.clone(), new_master)
                    .compat()
                    .await?;

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
                .boxed()
                .compat(),
        )
    }

    async fn ensure_content(
        ctx: &CoreContext,
        hg_cs_id: HgChangesetId,
        repo: &BlobRepo,
        expected: BTreeMap<String, String>,
    ) -> Result<(), Error> {
        let cs = repo
            .get_changeset_by_changesetid(ctx.clone(), hg_cs_id)
            .compat()
            .await?;

        let entries = cs
            .manifestid()
            .list_all_entries(ctx.clone(), repo.get_blobstore())
            .compat()
            .try_collect::<Vec<_>>()
            .await?;

        let mut actual = BTreeMap::new();
        for (path, entry) in entries {
            match entry {
                Entry::Leaf((_, filenode_id)) => {
                    let content = repo
                        .get_file_content(ctx.clone(), filenode_id)
                        .compat()
                        .try_concat()
                        .await?;
                    let s = String::from_utf8_lossy(content.as_bytes()).into_owned();
                    actual.insert(format!("{}", path.unwrap()), s);
                }
                Entry::Tree(_) => {}
            }
        }

        assert_eq!(expected, actual);

        Ok(())
    }

    fn should_have_conflicts(res: Result<PushrebaseSuccessResult, PushrebaseError>) {
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
