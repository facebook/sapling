// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![cfg_attr(test, type_length_limit = "2097152")]
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
use blobrepo::{save_bonsai_changesets, BlobRepo};
use bonsai_utils::{bonsai_diff, BonsaiDiffResult};
use bookmarks::{BookmarkName, BookmarkUpdateReason, BundleReplayData};
use cloned::cloned;
use context::CoreContext;
use failure::{Error, Fail};
use failure_ext::{FutureFailureErrorExt, Result};
use futures::future::{err, join_all, loop_fn, ok, Loop};
use futures::{Future, IntoFuture, Stream};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use maplit::hashmap;
use mercurial_types::{Changeset, HgChangesetId, MPath};
use metaconfig_types::PushrebaseParams;
use mononoke_types::{
    check_case_conflicts, BonsaiChangeset, ChangesetId, DateTime, FileChange, RawBundle2Id,
    Timestamp,
};

use revset::RangeNodeStream;
use std::cmp::{max, Ordering};
use std::collections::{HashMap, HashSet, VecDeque};
use std::iter::FromIterator;

const MAX_REBASE_ATTEMPTS: usize = 100;

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "Bonsai not found for hg changeset: {:?}", _0)]
    BonsaiNotFoundForHgChangeset(HgChangesetId),
    #[fail(display = "Pushrebase onto bookmark not found: {:?}", _0)]
    PushrebaseBookmarkNotFound(BookmarkName),
    #[fail(display = "Only one head is allowed in pushed set")]
    PushrebaseTooManyHeads,
    #[fail(
        display = "Error while uploading data for changesets, hashes: {:?}",
        _0
    )]
    PushrebaseNoCommonRoot(BookmarkName, HashSet<ChangesetId>),
    #[fail(display = "Internal error: root changeset {} not found", _0)]
    RootNotFound(ChangesetId),
    #[fail(display = "No pushrebase roots found")]
    NoRoots,
    #[fail(display = "Pushrebase failed after too many unsuccessful rebases")]
    TooManyRebaseAttempts,
    #[fail(
        display = "Forbid pushrebase because root ({}) is not a p1 of {} bookmark",
        _0, _1
    )]
    P2RootRebaseForbidden(HgChangesetId, BookmarkName),
}

