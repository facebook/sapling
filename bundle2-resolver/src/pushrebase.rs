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
use bookmarks::Bookmark;
use errors::*;
use futures::Future;
use futures::future::{join_all, loop_fn, ok, Loop};
use futures_ext::{BoxFuture, FutureExt};
use mercurial_types::{HgChangesetId, MPath};
use mononoke_types::{BonsaiChangeset, ChangesetId};

use std::collections::HashSet;
use std::iter::FromIterator;
use std::sync::Arc;

#[allow(dead_code)]
#[derive(Debug)]
pub enum PushrebaseError {
    Conflicts,
    Error(Error),
}

impl From<Error> for PushrebaseError {
    fn from(error: Error) -> Self {
        PushrebaseError::Error(error)
    }
}

/// Does a pushrebase of a list of commits `pushed_set` onto `onto_bookmark`
/// The commits from the pushed set should already be committed to the blobrepo
pub fn do_pushrebase(
    repo: Arc<BlobRepo>,
    onto_bookmark: Bookmark,
    pushed_set: Vec<HgChangesetId>,
) -> impl Future<Item = (), Error = PushrebaseError> {
    fetch_bonsai_changesets(repo.clone(), pushed_set)
        .and_then(|pushed| {
            let head = find_only_head_or_fail(&pushed)?;
            let roots = find_roots(&pushed)?;

            Ok((head, roots))
        })
        .and_then({
            let repo = repo.clone();
            let onto_bookmark = onto_bookmark.clone();
            move |(head, roots)| {
                find_closest_root(&repo, onto_bookmark, roots).map(move |root| (head, root))
            }
        })
        .and_then({
            let repo = repo.clone();
            move |(head, root)| {
                // Calculate client changed files only once, since they won't change
                find_changed_files(&repo, root, head).and_then(move |client_cf| {
                    rebase_in_loop(repo, onto_bookmark, head, root, client_cf)
                })
            }
        })
}

fn rebase_in_loop(
    repo: Arc<BlobRepo>,
    onto_bookmark: Bookmark,
    head: ChangesetId,
    root: ChangesetId,
    client_cf: Vec<MPath>,
) -> BoxFuture<(), PushrebaseError> {
    loop_fn(root, move |root| {
        get_bookmark_value(&repo, &onto_bookmark).and_then({
            cloned!(client_cf, onto_bookmark, repo);
            move |bookmark_val| {
                find_changed_files(&repo, root.clone(), bookmark_val)
                    .and_then(|server_cf| intersect_changed_files(server_cf, client_cf))
                    .and_then(move |()| {
                        do_rebase(repo, root, head, bookmark_val, onto_bookmark).map(
                            move |update_res| {
                                if update_res {
                                    Loop::Break(())
                                } else {
                                    Loop::Continue(bookmark_val)
                                }
                            },
                        )
                    })
            }
        })
    }).boxify()
}

fn do_rebase(
    repo: Arc<BlobRepo>,
    root: ChangesetId,
    head: ChangesetId,
    bookmark_val: ChangesetId,
    onto_bookmark: Bookmark,
) -> impl Future<Item = bool, Error = PushrebaseError> {
    create_rebased_changesets(repo.clone(), root, head, bookmark_val).and_then({
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
    _repo: &Arc<BlobRepo>,
    _bookmark: Bookmark,
    roots: Vec<ChangesetId>,
) -> impl Future<Item = ChangesetId, Error = PushrebaseError> {
    // TODO(stash, aslpavel): actually find closest root

    if roots.len() == 1 {
        ok(roots.get(0).unwrap().clone())
    } else {
        unimplemented!()
    }
}

fn find_changed_files(
    _repo: &Arc<BlobRepo>,
    _ancestor: ChangesetId,
    _descendant: ChangesetId,
) -> impl Future<Item = Vec<MPath>, Error = PushrebaseError> {
    // TODO(stash, aslpavel) actually find changed files
    ok(vec![])
}

fn intersect_changed_files(
    _left: Vec<MPath>,
    _right: Vec<MPath>,
) -> ::std::result::Result<(), PushrebaseError> {
    // TODO(stash, aslpavel) actually find intersection
    Ok(())
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
    root: ChangesetId,
    head: ChangesetId,
    onto: ChangesetId,
) -> impl Future<Item = ChangesetId, Error = PushrebaseError> {
    // TODO(stash, aslpavel) at the moment it rebases just one commit
    repo.get_bonsai_changeset(head)
        .and_then(move |bcs| {
            {
                let parents: Vec<_> = bcs.parents().collect();
                if parents.len() != 1 {
                    unimplemented!()
                }
                if parents != vec![&root] {
                    unimplemented!()
                }
            }

            let mut bcs = bcs.into_mut();
            bcs.parents[0] = onto;
            bcs.freeze()
        })
        .and_then({
            cloned!(repo);
            move |bcs| {
                // TODO(stash): avoid .deref().clone(), get rid of Arc<BlobRepo>
                let repo: &BlobRepo = &repo;
                let bcs_id = bcs.get_changeset_id();
                save_bonsai_changeset(bcs, repo.clone()).map(move |()| bcs_id)
            }
        })
        .with_context(move |_| format!("While creating rebased changesets"))
        .map_err(Error::from)
        .from_err()
}

fn try_update_bookmark(
    repo: &Arc<BlobRepo>,
    bookmark_name: &Bookmark,
    old_value: ChangesetId,
    new_value: ChangesetId,
) -> BoxFuture<bool, PushrebaseError> {
    let mut txn = repo.update_bookmark_transaction();
    try_boxfuture!(txn.update(bookmark_name, &new_value, &old_value));
    txn.commit().from_err().boxify()
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

    #[test]
    fn pushrebase_one_commit() {
        async_unit::tokio_unit_test(|| {
            let repo = linear::getrepo(None);
            let file_changes = store_files(btreemap!{"file" => Some("content")}, repo.clone());

            // Bottom commit of the repo
            let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap();
            let p = repo.get_bonsai_from_hg(&root).wait().unwrap().unwrap();
            let parents = vec![p];

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
            let hg_cs = repo.get_hg_from_bonsai_changeset(bcs_id).wait().unwrap();

            let book = Bookmark::new("master").unwrap();
            let head = HgChangesetId::from_str("a5ffa77602a066db7d5cfb9fb5823a0895717c5a").unwrap();
            let head = repo.get_bonsai_from_hg(&head).wait().unwrap().unwrap();
            let mut txn = repo.update_bookmark_transaction();
            txn.force_set(&book, &head).unwrap();
            txn.commit().wait().unwrap();

            do_pushrebase(Arc::new(repo), book, vec![hg_cs])
                .wait()
                .expect("pushrebase failed");
        });
    }
}
