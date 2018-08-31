// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

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
use blobrepo::{save_bonsai_changeset, BlobRepo};
use bonsai_utils::{bonsai_diff, BonsaiDiffResult};
use bookmarks::Bookmark;
use errors::*;
use futures::{Future, IntoFuture, Stream};
use futures::future::{err, join_all, loop_fn, ok, Loop};
use futures_ext::{BoxFuture, FutureExt};
use mercurial_types::{Changeset, HgChangesetId, MPath};
use metaconfig::PushrebaseParams;
use mononoke_types::{BonsaiChangeset, ChangesetId, DateTime, FileChange};

use revset::RangeNodeStream;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet, VecDeque};
use std::iter::FromIterator;
use std::sync::Arc;

#[derive(Debug)]
pub enum PushrebaseError {
    Conflicts(Vec<PushrebaseConflict>),
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

/// Does a pushrebase of a list of commits `pushed_set` onto `onto_bookmark`
/// The commits from the pushed set should already be committed to the blobrepo
/// Returns updated bookmark value.
pub fn do_pushrebase(
    repo: Arc<BlobRepo>,
    config: PushrebaseParams,
    onto_bookmark: Bookmark,
    pushed_set: Vec<HgChangesetId>,
) -> impl Future<Item = ChangesetId, Error = PushrebaseError> {
    fetch_bonsai_changesets(repo.clone(), pushed_set)
        .and_then(|pushed| {
            let head = find_only_head_or_fail(&pushed)?;
            let roots = find_roots(&pushed)?;

            Ok((head, roots))
        })
        .and_then({
            cloned!(config, repo, onto_bookmark);
            move |(head, roots)| {
                find_closest_root(&repo, config, onto_bookmark, roots).map(move |root| (head, root))
            }
        })
        .and_then({
            cloned!(repo);
            move |(head, root)| {
                // Calculate client changed files only once, since they won't change
                find_changed_files(&repo, root, head, /* reject_merges */ false).and_then(
                    move |client_cf| {
                        rebase_in_loop(repo, config, onto_bookmark, head, root, client_cf)
                    },
                )
            }
        })
}

fn rebase_in_loop(
    repo: Arc<BlobRepo>,
    config: PushrebaseParams,
    onto_bookmark: Bookmark,
    head: ChangesetId,
    root: ChangesetId,
    client_cf: Vec<MPath>,
) -> BoxFuture<ChangesetId, PushrebaseError> {
    loop_fn(root, move |root| {
        get_bookmark_value(&repo, &onto_bookmark).and_then({
            cloned!(client_cf, onto_bookmark, repo, config);
            move |bookmark_val| {
                find_changed_files(
                    &repo,
                    root.clone(),
                    bookmark_val,
                    /* reject_merges */ true,
                ).and_then(|server_cf| intersect_changed_files(server_cf, client_cf))
                    .and_then(move |()| {
                        do_rebase(repo, config, root, head, bookmark_val, onto_bookmark).map(
                            move |update_res| match update_res {
                                Some(result) => Loop::Break(result),
                                None => Loop::Continue(bookmark_val),
                            },
                        )
                    })
            }
        })
    }).boxify()
}

fn do_rebase(
    repo: Arc<BlobRepo>,
    config: PushrebaseParams,
    root: ChangesetId,
    head: ChangesetId,
    bookmark_val: ChangesetId,
    onto_bookmark: Bookmark,
) -> impl Future<Item = Option<ChangesetId>, Error = PushrebaseError> {
    create_rebased_changesets(repo.clone(), config, root, head, bookmark_val).and_then({
        move |new_head| try_update_bookmark(&repo, &onto_bookmark, bookmark_val, new_head)
    })
}

fn fetch_bonsai_changesets(
    repo: Arc<BlobRepo>,
    commit_ids: Vec<HgChangesetId>,
) -> impl Future<Item = Vec<BonsaiChangeset>, Error = PushrebaseError> {
    join_all(commit_ids.into_iter().map(move |hg_cs| {
        repo.get_bonsai_from_hg(&hg_cs)
            .and_then({
                cloned!(hg_cs);
                move |bcs_cs| bcs_cs.ok_or(ErrorKind::BonsaiNotFoundForHgChangeset(hg_cs).into())
            })
            .and_then({
                cloned!(repo);
                move |bcs_id| repo.get_bonsai_changeset(bcs_id).from_err()
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
            commits_set.remove(p);
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

fn find_roots(
    commits: &Vec<BonsaiChangeset>,
) -> ::std::result::Result<Vec<ChangesetId>, PushrebaseError> {
    let commits_set: HashSet<_> =
        HashSet::from_iter(commits.iter().map(|commit| commit.get_changeset_id()));

    let mut roots = vec![];
    for commit in commits {
        for p in commit.parents() {
            if !commits_set.contains(p) {
                roots.push(p.clone());
            }
        }
    }
    Ok(roots)
}

fn find_closest_root(
    repo: &Arc<BlobRepo>,
    config: PushrebaseParams,
    bookmark: Bookmark,
    roots: Vec<ChangesetId>,
) -> impl Future<Item = ChangesetId, Error = PushrebaseError> {
    let roots: HashSet<_> = roots.into_iter().collect();
    get_bookmark_value(repo, &bookmark).from_err().and_then({
        cloned!(repo);
        move |id| {
            let mut queue = VecDeque::new();
            queue.push_back(id);

            loop_fn((queue, 0), move |(mut queue, depth)| {
                if depth >= config.recursion_limit {
                    return err(PushrebaseError::RootTooFarBehind).left_future();
                }
                match queue.pop_front() {
                    None => err(PushrebaseError::Error(
                        ErrorKind::PushrebaseNoCommonRoot(bookmark.clone(), roots.clone()).into(),
                    )).left_future(),
                    Some(id) => {
                        if roots.contains(&id) {
                            ok(Loop::Break(id)).left_future()
                        } else {
                            repo.get_bonsai_changeset(id)
                                .map(move |bcs| {
                                    queue.extend(bcs.parents());
                                    Loop::Continue((queue, depth + 1))
                                })
                                .from_err()
                                .right_future()
                        }
                    }
                }
            })
        }
    })
}

/// find changed files by comparing manifests of `ancestor` and `descendant`
fn find_changed_files_between_manfiests(
    repo: &Arc<BlobRepo>,
    ancestor: ChangesetId,
    descendant: ChangesetId,
) -> impl Future<Item = Vec<MPath>, Error = PushrebaseError> {
    let id_to_manifest = {
        cloned!(repo);
        move |bcs_id| {
            repo.get_hg_from_bonsai_changeset(bcs_id)
                .and_then({
                    cloned!(repo);
                    move |cs_id| repo.get_changeset_by_changesetid(&cs_id)
                })
                .map({
                    cloned!(repo);
                    move |cs| repo.get_root_entry(cs.manifestid())
                })
        }
    };

    (id_to_manifest(descendant), id_to_manifest(ancestor))
        .into_future()
        .and_then(|(d_mf, a_mf)| {
            bonsai_diff(d_mf, Some(a_mf), None)
                .map(|diff| match diff {
                    BonsaiDiffResult::Changed(path, ..)
                    | BonsaiDiffResult::ChangedReusedId(path, ..)
                    | BonsaiDiffResult::Deleted(path) => path,
                })
                .collect()
        })
        .from_err()
}

fn find_changed_files(
    repo: &Arc<BlobRepo>,
    ancestor: ChangesetId,
    descendant: ChangesetId,
    reject_merges: bool,
) -> impl Future<Item = Vec<MPath>, Error = PushrebaseError> {
    cloned!(repo);
    RangeNodeStream::new(&repo, ancestor, descendant)
        .map({
            cloned!(repo);
            move |bcs_id| {
                repo.get_bonsai_changeset(bcs_id)
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
                        [] | [_] => ok(bcs.file_changes()
                            .map(|(path, _)| path.clone())
                            .collect::<Vec<MPath>>())
                            .left_future(),
                        [p0_id, p1_id] => {
                            if reject_merges {
                                return err(PushrebaseError::RebaseOverMerge).left_future();
                            }
                            match (ids.get(p0_id), ids.get(p1_id)) {
                                (Some(_), Some(_)) => {
                                    // both parents are in the rebase set, so we can just take
                                    // filechanges from bonsai changeset
                                    ok(bcs.file_changes()
                                        .map(|(path, _)| path.clone())
                                        .collect::<Vec<MPath>>())
                                        .left_future()
                                }
                                (Some(p_id), None) | (None, Some(p_id)) => {
                                    // one of the parents is not in the rebase set, to calculate
                                    // changed files in this case we will compute manifest diff
                                    // between elements that are in rebase set.
                                    find_changed_files_between_manfiests(&repo, id, *p_id)
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

fn get_bookmark_value(
    repo: &Arc<BlobRepo>,
    bookmark_name: &Bookmark,
) -> impl Future<Item = ChangesetId, Error = PushrebaseError> {
    repo.get_bookmark(bookmark_name)
        .and_then({
            cloned!(bookmark_name);
            move |bookmark| {
                bookmark.ok_or(ErrorKind::PushrebaseBookmarkNotFound(bookmark_name).into())
            }
        })
        .and_then({
            cloned!(repo);
            move |hg_bookmark_value| {
                repo.get_bonsai_from_hg(&hg_bookmark_value).and_then({
                    cloned!(hg_bookmark_value);
                    move |bonsai| {
                        bonsai.ok_or(
                            ErrorKind::BonsaiNotFoundForHgChangeset(hg_bookmark_value).into(),
                        )
                    }
                })
            }
        })
        .with_context(move |_| format!("While getting bookmark value"))
        .map_err(Error::from)
        .from_err()
}

fn create_rebased_changesets(
    repo: Arc<BlobRepo>,
    config: PushrebaseParams,
    root: ChangesetId,
    head: ChangesetId,
    onto: ChangesetId,
) -> impl Future<Item = ChangesetId, Error = PushrebaseError> {
    find_rebased_set(repo.clone(), root, head.clone()).and_then(move |rebased_set| {
        let date = if config.rewritedates {
            Some(DateTime::now())
        } else {
            None
        };

        // rebased_set already sorted in reverse topological order, which guarantees
        // that all required nodes will be updated by the time they are needed
        let mut remapping = hashmap!{ root => onto };
        let mut rebased = Vec::new();
        for bcs_old in rebased_set {
            let id_old = bcs_old.get_changeset_id();
            let bcs_new = match rebase_changeset(bcs_old, &remapping, date.as_ref()) {
                Ok(bcs_new) => bcs_new,
                Err(e) => return err(e.into()).left_future(),
            };
            remapping.insert(id_old, bcs_new.get_changeset_id());
            rebased.push(bcs_new);
        }

        // XXX: This can potentially be slow for long stacks. To speed it up we can write
        // all bonsai changests at once
        loop_fn(
            rebased.into_iter(),
            move |mut changesets| match changesets.next() {
                Some(bcs) => save_bonsai_changeset(bcs, (*repo).clone())
                    .map(|()| Loop::Continue(changesets))
                    .boxify(),
                None => ok(Loop::Break(())).boxify(),
            },
        ).map(move |_| remapping.get(&head).cloned().unwrap_or(head))
            .from_err()
            .right_future()
    })
}

fn rebase_changeset(
    bcs: BonsaiChangeset,
    remapping: &HashMap<ChangesetId, ChangesetId>,
    date: Option<&DateTime>,
) -> Result<BonsaiChangeset> {
    let mut bcs = bcs.into_mut();
    bcs.parents = bcs.parents
        .into_iter()
        .map(|p| remapping.get(&p).cloned().unwrap_or(p))
        .collect();

    match date {
        Some(date) => bcs.author_date = *date,
        None => (),
    }

    // Copy information in bonsai changeset contains a commit parent. So parent changes, then
    // copy information for all copied/moved files needs to be updated
    bcs.file_changes = bcs.file_changes
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
                            (path.clone(), remapping.get(cs).cloned().unwrap_or(*cs))
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
    repo: Arc<BlobRepo>,
    root: ChangesetId,
    head: ChangesetId,
) -> impl Future<Item = Vec<BonsaiChangeset>, Error = PushrebaseError> {
    RangeNodeStream::new(&repo, root, head)
        .map({
            cloned!(repo);
            move |bcs_id| repo.get_bonsai_changeset(bcs_id)
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
    repo: &Arc<BlobRepo>,
    bookmark_name: &Bookmark,
    old_value: ChangesetId,
    new_value: ChangesetId,
) -> BoxFuture<Option<ChangesetId>, PushrebaseError> {
    let mut txn = repo.update_bookmark_transaction();
    try_boxfuture!(txn.update(bookmark_name, &new_value, &old_value));
    txn.commit()
        .map(move |success| if success { Some(new_value) } else { None })
        .from_err()
        .boxify()
}

#[cfg(test)]
mod tests {

    use super::*;
    use async_unit;
    use bytes::Bytes;
    use fixtures::linear;
    use mononoke_types::{BonsaiChangesetMut, DateTime, FileChange, FileContents, FileType};
    use std::collections::BTreeMap;
    use std::str::FromStr;

    fn store_files(
        files: BTreeMap<&str, Option<&str>>,
        repo: BlobRepo,
    ) -> BTreeMap<MPath, Option<FileChange>> {
        let mut res = btreemap!{};

        for (path, content) in files {
            let path = MPath::new(path).unwrap();
            match content {
                Some(content) => {
                    let size = content.len();
                    let content = FileContents::Bytes(Bytes::from(content));
                    let content_id = repo.unittest_store(content).wait().unwrap();

                    let file_change =
                        FileChange::new(content_id, FileType::Regular, size as u64, None);
                    res.insert(path, Some(file_change));
                }
                None => {
                    res.insert(path, None);
                }
            }
        }
        res
    }

    fn store_rename(
        copy_src: (MPath, ChangesetId),
        path: &str,
        content: &str,
        repo: BlobRepo,
    ) -> (MPath, Option<FileChange>) {
        let path = MPath::new(path).unwrap();
        let size = content.len();
        let content = FileContents::Bytes(Bytes::from(content));
        let content_id = repo.unittest_store(content).wait().unwrap();

        let file_change =
            FileChange::new(content_id, FileType::Regular, size as u64, Some(copy_src));
        (path, Some(file_change))
    }

    fn create_commit(
        repo: BlobRepo,
        parents: Vec<ChangesetId>,
        file_changes: BTreeMap<MPath, Option<FileChange>>,
    ) -> ChangesetId {
        let bcs = BonsaiChangesetMut {
            parents: parents,
            author: "author".to_string(),
            author_date: DateTime::from_timestamp(0, 0).unwrap(),
            committer: None,
            committer_date: None,
            message: "message".to_string(),
            extra: btreemap!{},
            file_changes,
        }.freeze()
            .unwrap();

        let bcs_id = bcs.get_changeset_id();
        save_bonsai_changeset(bcs, repo.clone()).wait().unwrap();
        bcs_id
    }

    fn set_bookmark(repo: BlobRepo, book: &Bookmark, cs_id: &str) {
        let head = HgChangesetId::from_str(cs_id).unwrap();
        let head = repo.get_bonsai_from_hg(&head).wait().unwrap().unwrap();
        let mut txn = repo.update_bookmark_transaction();
        txn.force_set(&book, &head).unwrap();
        txn.commit().wait().unwrap();
    }

    fn make_paths(paths: &[&str]) -> Vec<MPath> {
        let paths: ::std::result::Result<_, _> = paths.into_iter().map(MPath::new).collect();
        paths.unwrap()
    }

    #[test]
    fn pushrebase_one_commit() {
        async_unit::tokio_unit_test(|| {
            let repo = linear::getrepo(None);
            // Bottom commit of the repo
            let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap();
            let p = repo.get_bonsai_from_hg(&root).wait().unwrap().unwrap();
            let parents = vec![p];

            let bcs_id = create_commit(
                repo.clone(),
                parents,
                store_files(btreemap!{"file" => Some("content")}, repo.clone()),
            );
            let hg_cs = repo.get_hg_from_bonsai_changeset(bcs_id).wait().unwrap();

            let book = Bookmark::new("master").unwrap();
            set_bookmark(
                repo.clone(),
                &book,
                "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
            );

            do_pushrebase(Arc::new(repo), Default::default(), book, vec![hg_cs])
                .wait()
                .expect("pushrebase failed");
        });
    }

    #[test]
    fn pushrebase_stack() {
        async_unit::tokio_unit_test(|| {
            let repo = linear::getrepo(None);
            // Bottom commit of the repo
            let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap();
            let p = repo.get_bonsai_from_hg(&root).wait().unwrap().unwrap();
            let bcs_id_1 = create_commit(
                repo.clone(),
                vec![p],
                store_files(btreemap!{"file" => Some("content")}, repo.clone()),
            );
            let bcs_id_2 = create_commit(
                repo.clone(),
                vec![bcs_id_1],
                store_files(btreemap!{"file2" => Some("content")}, repo.clone()),
            );

            assert_eq!(
                find_changed_files(&Arc::new(repo.clone()), p, bcs_id_2, false)
                    .wait()
                    .unwrap(),
                make_paths(&["file", "file2"]),
            );

            let book = Bookmark::new("master").unwrap();
            set_bookmark(
                repo.clone(),
                &book,
                "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
            );

            let hg_cs_1 = repo.get_hg_from_bonsai_changeset(bcs_id_1).wait().unwrap();
            let hg_cs_2 = repo.get_hg_from_bonsai_changeset(bcs_id_2).wait().unwrap();
            do_pushrebase(
                Arc::new(repo),
                Default::default(),
                book,
                vec![hg_cs_1, hg_cs_2],
            ).wait()
                .expect("pushrebase failed");
        });
    }

    #[test]
    fn pushrebase_stack_with_renames() {
        async_unit::tokio_unit_test(|| {
            let repo = linear::getrepo(None);
            // Bottom commit of the repo
            let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap();
            let p = repo.get_bonsai_from_hg(&root).wait().unwrap().unwrap();
            let bcs_id_1 = create_commit(
                repo.clone(),
                vec![p],
                store_files(btreemap!{"file" => Some("content")}, repo.clone()),
            );

            let rename = store_rename(
                (MPath::new("file").unwrap(), bcs_id_1),
                "file_renamed",
                "content",
                repo.clone(),
            );

            let file_changes = btreemap!{rename.0 => rename.1};
            let bcs_id_2 = create_commit(repo.clone(), vec![bcs_id_1], file_changes);

            assert_eq!(
                find_changed_files(&Arc::new(repo.clone()), p, bcs_id_2, false)
                    .wait()
                    .unwrap(),
                make_paths(&["file", "file_renamed"]),
            );

            let book = Bookmark::new("master").unwrap();
            set_bookmark(
                repo.clone(),
                &book,
                "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
            );

            let hg_cs_1 = repo.get_hg_from_bonsai_changeset(bcs_id_1).wait().unwrap();
            let hg_cs_2 = repo.get_hg_from_bonsai_changeset(bcs_id_2).wait().unwrap();
            do_pushrebase(
                Arc::new(repo),
                Default::default(),
                book,
                vec![hg_cs_1, hg_cs_2],
            ).wait()
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
            let repo = linear::getrepo(None);
            let repo_arc = Arc::new(repo.clone());
            let config = PushrebaseParams::default();

            let root0 = repo.get_bonsai_from_hg(&HgChangesetId::from_str(
                "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536",
            ).unwrap())
                .wait()
                .unwrap()
                .unwrap();

            let root1 = repo.get_bonsai_from_hg(&HgChangesetId::from_str(
                "607314ef579bd2407752361ba1b0c1729d08b281",
            ).unwrap())
                .wait()
                .unwrap()
                .unwrap();

            let bcs_id_1 = create_commit(
                repo.clone(),
                vec![root0],
                store_files(btreemap!{"f0" => Some("f0"), "files" => None}, repo.clone()),
            );
            let bcs_id_2 = create_commit(
                repo.clone(),
                vec![bcs_id_1, root1],
                store_files(btreemap!{"f1" => Some("f1")}, repo.clone()),
            );
            let bcs_id_3 = create_commit(
                repo.clone(),
                vec![bcs_id_2],
                store_files(btreemap!{"f2" => Some("f2")}, repo.clone()),
            );

            let book = Bookmark::new("master").unwrap();
            set_bookmark(
                repo.clone(),
                &book,
                "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
            );
            let bcs_id_master = repo.get_bonsai_from_hg(&HgChangesetId::from_str(
                "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
            ).unwrap())
                .wait()
                .unwrap()
                .unwrap();

            let root = root1;
            assert_eq!(
                find_closest_root(&repo_arc, config.clone(), book.clone(), vec![root0, root1])
                    .wait()
                    .unwrap(),
                root,
            );

            assert_eq!(
                find_changed_files(&repo_arc, root, bcs_id_3, false)
                    .wait()
                    .unwrap(),
                make_paths(&["f0", "f1", "f2"]),
            );

            let hg_cs_1 = repo.get_hg_from_bonsai_changeset(bcs_id_1).wait().unwrap();
            let hg_cs_2 = repo.get_hg_from_bonsai_changeset(bcs_id_2).wait().unwrap();
            let hg_cs_3 = repo.get_hg_from_bonsai_changeset(bcs_id_3).wait().unwrap();
            let bcs_id_rebased = do_pushrebase(
                repo_arc.clone(),
                config,
                book,
                vec![hg_cs_1, hg_cs_2, hg_cs_3],
            ).wait()
                .expect("pushrebase failed");

            // should only rebase {bcs2, bcs3}
            let rebased = find_rebased_set(repo_arc.clone(), bcs_id_master, bcs_id_rebased)
                .wait()
                .unwrap();
            assert_eq!(rebased.len(), 2);
            let bcs2 = &rebased[0];
            let bcs3 = &rebased[1];

            // bcs3 parent correctly updated and contains only {bcs2}
            assert_eq!(
                bcs3.parents().cloned().collect::<Vec<_>>(),
                vec![bcs2.get_changeset_id()]
            );

            // bcs2 parents cotains old bcs1 and old master bookmark
            assert_eq!(
                bcs2.parents().cloned().collect::<HashSet<_>>(),
                hashset!{ bcs_id_1, bcs_id_master },
            );
        });
    }

    #[test]
    fn pushrebase_conflict() {
        async_unit::tokio_unit_test(|| {
            let repo = linear::getrepo(None);
            let root = repo.get_bonsai_from_hg(&HgChangesetId::from_str(
                "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536",
            ).unwrap())
                .wait()
                .unwrap()
                .unwrap();

            let bcs_id_1 = create_commit(
                repo.clone(),
                vec![root],
                store_files(btreemap!{"f0" => Some("f0")}, repo.clone()),
            );
            let bcs_id_2 = create_commit(
                repo.clone(),
                vec![bcs_id_1],
                store_files(btreemap!{"9/file" => Some("file")}, repo.clone()),
            );
            let bcs_id_3 = create_commit(
                repo.clone(),
                vec![bcs_id_2],
                store_files(btreemap!{"f1" => Some("f1")}, repo.clone()),
            );

            let book = Bookmark::new("master").unwrap();
            set_bookmark(
                repo.clone(),
                &book,
                "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
            );

            let hg_cs_1 = repo.get_hg_from_bonsai_changeset(bcs_id_1).wait().unwrap();
            let hg_cs_2 = repo.get_hg_from_bonsai_changeset(bcs_id_2).wait().unwrap();
            let hg_cs_3 = repo.get_hg_from_bonsai_changeset(bcs_id_3).wait().unwrap();
            let result = do_pushrebase(
                Arc::new(repo),
                Default::default(),
                book,
                vec![hg_cs_1, hg_cs_2, hg_cs_3],
            ).wait();
            match result {
                Err(PushrebaseError::Conflicts(conflicts)) => {
                    assert_eq!(
                        conflicts,
                        vec![
                            PushrebaseConflict {
                                left: MPath::new("9").unwrap(),
                                right: MPath::new("9/file").unwrap(),
                            },
                        ],
                    );
                }
                _ => panic!("push-rebase should have failed with conflict"),
            }
        });
    }

    #[test]
    fn pushrebase_recursion_limit() {
        async_unit::tokio_unit_test(|| {
            let repo = linear::getrepo(None);
            let root = repo.get_bonsai_from_hg(&HgChangesetId::from_str(
                "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536",
            ).unwrap())
                .wait()
                .unwrap()
                .unwrap();

            // create a lot of commits
            let mut bcss = Vec::new();
            (0..128).fold(root, |head, index| {
                let file = format!("f{}", index);
                let content = format!("{}", index);
                let bcs = create_commit(
                    repo.clone(),
                    vec![head],
                    store_files(
                        btreemap!{file.as_ref() => Some(content.as_ref())},
                        repo.clone(),
                    ),
                );
                bcss.push(bcs);
                bcs
            });

            let hgcss = join_all(
                bcss.iter()
                    .map(|bcs| repo.get_hg_from_bonsai_changeset(*bcs))
                    .collect::<Vec<_>>(),
            ).wait()
                .unwrap();
            let book = Bookmark::new("master").unwrap();
            set_bookmark(
                repo.clone(),
                &book,
                "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
            );
            let repo_arc = Arc::new(repo.clone());
            do_pushrebase(repo_arc.clone(), Default::default(), book.clone(), hgcss)
                .wait()
                .expect("pushrebase failed");

            let bcs = create_commit(
                repo.clone(),
                vec![root],
                store_files(btreemap!{"file" => Some("data")}, repo.clone()),
            );
            let hgcss = vec![repo_arc.get_hg_from_bonsai_changeset(bcs).wait().unwrap()];

            // try rebase with small recursion limit
            let config = PushrebaseParams {
                recursion_limit: 128,
                ..Default::default()
            };
            let result =
                do_pushrebase(repo_arc.clone(), config, book.clone(), hgcss.clone()).wait();
            match result {
                Err(PushrebaseError::RootTooFarBehind) => (),
                _ => panic!("push-rebase should have failed because root too far behind"),
            }

            let config = PushrebaseParams {
                recursion_limit: 256,
                ..Default::default()
            };
            do_pushrebase(repo_arc, config, book, hgcss)
                .wait()
                .expect("push-rebase failed");
        })
    }

    #[test]
    fn pushrebase_rewritedates() {
        async_unit::tokio_unit_test(|| {
            let repo = linear::getrepo(None);
            let root = repo.get_bonsai_from_hg(&HgChangesetId::from_str(
                "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536",
            ).unwrap())
                .wait()
                .unwrap()
                .unwrap();
            let book = Bookmark::new("master").unwrap();
            let bcs = create_commit(
                repo.clone(),
                vec![root],
                store_files(btreemap!{"file" => Some("data")}, repo.clone()),
            );
            let hgcss = vec![repo.get_hg_from_bonsai_changeset(bcs).wait().unwrap()];

            set_bookmark(
                repo.clone(),
                &book,
                "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
            );
            let config = PushrebaseParams {
                rewritedates: false,
                ..Default::default()
            };
            let bcs_keep_date =
                do_pushrebase(Arc::new(repo.clone()), config, book.clone(), hgcss.clone())
                    .wait()
                    .expect("push-rebase failed");

            set_bookmark(
                repo.clone(),
                &book,
                "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
            );
            let config = PushrebaseParams {
                rewritedates: true,
                ..Default::default()
            };
            let bcs_rewrite_date = do_pushrebase(Arc::new(repo.clone()), config, book, hgcss)
                .wait()
                .expect("push-rebase failed");

            let bcs = repo.get_bonsai_changeset(bcs).wait().unwrap();
            let bcs_keep_date = repo.get_bonsai_changeset(bcs_keep_date).wait().unwrap();
            let bcs_rewrite_date = repo.get_bonsai_changeset(bcs_rewrite_date).wait().unwrap();

            assert_eq!(bcs.author_date(), bcs_keep_date.author_date());
            assert!(bcs.author_date() < bcs_rewrite_date.author_date());
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
}