#[derive(Debug)]
pub enum PushrebaseError {
    Conflicts(Vec<PushrebaseConflict>),
    PotentialCaseConflict(MPath),
    RebaseOverMerge,
    RootTooFarBehind,
    Error(Error),
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

type RebasedChangesets = HashMap<ChangesetId, (ChangesetId, Timestamp)>;

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

pub struct PushrebaseSuccessResult {
    pub head: ChangesetId,
    pub retry_num: usize,
    pub rebased_changesets: Vec<PushrebaseChangesetPair>,
}

#[derive(Clone)]
pub struct OntoBookmarkParams {
    pub bookmark: BookmarkName,
}

/// Does a pushrebase of a list of commits `pushed_set` onto `onto_bookmark`
/// The commits from the pushed set should already be committed to the blobrepo
/// Returns updated bookmark value.
pub fn do_pushrebase(
    ctx: CoreContext,
    repo: BlobRepo,
    config: PushrebaseParams,
    onto_bookmark: OntoBookmarkParams,
    pushed_set: Vec<HgChangesetId>,
    maybe_raw_bundle2_id: Option<RawBundle2Id>,
) -> impl Future<Item = PushrebaseSuccessResult, Error = PushrebaseError> {
    fetch_bonsai_changesets(ctx.clone(), repo.clone(), pushed_set)
        .and_then(|pushed| {
            let head = find_only_head_or_fail(&pushed)?;
            let roots = find_roots(&pushed)?;

            Ok((head, roots))
        })
        .and_then({
            cloned!(ctx, config, repo, onto_bookmark);
            move |(head, roots)| {
                find_closest_root(ctx, &repo, config, onto_bookmark, roots)
                    .map(move |root| (head, root))
            }
        })
        .and_then({
            cloned!(repo);
            move |(head, root)| {
                // Calculate client changed files only once, since they won't change
                (
                    find_changed_files(
                        ctx.clone(),
                        &repo,
                        root,
                        head,
                        /* reject_merges */ false,
                    ),
                    fetch_bonsai_range(ctx.clone(), &repo, root, head),
                )
                    .into_future()
                    .and_then(move |(client_cf, client_bcs)| {
                        rebase_in_loop(
                            ctx,
                            repo,
                            config,
                            onto_bookmark,
                            head,
                            root,
                            client_cf,
                            client_bcs,
                            maybe_raw_bundle2_id,
                        )
                    })
            }
        })
}

fn rebase_in_loop(
    ctx: CoreContext,
    repo: BlobRepo,
    config: PushrebaseParams,
    onto_bookmark: OntoBookmarkParams,
    head: ChangesetId,
    root: ChangesetId,
    client_cf: Vec<MPath>,
    client_bcs: Vec<BonsaiChangeset>,
    maybe_raw_bundle2_id: Option<RawBundle2Id>,
) -> BoxFuture<PushrebaseSuccessResult, PushrebaseError> {
    loop_fn(
        (root.clone(), 0),
        move |(latest_rebase_attempt, retry_num)| {
            get_onto_bookmark_value(ctx.clone(), &repo, onto_bookmark.clone()).and_then({
                cloned!(ctx, client_cf, client_bcs, onto_bookmark, repo, config);
                move |bookmark_val| {
                    fetch_bonsai_range(
                        ctx.clone(),
                        &repo,
                        latest_rebase_attempt,
                        bookmark_val.unwrap_or(root),
                    )
                    .and_then({
                        let casefolding_check = config.casefolding_check;
                        move |server_bcs| {
                            if casefolding_check {
                                match check_case_conflicts(
                                    server_bcs.iter().rev().chain(client_bcs.iter().rev()),
                                ) {
                                    Some(path) => Err(PushrebaseError::PotentialCaseConflict(path)),
                                    None => Ok(()),
                                }
                            } else {
                                Ok(())
                            }
                        }
                    })
                    .and_then({
                        cloned!(ctx, repo);
                        move |()| {
                            find_changed_files(
                                ctx.clone(),
                                &repo,
                                latest_rebase_attempt.clone(),
                                bookmark_val.unwrap_or(root),
                                /* reject_merges */ true,
                            )
                        }
                    })
                    .and_then(|server_cf| intersect_changed_files(server_cf, client_cf))
                    .and_then(move |()| {
                        do_rebase(
                            ctx.clone(),
                            repo,
                            config,
                            root.clone(),
                            head,
                            bookmark_val,
                            onto_bookmark.bookmark,
                            maybe_raw_bundle2_id,
                        )
                        .and_then(move |update_res| match update_res {
                            Some((head, rebased_changesets)) => {
                                ok(Loop::Break(PushrebaseSuccessResult {
                                    head,
                                    retry_num,
                                    rebased_changesets,
                                }))
                            }
                            None => {
                                if retry_num < MAX_REBASE_ATTEMPTS {
                                    ok(Loop::Continue((
                                        bookmark_val.unwrap_or(root),
                                        retry_num + 1,
                                    )))
                                } else {
                                    err(ErrorKind::TooManyRebaseAttempts.into())
                                }
                            }
                        })
                    })
                }
            })
        },
    )
    .boxify()
}

fn do_rebase(
    ctx: CoreContext,
    repo: BlobRepo,
    config: PushrebaseParams,
    root: ChangesetId,
    head: ChangesetId,
    bookmark_val: Option<ChangesetId>,
    onto_bookmark: BookmarkName,
    maybe_raw_bundle2_id: Option<RawBundle2Id>,
) -> impl Future<Item = Option<(ChangesetId, Vec<PushrebaseChangesetPair>)>, Error = PushrebaseError>
{
    create_rebased_changesets(
        ctx.clone(),
        repo.clone(),
        config,
        root,
        head,
        bookmark_val.unwrap_or(root),
    )
    .and_then({
        move |(new_head, rebased_changesets)| match bookmark_val {
            Some(bookmark_val) => try_update_bookmark(
                ctx,
                &repo,
                &onto_bookmark,
                bookmark_val,
                new_head,
                maybe_raw_bundle2_id,
                rebased_changesets,
            ),
            None => try_create_bookmark(
                ctx,
                &repo,
                &onto_bookmark,
                new_head,
                maybe_raw_bundle2_id,
                rebased_changesets,
            ),
        }
    })
}

fn fetch_bonsai_changesets(
    ctx: CoreContext,
    repo: BlobRepo,
    commit_ids: Vec<HgChangesetId>,
) -> impl Future<Item = Vec<BonsaiChangeset>, Error = PushrebaseError> {
    join_all(commit_ids.into_iter().map(move |hg_cs| {
        repo.get_bonsai_from_hg(ctx.clone(), hg_cs)
            .and_then({
                cloned!(hg_cs);
                move |bcs_cs| bcs_cs.ok_or(ErrorKind::BonsaiNotFoundForHgChangeset(hg_cs).into())
            })
            .and_then({
                cloned!(ctx, repo);
                move |bcs_id| repo.get_bonsai_changeset(ctx, bcs_id).from_err()
            })
            .with_context(move |_| format!("While intitial bonsai changesets fetching"))
            .map_err(Error::from)
            .from_err()
    }))
}

// There should only be one head in the pushed set
fn find_only_head_or_fail(
    commits: &Vec<BonsaiChangeset>,
) -> ::std::result::Result<ChangesetId, PushrebaseError> {
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

fn find_roots(
    commits: &Vec<BonsaiChangeset>,
) -> ::std::result::Result<HashMap<ChangesetId, ChildIndex>, PushrebaseError> {
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
    Ok(roots)
}

fn find_closest_root(
    ctx: CoreContext,
    repo: &BlobRepo,
    config: PushrebaseParams,
    bookmark: OntoBookmarkParams,
    roots: HashMap<ChangesetId, ChildIndex>,
) -> impl Future<Item = ChangesetId, Error = PushrebaseError> {
    get_bookmark_value(ctx.clone(), repo, &bookmark.bookmark).and_then({
        cloned!(repo);
        move |maybe_id| match maybe_id {
            Some(id) => {
                find_closest_ancestor_root(ctx.clone(), repo, config, bookmark.bookmark, roots, id)
            }
            None => join_all(roots.into_iter().map(move |(root, _)| {
                repo.get_generation_number_by_bonsai(ctx.clone(), root)
                    .and_then(move |maybe_gen_num| {
                        maybe_gen_num.ok_or(ErrorKind::RootNotFound(root).into())
                    })
                    .map(move |gen_num| (root, gen_num))
            }))
            .and_then(|roots_with_gen_nums| {
                roots_with_gen_nums
                    .into_iter()
                    .max_by_key(|(_, gen_num)| gen_num.clone())
                    .ok_or(ErrorKind::NoRoots.into())
            })
            .map(|(cs_id, _)| cs_id)
            .from_err()
            .boxify(),
        }
    })
}

fn find_closest_ancestor_root(
    ctx: CoreContext,
    repo: BlobRepo,
    config: PushrebaseParams,
    bookmark: BookmarkName,
    roots: HashMap<ChangesetId, ChildIndex>,
    onto_bookmark_cs_id: ChangesetId,
) -> BoxFuture<ChangesetId, PushrebaseError> {
    let mut queue = VecDeque::new();
    queue.push_back(onto_bookmark_cs_id);
    loop_fn((queue, 0), move |(mut queue, depth)| {
        if depth >= config.recursion_limit {
            return err(PushrebaseError::RootTooFarBehind).boxify();
        }
        match queue.pop_front() {
            None => err(PushrebaseError::Error(
                ErrorKind::PushrebaseNoCommonRoot(
                    bookmark.clone(),
                    roots.keys().cloned().collect(),
                )
                .into(),
            ))
            .boxify(),
            Some(id) => match roots.get(&id) {
                Some(index) => {
                    if config.forbid_p2_root_rebases && *index != ChildIndex(0) {
                        repo.get_hg_from_bonsai_changeset(ctx.clone(), id)
                            .from_err()
                            .and_then({
                                cloned!(bookmark);
                                move |hgcs| {
                                    err(PushrebaseError::Error(
                                        ErrorKind::P2RootRebaseForbidden(hgcs, bookmark).into(),
                                    ))
                                }
                            })
                            .boxify()
                    } else {
                        ok(Loop::Break(id)).boxify()
                    }
                }
                None => repo
                    .get_changeset_parents_by_bonsai(ctx.clone(), id)
                    .from_err()
                    .and_then(move |parents| {
                        match parents.as_slice() {
                            [] => (),
                            [parent] => {
                                queue.push_back(*parent);
                            }
                            _ => {
                                return err(PushrebaseError::RebaseOverMerge);
                            }
                        };
                        ok(Loop::Continue((queue, depth + 1)))
                    })
                    .boxify(),
            },
        }
    })
    .boxify()
}

/// find changed files by comparing manifests of `ancestor` and `descendant`
fn find_changed_files_between_manfiests(
    ctx: CoreContext,
    repo: &BlobRepo,
    ancestor: ChangesetId,
    descendant: ChangesetId,
) -> impl Future<Item = Vec<MPath>, Error = PushrebaseError> {
    let id_to_manifest = {
        cloned!(ctx, repo);
        move |bcs_id| {
            repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id)
                .and_then({
                    cloned!(ctx, repo);
                    move |cs_id| repo.get_changeset_by_changesetid(ctx, cs_id)
                })
                .map({
                    cloned!(repo);
                    move |cs| repo.get_root_entry(cs.manifestid())
                })
        }
    };

    (id_to_manifest(descendant), id_to_manifest(ancestor))
        .into_future()
        .and_then({
            cloned!(ctx);
            move |(d_mf, a_mf)| {
                bonsai_diff(ctx, Box::new(d_mf), Some(Box::new(a_mf)), None)
                    .map(|diff| match diff {
                        BonsaiDiffResult::Changed(path, ..)
                        | BonsaiDiffResult::ChangedReusedId(path, ..)
                        | BonsaiDiffResult::Deleted(path) => path,
                    })
                    .collect()
            }
        })
        .from_err()
}

// from larger generation number to smaller
fn fetch_bonsai_range(
    ctx: CoreContext,
    repo: &BlobRepo,
    ancestor: ChangesetId,
    descendant: ChangesetId,
) -> impl Future<Item = Vec<BonsaiChangeset>, Error = PushrebaseError> {
    cloned!(repo);
    RangeNodeStream::new(
        ctx.clone(),
        repo.get_changeset_fetcher(),
        ancestor,
        descendant,
    )
    .map(move |id| repo.get_bonsai_changeset(ctx.clone(), id))
    .buffered(100)
    .collect()
    .from_err()
}

fn find_changed_files(
    ctx: CoreContext,
    repo: &BlobRepo,
    ancestor: ChangesetId,
    descendant: ChangesetId,
    reject_merges: bool,
) -> impl Future<Item = Vec<MPath>, Error = PushrebaseError> {
    cloned!(repo);
    RangeNodeStream::new(
        ctx.clone(),
        repo.get_changeset_fetcher(),
        ancestor,
        descendant,
    )
    .map({
        cloned!(ctx, repo);
        move |bcs_id| {
            repo.get_bonsai_changeset(ctx.clone(), bcs_id)
                .map(move |bcs| (bcs_id, bcs))
        }
    })
    .buffered(100)
    .collect()
    .from_err()
    .and_then(move |id_to_bcs| {
        let ids: HashSet<_> = id_to_bcs.iter().map(|(id, _)| *id).collect();
        let file_changes_fut: Vec<_> = id_to_bcs
            .into_iter()
            .filter(|(id, _)| *id != ancestor)
            .map(move |(id, bcs)| {
                let parents: Vec<_> = bcs.parents().collect();
                match *parents {
                    [] | [_] => ok(extract_conflict_files_from_bonsai_changeset(bcs)).left_future(),
                    [p0_id, p1_id] => {
                        if reject_merges {
                            return err(PushrebaseError::RebaseOverMerge).left_future();
                        }
                        match (ids.get(&p0_id), ids.get(&p1_id)) {
                            (Some(_), Some(_)) => {
                                // both parents are in the rebase set, so we can just take
                                // filechanges from bonsai changeset
                                ok(extract_conflict_files_from_bonsai_changeset(bcs)).left_future()
                            }
                            (Some(p_id), None) | (None, Some(p_id)) => {
                                // TODO(stash, T40460159) - include copy sources in the list of
                                // conflict files

                                // one of the parents is not in the rebase set, to calculate
                                // changed files in this case we will compute manifest diff
                                // between elements that are in rebase set.
                                find_changed_files_between_manfiests(ctx.clone(), &repo, id, *p_id)
                                    .right_future()
                            }
                            (None, None) => panic!(
                                "`RangeNodeStream` produced invalid result for: ({}, {})",
                                descendant, ancestor,
                            ),
                        }
                    }
                    _ => panic!("pushrebase supports only two parents"),
                }
            })
            .collect();
        join_all(file_changes_fut).map(|file_changes| {
            let mut file_changes_union = file_changes
                    .into_iter()
                    .flat_map(|v| v)
                    .collect::<HashSet<_>>()  // compute union
                    .into_iter()
                    .collect::<Vec<_>>();
            file_changes_union.sort_unstable();
            file_changes_union
        })
    })
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
fn intersect_changed_files(
    left: Vec<MPath>,
    right: Vec<MPath>,
) -> ::std::result::Result<(), PushrebaseError> {
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
fn get_onto_bookmark_value(
    ctx: CoreContext,
    repo: &BlobRepo,
    onto_bookmark: OntoBookmarkParams,
) -> impl Future<Item = Option<ChangesetId>, Error = PushrebaseError> {
    get_bookmark_value(ctx.clone(), &repo, &onto_bookmark.bookmark).and_then(
        move |maybe_bookmark_value| match maybe_bookmark_value {
            Some(bookmark_value) => ok(Some(bookmark_value)),
            None => ok(None),
        },
    )
}

fn get_bookmark_value(
    ctx: CoreContext,
    repo: &BlobRepo,
    bookmark_name: &BookmarkName,
) -> impl Future<Item = Option<ChangesetId>, Error = PushrebaseError> {
    repo.get_bonsai_bookmark(ctx, bookmark_name).from_err()
}

fn create_rebased_changesets(
    ctx: CoreContext,
    repo: BlobRepo,
    config: PushrebaseParams,
    root: ChangesetId,
    head: ChangesetId,
    onto: ChangesetId,
) -> impl Future<Item = (ChangesetId, RebasedChangesets), Error = PushrebaseError> {
    find_rebased_set(ctx.clone(), repo.clone(), root, head.clone()).and_then(move |rebased_set| {
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
            let bcs_new = match rebase_changeset(bcs_old, &remapping, date.as_ref()) {
                Ok(bcs_new) => bcs_new,
                Err(e) => return err(e.into()).left_future(),
            };
            let timestamp = Timestamp::from(*bcs_new.author_date());
            remapping.insert(id_old, (bcs_new.get_changeset_id(), timestamp));
            rebased.push(bcs_new);
        }

        save_bonsai_changesets(rebased, ctx, repo)
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
            .right_future()
    })
}

fn rebase_changeset(
    bcs: BonsaiChangeset,
    remapping: &HashMap<ChangesetId, (ChangesetId, Timestamp)>,
    timestamp: Option<&Timestamp>,
) -> Result<BonsaiChangeset> {
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
    for key in &["mutpred", "mutuser", "mutdate", "mutop", "mutsplit"] {
        bcs.extra.remove(*key);
    }

    // Copy information in bonsai changeset contains a commit parent. So parent changes, then
    // copy information for all copied/moved files needs to be updated
    bcs.file_changes = bcs
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

    bcs.freeze()
}

// Order - from lowest generation number to highest
fn find_rebased_set(
    ctx: CoreContext,
    repo: BlobRepo,
    root: ChangesetId,
    head: ChangesetId,
) -> impl Future<Item = Vec<BonsaiChangeset>, Error = PushrebaseError> {
    RangeNodeStream::new(ctx.clone(), repo.get_changeset_fetcher(), root, head)
        .map({
            cloned!(repo);
            move |bcs_id| repo.get_bonsai_changeset(ctx.clone(), bcs_id)
        })
        .buffered(100)
        .collect()
        .map(move |nodes| {
            nodes
                .into_iter()
                .filter(|node| node.get_changeset_id() != root)
                .rev()
                .collect()
        })
        .from_err()
}

fn try_update_bookmark(
    ctx: CoreContext,
    repo: &BlobRepo,
    bookmark_name: &BookmarkName,
    old_value: ChangesetId,
    new_value: ChangesetId,
    maybe_raw_bundle2_id: Option<RawBundle2Id>,
    rebased_changesets: RebasedChangesets,
) -> BoxFuture<Option<(ChangesetId, Vec<PushrebaseChangesetPair>)>, PushrebaseError> {
    let mut txn = repo.update_bookmark_transaction(ctx.clone());
    let bookmark_update_reason = create_bookmark_update_reason(
        ctx,
        repo.clone(),
        maybe_raw_bundle2_id,
        rebased_changesets.clone(),
    );
    bookmark_update_reason
        .from_err()
        .and_then({
            cloned!(bookmark_name);
            move |reason| {
                try_boxfuture!(txn.update(&bookmark_name, new_value, old_value, reason));
                txn.commit()
                    .map(move |success| {
                        if success {
                            Some((new_value, rebased_changesets_into_pairs(rebased_changesets)))
                        } else {
                            None
                        }
                    })
                    .from_err()
                    .boxify()
            }
        })
        .boxify()
}

fn try_create_bookmark(
    ctx: CoreContext,
    repo: &BlobRepo,
    bookmark_name: &BookmarkName,
    new_value: ChangesetId,
    maybe_raw_bundle2_id: Option<RawBundle2Id>,
    rebased_changesets: RebasedChangesets,
) -> BoxFuture<Option<(ChangesetId, Vec<PushrebaseChangesetPair>)>, PushrebaseError> {
    let mut txn = repo.update_bookmark_transaction(ctx.clone());
    let bookmark_update_reason = create_bookmark_update_reason(
        ctx,
        repo.clone(),
        maybe_raw_bundle2_id,
        rebased_changesets.clone(),
    );

    bookmark_update_reason
        .from_err()
        .and_then({
            cloned!(bookmark_name);
            move |reason| {
                try_boxfuture!(txn.create(&bookmark_name, new_value, reason));
                txn.commit()
                    .map(move |success| {
                        if success {
                            Some((new_value, rebased_changesets_into_pairs(rebased_changesets)))
                        } else {
                            None
                        }
                    })
                    .from_err()
                    .boxify()
            }
        })
        .boxify()
}

fn create_bookmark_update_reason(
    ctx: CoreContext,
    repo: BlobRepo,
    maybe_raw_bundle2_id: Option<RawBundle2Id>,
    rebased_changesets: RebasedChangesets,
) -> BoxFuture<BookmarkUpdateReason, Error> {
    match maybe_raw_bundle2_id {
        Some(id) => {
            let bundle_replay_data = BundleReplayData::new(id);
            let timestamps = rebased_changesets
                .into_iter()
                .map(|(id_old, (_, datetime))| (id_old, datetime.into()))
                .map({
                    cloned!(ctx, repo);
                    move |(id_old, timestamp)| {
                        repo.get_hg_from_bonsai_changeset(ctx.clone(), id_old)
                            .map(move |hg_cs_id| (hg_cs_id, timestamp))
                    }
                });
            join_all(timestamps)
                .map(move |timestamps| {
                    let timestamps = timestamps.into_iter().collect();
                    BookmarkUpdateReason::Pushrebase {
                        bundle_replay_data: Some(bundle_replay_data.with_timestamps(timestamps)),
                    }
                })
                .boxify()
        }
        None => ok(BookmarkUpdateReason::Pushrebase {
            bundle_replay_data: None,
        })
        .boxify(),
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use async_unit;
    use failure::err_msg;
    use fixtures::{linear, many_files_dirs};
    use futures::future::join_all;
    use futures_ext::spawn_future;
    use maplit::{btreemap, hashmap, hashset};
    use mononoke_types_mocks::hash::AS;
    use std::str::FromStr;
    use tests_utils::{create_commit, create_commit_with_date, store_files, store_rename};

    fn set_bookmark(ctx: CoreContext, repo: BlobRepo, book: &BookmarkName, cs_id: &str) {
        let head = HgChangesetId::from_str(cs_id).unwrap();
        let head = repo
            .get_bonsai_from_hg(ctx.clone(), head)
            .wait()
            .unwrap()
            .unwrap();
        let mut txn = repo.update_bookmark_transaction(ctx);
        txn.force_set(
            &book,
            head,
            BookmarkUpdateReason::TestMove {
                bundle_replay_data: None,
            },
        )
        .unwrap();
        txn.commit().wait().unwrap();
    }

    fn make_paths(paths: &[&str]) -> Vec<MPath> {
        let paths: ::std::result::Result<_, _> = paths.into_iter().map(MPath::new).collect();
        paths.unwrap()
    }

    fn master_bookmark() -> OntoBookmarkParams {
        let book = BookmarkName::new("master").unwrap();
        let book = OntoBookmarkParams { bookmark: book };
        book
    }

    #[test]
    fn pushrebase_one_commit() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = linear::getrepo(None);
            // Bottom commit of the repo
            let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap();
            let p = repo
                .get_bonsai_from_hg(ctx.clone(), root)
                .wait()
                .unwrap()
                .unwrap();
            let parents = vec![p];

            let bcs_id = create_commit(
                ctx.clone(),
                repo.clone(),
                parents,
                store_files(
                    ctx.clone(),
                    btreemap! {"file" => Some("content")},
                    repo.clone(),
                ),
            );
            let hg_cs = repo
                .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id)
                .wait()
                .unwrap();

            let book = master_bookmark();
            set_bookmark(
                ctx.clone(),
                repo.clone(),
                &book.bookmark,
                "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
            );

            do_pushrebase(ctx, repo, Default::default(), book, vec![hg_cs], None)
                .wait()
                .expect("pushrebase failed");
        });
    }

    #[test]
    fn pushrebase_stack() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = linear::getrepo(None);
            // Bottom commit of the repo
            let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap();
            let p = repo
                .get_bonsai_from_hg(ctx.clone(), root)
                .wait()
                .unwrap()
                .unwrap();
            let bcs_id_1 = create_commit(
                ctx.clone(),
                repo.clone(),
                vec![p],
                store_files(
                    ctx.clone(),
                    btreemap! {"file" => Some("content")},
                    repo.clone(),
                ),
            );
            let bcs_id_2 = create_commit(
                ctx.clone(),
                repo.clone(),
                vec![bcs_id_1],
                store_files(
                    ctx.clone(),
                    btreemap! {"file2" => Some("content")},
                    repo.clone(),
                ),
            );

            assert_eq!(
                find_changed_files(ctx.clone(), &repo.clone(), p, bcs_id_2, false)
                    .wait()
                    .unwrap(),
                make_paths(&["file", "file2"]),
            );

            let book = master_bookmark();
            set_bookmark(
                ctx.clone(),
                repo.clone(),
                &book.bookmark,
                "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
            );

            let hg_cs_1 = repo
                .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_1)
                .wait()
                .unwrap();
            let hg_cs_2 = repo
                .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_2)
                .wait()
                .unwrap();
            do_pushrebase(
                ctx,
                repo,
                Default::default(),
                book,
                vec![hg_cs_1, hg_cs_2],
                None,
            )
            .wait()
            .expect("pushrebase failed");
        });
    }

    #[test]
    fn pushrebase_stack_with_renames() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = linear::getrepo(None);
            // Bottom commit of the repo
            let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap();
            let p = repo
                .get_bonsai_from_hg(ctx.clone(), root)
                .wait()
                .unwrap()
                .unwrap();
            let bcs_id_1 = create_commit(
                ctx.clone(),
                repo.clone(),
                vec![p],
                store_files(
                    ctx.clone(),
                    btreemap! {"file" => Some("content")},
                    repo.clone(),
                ),
            );

            let rename = store_rename(
                ctx.clone(),
                (MPath::new("file").unwrap(), bcs_id_1),
                "file_renamed",
                "content",
                repo.clone(),
            );

            let file_changes = btreemap! {rename.0 => rename.1};
            let bcs_id_2 = create_commit(ctx.clone(), repo.clone(), vec![bcs_id_1], file_changes);

            assert_eq!(
                find_changed_files(ctx.clone(), &repo.clone(), p, bcs_id_2, false)
                    .wait()
                    .unwrap(),
                make_paths(&["file", "file_renamed"]),
            );

            let book = master_bookmark();
            set_bookmark(
                ctx.clone(),
                repo.clone(),
                &book.bookmark,
                "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
            );

            let hg_cs_1 = repo
                .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_1)
                .wait()
                .unwrap();
            let hg_cs_2 = repo
                .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_2)
                .wait()
                .unwrap();
            do_pushrebase(
                ctx,
                repo,
                Default::default(),
                book,
                vec![hg_cs_1, hg_cs_2],
                None,
            )
            .wait()
            .expect("pushrebase failed");
        });
    }

    #[test]
    fn pushrebase_multi_root() {
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
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = linear::getrepo(None);
            let config = PushrebaseParams::default();

            let root0 = repo
                .get_bonsai_from_hg(
                    ctx.clone(),
                    HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
                )
                .wait()
                .unwrap()
                .unwrap();

            let root1 = repo
                .get_bonsai_from_hg(
                    ctx.clone(),
                    HgChangesetId::from_str("607314ef579bd2407752361ba1b0c1729d08b281").unwrap(),
                )
                .wait()
                .unwrap()
                .unwrap();

            let bcs_id_1 = create_commit(
                ctx.clone(),
                repo.clone(),
                vec![root0],
                store_files(
                    ctx.clone(),
                    btreemap! {"f0" => Some("f0"), "files" => None},
                    repo.clone(),
                ),
            );
            let bcs_id_2 = create_commit(
                ctx.clone(),
                repo.clone(),
                vec![root1, bcs_id_1],
                store_files(ctx.clone(), btreemap! {"f1" => Some("f1")}, repo.clone()),
            );
            let bcs_id_3 = create_commit(
                ctx.clone(),
                repo.clone(),
                vec![bcs_id_2],
                store_files(ctx.clone(), btreemap! {"f2" => Some("f2")}, repo.clone()),
            );

            let book = master_bookmark();
            set_bookmark(
                ctx.clone(),
                repo.clone(),
                &book.bookmark,
                "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
            );
            let bcs_id_master = repo
                .get_bonsai_from_hg(
                    ctx.clone(),
                    HgChangesetId::from_str("a5ffa77602a066db7d5cfb9fb5823a0895717c5a").unwrap(),
                )
                .wait()
                .unwrap()
                .unwrap();

            let root = root1;
            assert_eq!(
                find_closest_root(
                    ctx.clone(),
                    &repo,
                    config.clone(),
                    book.clone(),
                    hashmap! {root0 => ChildIndex(0), root1 => ChildIndex(0) },
                )
                .wait()
                .unwrap(),
                root,
            );

            assert_eq!(
                find_changed_files(ctx.clone(), &repo, root, bcs_id_3, false)
                    .wait()
                    .unwrap(),
                make_paths(&["f0", "f1", "f2"]),
            );

            let hg_cs_1 = repo
                .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_1)
                .wait()
                .unwrap();
            let hg_cs_2 = repo
                .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_2)
                .wait()
                .unwrap();
            let hg_cs_3 = repo
                .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_3)
                .wait()
                .unwrap();
            let bcs_id_rebased = do_pushrebase(
                ctx.clone(),
                repo.clone(),
                config,
                book,
                vec![hg_cs_1, hg_cs_2, hg_cs_3],
                None,
            )
            .wait()
            .expect("pushrebase failed");

            // should only rebase {bcs2, bcs3}
            let rebased = find_rebased_set(ctx, repo, bcs_id_master, bcs_id_rebased.head)
                .wait()
                .unwrap();
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
        });
    }

    #[test]
    fn pushrebase_conflict() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = linear::getrepo(None);
            let root = repo
                .get_bonsai_from_hg(
                    ctx.clone(),
                    HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
                )
                .wait()
                .unwrap()
                .unwrap();

            let bcs_id_1 = create_commit(
                ctx.clone(),
                repo.clone(),
                vec![root],
                store_files(ctx.clone(), btreemap! {"f0" => Some("f0")}, repo.clone()),
            );
            let bcs_id_2 = create_commit(
                ctx.clone(),
                repo.clone(),
                vec![bcs_id_1],
                store_files(
                    ctx.clone(),
                    btreemap! {"9/file" => Some("file")},
                    repo.clone(),
                ),
            );
            let bcs_id_3 = create_commit(
                ctx.clone(),
                repo.clone(),
                vec![bcs_id_2],
                store_files(ctx.clone(), btreemap! {"f1" => Some("f1")}, repo.clone()),
            );

            let book = master_bookmark();
            set_bookmark(
                ctx.clone(),
                repo.clone(),
                &book.bookmark,
                "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
            );

            let hg_cs_1 = repo
                .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_1)
                .wait()
                .unwrap();
            let hg_cs_2 = repo
                .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_2)
                .wait()
                .unwrap();
            let hg_cs_3 = repo
                .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_3)
                .wait()
                .unwrap();
            let result = do_pushrebase(
                ctx,
                repo,
                Default::default(),
                book,
                vec![hg_cs_1, hg_cs_2, hg_cs_3],
                None,
            )
            .wait();
            match result {
                Err(PushrebaseError::Conflicts(conflicts)) => {
                    assert_eq!(
                        conflicts,
                        vec![PushrebaseConflict {
                            left: MPath::new("9").unwrap(),
                            right: MPath::new("9/file").unwrap(),
                        },],
                    );
                }
                _ => panic!("push-rebase should have failed with conflict"),
            }
        });
    }

    #[test]
    fn pushrebase_caseconflicting_rename() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = linear::getrepo(None);
            let root = repo
                .get_bonsai_from_hg(
                    ctx.clone(),
                    HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
                )
                .wait()
                .unwrap()
                .unwrap();

            let bcs_id_1 = create_commit(
                ctx.clone(),
                repo.clone(),
                vec![root],
                store_files(
                    ctx.clone(),
                    btreemap! {"FILE" => Some("file")},
                    repo.clone(),
                ),
            );
            let bcs_id_2 = create_commit(
                ctx.clone(),
                repo.clone(),
                vec![bcs_id_1],
                store_files(ctx.clone(), btreemap! {"FILE" => None}, repo.clone()),
            );
            let bcs_id_3 = create_commit(
                ctx.clone(),
                repo.clone(),
                vec![bcs_id_2],
                store_files(
                    ctx.clone(),
                    btreemap! {"file" => Some("file")},
                    repo.clone(),
                ),
            );
            let hgcss = vec![
                repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_1)
                    .wait()
                    .unwrap(),
                repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_2)
                    .wait()
                    .unwrap(),
                repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_3)
                    .wait()
                    .unwrap(),
            ];

            let book = master_bookmark();
            set_bookmark(
                ctx.clone(),
                repo.clone(),
                &book.bookmark,
                "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
            );

            do_pushrebase(ctx, repo, Default::default(), book, hgcss, None)
                .wait()
                .expect("push-rebase failed");
        })
    }

    #[test]
    fn pushrebase_caseconflicting_dirs() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = linear::getrepo(None);
            let root = repo
                .get_bonsai_from_hg(
                    ctx.clone(),
                    HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
                )
                .wait()
                .unwrap()
                .unwrap();

            let bcs_id_1 = create_commit(
                ctx.clone(),
                repo.clone(),
                vec![root],
                store_files(
                    ctx.clone(),
                    btreemap! {"DIR/a" => Some("a"), "DIR/b" => Some("b")},
                    repo.clone(),
                ),
            );
            let bcs_id_2 = create_commit(
                ctx.clone(),
                repo.clone(),
                vec![bcs_id_1],
                store_files(
                    ctx.clone(),
                    btreemap! {"dir/a" => Some("a"), "DIR/a" => None, "DIR/b" => None},
                    repo.clone(),
                ),
            );
            let hgcss = vec![
                repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_1)
                    .wait()
                    .unwrap(),
                repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_2)
                    .wait()
                    .unwrap(),
            ];

            let book = master_bookmark();
            set_bookmark(
                ctx.clone(),
                repo.clone(),
                &book.bookmark,
                "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
            );

            do_pushrebase(ctx, repo, Default::default(), book, hgcss, None)
                .wait()
                .expect("push-rebase failed");
        })
    }

    #[test]
    fn pushrebase_recursion_limit() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = linear::getrepo(None);
            let root = repo
                .get_bonsai_from_hg(
                    ctx.clone(),
                    HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
                )
                .wait()
                .unwrap()
                .unwrap();

            // create a lot of commits
            let mut bcss = Vec::new();
            (0..128).fold(root, |head, index| {
                let file = format!("f{}", index);
                let content = format!("{}", index);
                let bcs = create_commit(
                    ctx.clone(),
                    repo.clone(),
                    vec![head],
                    store_files(
                        ctx.clone(),
                        btreemap! {file.as_ref() => Some(content.as_ref())},
                        repo.clone(),
                    ),
                );
                bcss.push(bcs);
                bcs
            });

            let hgcss = join_all(
                bcss.iter()
                    .map(|bcs| repo.get_hg_from_bonsai_changeset(ctx.clone(), *bcs))
                    .collect::<Vec<_>>(),
            )
            .wait()
            .unwrap();
            let book = master_bookmark();
            set_bookmark(
                ctx.clone(),
                repo.clone(),
                &book.bookmark,
                "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
            );
            let repo_arc = repo.clone();
            do_pushrebase(
                ctx.clone(),
                repo_arc.clone(),
                Default::default(),
                book.clone(),
                hgcss,
                None,
            )
            .wait()
            .expect("pushrebase failed");

            let bcs = create_commit(
                ctx.clone(),
                repo.clone(),
                vec![root],
                store_files(
                    ctx.clone(),
                    btreemap! {"file" => Some("data")},
                    repo.clone(),
                ),
            );
            let hgcss = vec![repo_arc
                .get_hg_from_bonsai_changeset(ctx.clone(), bcs)
                .wait()
                .unwrap()];

            // try rebase with small recursion limit
            let config = PushrebaseParams {
                recursion_limit: 128,
                ..Default::default()
            };
            let result = do_pushrebase(
                ctx.clone(),
                repo_arc.clone(),
                config,
                book.clone(),
                hgcss.clone(),
                None,
            )
            .wait();
            match result {
                Err(PushrebaseError::RootTooFarBehind) => (),
                _ => panic!("push-rebase should have failed because root too far behind"),
            }

            let config = PushrebaseParams {
                recursion_limit: 256,
                ..Default::default()
            };
            do_pushrebase(ctx, repo_arc, config, book, hgcss, None)
                .wait()
                .expect("push-rebase failed");
        })
    }

    #[test]
    fn pushrebase_rewritedates() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = linear::getrepo(None);
            let root = repo
                .get_bonsai_from_hg(
                    ctx.clone(),
                    HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
                )
                .wait()
                .unwrap()
                .unwrap();
            let book = master_bookmark();
            let bcs = create_commit(
                ctx.clone(),
                repo.clone(),
                vec![root],
                store_files(
                    ctx.clone(),
                    btreemap! {"file" => Some("data")},
                    repo.clone(),
                ),
            );
            let hgcss = vec![repo
                .get_hg_from_bonsai_changeset(ctx.clone(), bcs)
                .wait()
                .unwrap()];

            set_bookmark(
                ctx.clone(),
                repo.clone(),
                &book.bookmark,
                "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
            );
            let config = PushrebaseParams {
                rewritedates: false,
                ..Default::default()
            };
            let bcs_keep_date = do_pushrebase(
                ctx.clone(),
                repo.clone(),
                config,
                book.clone(),
                hgcss.clone(),
                None,
            )
            .wait()
            .expect("push-rebase failed");

            set_bookmark(
                ctx.clone(),
                repo.clone(),
                &book.bookmark,
                "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
            );
            let config = PushrebaseParams {
                rewritedates: true,
                ..Default::default()
            };
            let bcs_rewrite_date =
                do_pushrebase(ctx.clone(), repo.clone(), config, book, hgcss, None)
                    .wait()
                    .expect("push-rebase failed");

            let bcs = repo.get_bonsai_changeset(ctx.clone(), bcs).wait().unwrap();
            let bcs_keep_date = repo
                .get_bonsai_changeset(ctx.clone(), bcs_keep_date.head)
                .wait()
                .unwrap();
            let bcs_rewrite_date = repo
                .get_bonsai_changeset(ctx.clone(), bcs_rewrite_date.head)
                .wait()
                .unwrap();

            assert_eq!(bcs.author_date(), bcs_keep_date.author_date());
            assert!(bcs.author_date() < bcs_rewrite_date.author_date());
        })
    }

    #[test]
    fn pushrebase_case_conflict() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = many_files_dirs::getrepo(None);
            let root = repo
                .get_bonsai_from_hg(
                    ctx.clone(),
                    HgChangesetId::from_str("5a28e25f924a5d209b82ce0713d8d83e68982bc8").unwrap(),
                )
                .wait()
                .unwrap()
                .unwrap();

            let bcs = create_commit(
                ctx.clone(),
                repo.clone(),
                vec![root],
                store_files(
                    ctx.clone(),
                    btreemap! {"Dir1/file_1_in_dir1" => Some("data")},
                    repo.clone(),
                ),
            );
            let hgcss = vec![repo
                .get_hg_from_bonsai_changeset(ctx.clone(), bcs)
                .wait()
                .unwrap()];

            let book = master_bookmark();
            set_bookmark(
                ctx.clone(),
                repo.clone(),
                &book.bookmark,
                "2f866e7e549760934e31bf0420a873f65100ad63",
            );

            let result = do_pushrebase(
                ctx.clone(),
                repo.clone(),
                Default::default(),
                book.clone(),
                hgcss.clone(),
                None,
            )
            .wait();
            match result {
                Err(PushrebaseError::PotentialCaseConflict(conflict)) => {
                    assert_eq!(conflict, MPath::new("Dir1/file_1_in_dir1").unwrap())
                }
                _ => panic!("push-rebase should have failed with case conflict"),
            };

            // make sure that it is succeeds with disabled casefolding
            do_pushrebase(
                ctx,
                repo,
                PushrebaseParams {
                    casefolding_check: false,
                    ..Default::default()
                },
                book,
                hgcss,
                None,
            )
            .wait()
            .expect("pushrebase failed");
        })
    }

    #[test]
    fn pushrebase_intersect_changed() {
        match intersect_changed_files(
            make_paths(&["a/b/c", "c", "a/b/d", "d/d", "b", "e/c"]),
            make_paths(&["d/f", "a/b/d/f", "c", "e"]),
        ) {
            Err(PushrebaseError::Conflicts(conflicts)) => assert_eq!(
                *conflicts,
                [
                    PushrebaseConflict {
                        left: MPath::new("a/b/d").unwrap(),
                        right: MPath::new("a/b/d/f").unwrap(),
                    },
                    PushrebaseConflict {
                        left: MPath::new("c").unwrap(),
                        right: MPath::new("c").unwrap(),
                    },
                    PushrebaseConflict {
                        left: MPath::new("e/c").unwrap(),
                        right: MPath::new("e").unwrap(),
                    },
                ]
            ),
            _ => panic!("should contain conflict"),
        }
    }

    #[test]
    fn pushrebase_executable_bit_change() {
        use mononoke_types::FileType;

        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = linear::getrepo(None);
            let path_1 = MPath::new("1").unwrap();

            let root_hg =
                HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap();
            let root_cs = repo
                .get_changeset_by_changesetid(ctx.clone(), root_hg)
                .wait()
                .unwrap();
            let root_1_id = repo
                .find_file_in_manifest(ctx.clone(), &path_1, root_cs.manifestid())
                .wait()
                .unwrap()
                .unwrap();

            // crate filechange with with same content as "1" but set executable bit
            let root = repo
                .get_bonsai_from_hg(ctx.clone(), root_hg)
                .wait()
                .unwrap()
                .unwrap();
            let root_bcs = repo.get_bonsai_changeset(ctx.clone(), root).wait().unwrap();
            let file_1 = root_bcs
                .file_changes()
                .find(|(path, _)| path == &&path_1)
                .unwrap()
                .1
                .unwrap()
                .clone();
            assert_eq!(file_1.file_type(), FileType::Regular);
            let file_1_exec = FileChange::new(
                file_1.content_id(),
                FileType::Executable,
                file_1.size(),
                /* copy_from */ None,
            );

            let bcs = create_commit(
                ctx.clone(),
                repo.clone(),
                vec![root],
                btreemap! {path_1.clone() => Some(file_1_exec.clone())},
            );
            let hgcss = vec![repo
                .get_hg_from_bonsai_changeset(ctx.clone(), bcs)
                .wait()
                .unwrap()];

            let book = master_bookmark();
            set_bookmark(
                ctx.clone(),
                repo.clone(),
                &book.bookmark,
                "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
            );

            let result = do_pushrebase(
                ctx.clone(),
                repo.clone(),
                Default::default(),
                book,
                hgcss,
                None,
            )
            .wait()
            .expect("pushrebase failed");
            let result_bcs = repo
                .get_bonsai_changeset(ctx.clone(), result.head)
                .wait()
                .unwrap();
            let file_1_result = result_bcs
                .file_changes()
                .find(|(path, _)| path == &&path_1)
                .unwrap()
                .1
                .unwrap();
            assert_eq!(file_1_result, &file_1_exec);

            let result_hg = repo
                .get_hg_from_bonsai_changeset(ctx.clone(), result.head)
                .wait()
                .unwrap();
            let result_cs = repo
                .get_changeset_by_changesetid(ctx.clone(), result_hg)
                .wait()
                .unwrap();
            let result_1_id = repo
                .find_file_in_manifest(ctx.clone(), &path_1, result_cs.manifestid())
                .wait()
                .unwrap()
                .unwrap();

            // `result_1_id` should be equal to `root_1_id`, because executable flag
            // is not a part of file envelope
            assert_eq!(root_1_id.1, result_1_id.1);
        })
    }

    fn count_commits_between(
        ctx: CoreContext,
        repo: BlobRepo,
        ancestor: HgChangesetId,
        descendant: BookmarkName,
    ) -> impl Future<Item = usize, Error = Error> {
        let ancestor = repo
            .get_bonsai_from_hg(ctx.clone(), ancestor)
            .and_then(|val| val.ok_or(err_msg("ancestor not found")));

        let descendant = repo
            .get_bookmark(ctx.clone(), &descendant)
            .and_then(|val| val.ok_or(err_msg("bookmark not found")));
        let descendant = descendant.and_then({
            cloned!(ctx, repo);
            move |descendant| {
                repo.get_bonsai_from_hg(ctx.clone(), descendant)
                    .and_then(|bonsai| bonsai.ok_or(err_msg("bonsai not found")))
            }
        });

        ancestor
            .join(descendant)
            .and_then(move |(ancestor, descendant)| {
                RangeNodeStream::new(ctx, repo.get_changeset_fetcher(), ancestor, descendant)
                    .collect()
                    .map(|v| v.len())
            })
    }

    #[test]
    fn pushrebase_simultaneously() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = linear::getrepo(None);
            // Bottom commit of the repo
            let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap();
            let p = repo
                .get_bonsai_from_hg(ctx.clone(), root)
                .wait()
                .unwrap()
                .unwrap();
            let parents = vec![p];

            let book = master_bookmark();
            set_bookmark(
                ctx.clone(),
                repo.clone(),
                &book.bookmark,
                "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
            );

            let num_pushes = 10;
            let mut futs = vec![];
            for i in 0..num_pushes {
                let f = format!("file{}", i);
                let bcs_id = create_commit(
                    ctx.clone(),
                    repo.clone(),
                    parents.clone(),
                    store_files(
                        ctx.clone(),
                        btreemap! { f.as_ref() => Some("content")},
                        repo.clone(),
                    ),
                );
                let hg_cs = repo
                    .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id)
                    .wait()
                    .unwrap();

                let fut = spawn_future(
                    do_pushrebase(
                        ctx.clone(),
                        repo.clone(),
                        Default::default(),
                        book.clone(),
                        vec![hg_cs],
                        None,
                    )
                    .map_err(|_| err_msg("error while pushrebasing")),
                );
                futs.push(fut);
            }
            let res = join_all(futs).wait().expect("pushrebase failed");
            let mut has_retry_num_bigger_1 = false;
            for r in res {
                if r.retry_num > 1 {
                    has_retry_num_bigger_1 = true;
                }
            }

            assert!(has_retry_num_bigger_1);

            let previous_master =
                HgChangesetId::from_str("a5ffa77602a066db7d5cfb9fb5823a0895717c5a").unwrap();
            let commits_between = count_commits_between(ctx, repo, previous_master, book.bookmark)
                .wait()
                .unwrap();
            // `- 1` because RangeNodeStream is inclusive
            assert_eq!(commits_between - 1, num_pushes);
        })
    }

    fn run_future<F, I, E>(
        runtime: &mut tokio::runtime::Runtime,
        future: F,
    ) -> std::result::Result<I, E>
    where
        F: Future<Item = I, Error = E> + Send + 'static,
        I: Send + 'static,
        E: Send + 'static,
    {
        runtime.block_on(future)
    }

    #[test]
    fn pushrebase_create_new_bookmark() {
        let mut runtime = tokio::runtime::Runtime::new().unwrap();
        let ctx = CoreContext::test_mock();
        let repo = linear::getrepo(None);
        // Bottom commit of the repo
        let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap();
        let p = run_future(&mut runtime, repo.get_bonsai_from_hg(ctx.clone(), root))
            .unwrap()
            .unwrap();
        let parents = vec![p];

        let bcs_id = create_commit(
            ctx.clone(),
            repo.clone(),
            parents,
            store_files(
                ctx.clone(),
                btreemap! {"file" => Some("content")},
                repo.clone(),
            ),
        );
        let hg_cs = run_future(
            &mut runtime,
            repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id),
        )
        .unwrap();

        let book = BookmarkName::new("newbook").unwrap();
        let book = OntoBookmarkParams { bookmark: book };
        assert!(run_future(
            &mut runtime,
            do_pushrebase(ctx, repo, Default::default(), book, vec![hg_cs], None),
        )
        .is_ok());
    }

    #[test]
    fn pushrebase_simultaneously_and_create_new() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = linear::getrepo(None);
            // Bottom commit of the repo
            let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap();
            let p = repo
                .get_bonsai_from_hg(ctx.clone(), root)
                .wait()
                .unwrap()
                .unwrap();
            let parents = vec![p];

            let book = BookmarkName::new("newbook").unwrap();
            let book = OntoBookmarkParams { bookmark: book };

            let num_pushes = 10;
            let mut futs = vec![];
            for i in 0..num_pushes {
                let f = format!("file{}", i);
                let bcs_id = create_commit(
                    ctx.clone(),
                    repo.clone(),
                    parents.clone(),
                    store_files(
                        ctx.clone(),
                        btreemap! { f.as_ref() => Some("content")},
                        repo.clone(),
                    ),
                );
                let hg_cs = repo
                    .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id)
                    .wait()
                    .unwrap();

                let fut = spawn_future(
                    do_pushrebase(
                        ctx.clone(),
                        repo.clone(),
                        Default::default(),
                        book.clone(),
                        vec![hg_cs],
                        None,
                    )
                    .map_err(|err| err_msg(format!("error while pushrebasing {:?}", err))),
                );
                futs.push(fut);
            }
            let res = join_all(futs).wait().expect("pushrebase failed");
            let mut has_retry_num_bigger_1 = false;
            for r in res {
                if r.retry_num > 1 {
                    has_retry_num_bigger_1 = true;
                }
            }

            assert!(has_retry_num_bigger_1);

            let commits_between = count_commits_between(ctx, repo, root, book.bookmark)
                .wait()
                .unwrap();
            // `- 1` because RangeNodeStream is inclusive
            assert_eq!(commits_between - 1, num_pushes);
        })
    }

    #[test]
    fn pushrebase_one_commit_with_bundle_id() {
        let mut runtime = tokio::runtime::Runtime::new().unwrap();

        let ctx = CoreContext::test_mock();
        let repo = linear::getrepo(None);
        // Bottom commit of the repo
        let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap();
        let p = run_future(&mut runtime, repo.get_bonsai_from_hg(ctx.clone(), root))
            .unwrap()
            .unwrap();
        let parents = vec![p];

        let bcs_id = create_commit(
            ctx.clone(),
            repo.clone(),
            parents,
            store_files(
                ctx.clone(),
                btreemap! {"file" => Some("content")},
                repo.clone(),
            ),
        );
        let hg_cs = run_future(
            &mut runtime,
            repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id),
        )
        .unwrap();

        let book = master_bookmark();
        set_bookmark(
            ctx.clone(),
            repo.clone(),
            &book.bookmark,
            "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
        );

        run_future(
            &mut runtime,
            do_pushrebase(
                ctx,
                repo,
                Default::default(),
                book,
                vec![hg_cs],
                Some(RawBundle2Id::new(AS)),
            ),
        )
        .expect("pushrebase failed");
    }

    #[test]
    fn pushrebase_timezone() {
        // We shouldn't change timezone even if timestamp changes

        let mut runtime = tokio::runtime::Runtime::new().unwrap();

        let ctx = CoreContext::test_mock();
        let repo = linear::getrepo(None);
        // Bottom commit of the repo
        let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap();
        let p = run_future(&mut runtime, repo.get_bonsai_from_hg(ctx.clone(), root))
            .unwrap()
            .unwrap();
        let parents = vec![p];

        let tz_offset_secs = 100;
        let bcs_id = create_commit_with_date(
            ctx.clone(),
            repo.clone(),
            parents,
            store_files(
                ctx.clone(),
                btreemap! {"file" => Some("content")},
                repo.clone(),
            ),
            DateTime::from_timestamp(0, 100).unwrap(),
        );
        let hg_cs = run_future(
            &mut runtime,
            repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id),
        )
        .unwrap();

        let book = master_bookmark();
        set_bookmark(
            ctx.clone(),
            repo.clone(),
            &book.bookmark,
            "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
        );

        let config = PushrebaseParams {
            rewritedates: true,
            ..Default::default()
        };
        let bcs_rewrite_date = run_future(
            &mut runtime,
            do_pushrebase(
                ctx.clone(),
                repo.clone(),
                config,
                book,
                vec![hg_cs],
                Some(RawBundle2Id::new(AS)),
            ),
        )
        .expect("pushrebase failed");

        let bcs_rewrite_date = run_future(
            &mut runtime,
            repo.get_bonsai_changeset(ctx.clone(), bcs_rewrite_date.head),
        )
        .unwrap();
        assert_eq!(
            bcs_rewrite_date.author_date().tz_offset_secs(),
            tz_offset_secs
        );
    }

    #[test]
    fn forbid_p2_root_rebases() {
        let mut runtime = tokio::runtime::Runtime::new().unwrap();
        let ctx = CoreContext::test_mock();
        let repo = linear::getrepo(None);

        let root = run_future(
            &mut runtime,
            repo.get_bonsai_from_hg(
                ctx.clone(),
                HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
            ),
        )
        .unwrap()
        .unwrap();

        let bcs_id_0 = create_commit(
            ctx.clone(),
            repo.clone(),
            Vec::new(),
            store_files(
                ctx.clone(),
                btreemap! {"merge_file" => Some("merge content")},
                repo.clone(),
            ),
        );
        let bcs_id_1 = create_commit(
            ctx.clone(),
            repo.clone(),
            vec![bcs_id_0, root],
            store_files(
                ctx.clone(),
                btreemap! {"file" => Some("content")},
                repo.clone(),
            ),
        );
        let hgcss = vec![
            run_future(
                &mut runtime,
                repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_0),
            )
            .unwrap(),
            run_future(
                &mut runtime,
                repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id_1),
            )
            .unwrap(),
        ];

        let book = master_bookmark();
        set_bookmark(
            ctx.clone(),
            repo.clone(),
            &book.bookmark,
            "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
        );

        let config_forbid_p2 = PushrebaseParams {
            forbid_p2_root_rebases: true,
            ..Default::default()
        };
        match run_future(
            &mut runtime,
            do_pushrebase(
                ctx.clone(),
                repo.clone(),
                config_forbid_p2,
                book.clone(),
                hgcss.clone(),
                None,
            ),
        ) {
            Err(_) => (),
            _ => panic!("push-rebase should have failed"),
        };

        let config_allow_p2 = PushrebaseParams {
            forbid_p2_root_rebases: false,
            ..Default::default()
        };
        run_future(
            &mut runtime,
            do_pushrebase(ctx, repo, config_allow_p2, book, hgcss, None),
        )
        .expect("push-rebase failed");
    }
}
