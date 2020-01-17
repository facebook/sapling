/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::{format_err, Error};
use blobrepo::BlobRepo;
use blobstore::{Blobstore, Loadable};
use cloned::cloned;
use context::CoreContext;
use derived_data::BonsaiDerived;
use futures::{
    future::{err, join_all, lazy, ok, Future, IntoFuture},
    stream::Stream,
    sync::{mpsc, oneshot},
};
use futures_ext::{bounded_traversal::bounded_traversal, BoxFuture, FutureExt};
use manifest::{Diff, ManifestOps, PathTree};
use mononoke_types::{blob::BlobstoreValue, deleted_files_manifest::DeletedManifest};
use mononoke_types::{BonsaiChangeset, ChangesetId, DeletedManifestId, MPathElement, MononokeId};
use repo_blobstore::RepoBlobstore;
use std::{collections::BTreeMap, iter::FromIterator, sync::Arc};
use thiserror::Error;
use unodes::{RootUnodeManifestId, RootUnodeManifestMapping};

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Failed to create deleted files manifest: {0}")]
    InvalidDeletedManifest(String),
    #[error("Deleted files manifest is not implemented for {0}")]
    DeletedManifestNotImplemented(String),
}

/// Derives deleted files manifest for bonsai changeset `cs_id` given parent deleted files
/// manifests and the changes associated with the changeset. Parent deleted manifests should be
/// constructed for each parent of the given changeset.
///
/// Deleted files manifest is a recursive data structure that starts with a root manifest and
/// points to the other manifests. Each node may represent either deleted directoty or deleted file.
/// Both directory's and file's manifest can have subentries, if a file has subentries it means
/// that this path was a directory earlier, then was deleted and reincarnated as a file.
///
/// Each manifest has an optional linknode. The initialized linknode points to the changeset where
/// the path was deleted. If linknode is not set, then manifest represents an existing
/// directory where some of the subentries (directories or files) have been deleted. There cannot
/// be a manifest without linknode and with no subentries.
///
/// Changes represent creations and deletions for both files and directories. They are applied
/// recursively starting from the root of parent manifest.
///
/// 1. If no files were deleted or created on the current path or any subpaths
///    - if there was corresponding deleted files manifest, reuse it;
///    - otherwise, there is no need to create a new node.
/// 2. If no change ends on the current path BUT there are creations/deletions on the subpaths,
///    recurse to the parent subentries and the current subpaths' changes
///    - if there are deleted subpaths (subentries are not empty), create a live manifest (manifest
///      without an empty linknode);
///    - if subentries are empty (all subpaths were restored), delete the current node.
/// 3. If current path was deleted, recurse to the parent subentries and the current subpaths'
///    changes
///   - create a deleted manifest for the current path and set linknode to the current changeset id.
/// 4. If current path was created, recurse to the parent subentries and the current subpaths'
///    changes
///   - if there was a corresponding manifest and there are no subentries, delete the node;
///   - if there are subentries, create a live manifest or mark the existing node as live.
/// 5. If there was a file/dir conflict (file was replaced with directory or other way round),
///    recurse to the parent subentries and the current subpaths' changes
///   - if there are subentries, create a live manifest or mark the existing node as live.
///
pub(crate) fn derive_deleted_files_manifest(
    ctx: CoreContext,
    repo: BlobRepo,
    cs_id: ChangesetId,
    parents: Vec<DeletedManifestId>,
    changes: PathTree<Option<PathChange>>,
) -> impl Future<Item = DeletedManifestId, Error = Error> {
    lazy(move || {
        let (result_sender, result_receiver) = oneshot::channel();
        // Stream is used to batch writes to blobstore
        let (sender, receiver) = mpsc::unbounded();
        let f = bounded_traversal(
            256,
            DeletedManifestUnfoldNode {
                path_element: None,
                changes,
                parents,
            },
            // unfold
            {
                cloned!(ctx, repo);
                move |DeletedManifestUnfoldNode {
                          path_element,
                          changes,
                          parents,
                      }| {
                    do_derive_unfold(ctx.clone(), repo.clone(), changes, parents).map(
                        move |(mf_change, next_states)| ((path_element, mf_change), next_states),
                    )
                }
            },
            // fold
            {
                cloned!(ctx, repo, sender);
                move |(path, manifest_change), subentries_iter| {
                    let mut subentries = BTreeMap::new();
                    for entry in subentries_iter {
                        match entry {
                            Some((Some(path), mf_id)) => {
                                subentries.insert(path, mf_id);
                            }
                            Some((None, _)) => {
                                return err(ErrorKind::InvalidDeletedManifest(
                                    "subentry must have a path".to_string(),
                                )
                                .into())
                                .boxify();
                            }
                            _ => {}
                        }
                    }

                    do_derive_create(
                        ctx.clone(),
                        repo.clone(),
                        cs_id.clone(),
                        manifest_change,
                        subentries,
                        sender.clone(),
                    )
                    .map(move |mf_id_opt| mf_id_opt.map(|mf_id| (path, mf_id)))
                    .boxify()
                }
            },
        )
        .and_then({
            cloned!(ctx, repo);
            move |manifest_opt| match manifest_opt {
                Some((_, mf_id)) => ok(mf_id).left_future(),
                None => {
                    // there is no deleted files, need to create an empty root manifest
                    create_manifest(
                        ctx.clone(),
                        repo.get_blobstore(),
                        None,
                        BTreeMap::new(),
                        sender.clone(),
                    )
                    .right_future()
                }
            }
        })
        .then(move |res| {
            // Error means receiver went away, just ignore it
            let _ = result_sender.send(res);
            Ok(())
        });

        tokio::spawn(f);
        let blobstore_put_stream = receiver.map_err(|()| Error::msg("receiver failed"));

        blobstore_put_stream
            .buffered(1024)
            .for_each(|_| Ok(()))
            .and_then(move |()| result_receiver.from_err().and_then(|res| res).boxify())
    })
}

pub(crate) enum PathChange {
    Add,
    Remove,
    FileDirConflict,
}

pub(crate) fn get_changes(
    ctx: CoreContext,
    repo: BlobRepo,
    bonsai: &BonsaiChangeset,
) -> BoxFuture<PathTree<Option<PathChange>>, Error> {
    let blobstore = repo.get_blobstore();
    // Get file/directory changes between the current changeset and its parents
    //
    // get unode manifests first
    let unode_mapping = Arc::new(RootUnodeManifestMapping::new(blobstore.clone()));
    let bcs_id = bonsai.get_changeset_id();

    // get parent unodes
    let parent_cs_ids: Vec<_> = bonsai.parents().collect();
    let parent_unodes = parent_cs_ids.into_iter().map({
        cloned!(ctx, repo, unode_mapping);
        move |cs_id| {
            RootUnodeManifestId::derive(ctx.clone(), repo.clone(), unode_mapping.clone(), cs_id)
                .map(|root_mf_id| root_mf_id.manifest_unode_id().clone())
        }
    });
    RootUnodeManifestId::derive(ctx.clone(), repo.clone(), unode_mapping.clone(), bcs_id)
        .join(join_all(parent_unodes))
        // compute diff between changeset's and its parents' manifests
        .and_then({
            cloned!(ctx, blobstore);
            move |(root_unode_mf_id, parent_mf_ids)| {
                let unode_mf_id = root_unode_mf_id.manifest_unode_id().clone();
                match *parent_mf_ids {
                    [] => {
                        unode_mf_id
                            .list_all_entries(ctx.clone(), blobstore)
                            .filter_map(move |(path, _)| match path {
                                Some(path) => Some((path, PathChange::Add)),
                                None => None,
                            })
                            .collect()
                            .boxify()
                    }
                    [parent_mf_id] => {
                        parent_mf_id
                            .diff(ctx.clone(), blobstore, unode_mf_id)
                            .collect()
                            .map(move |diffs| {
                                let mut changes = BTreeMap::new();
                                for diff in diffs {
                                    let change = match diff {
                                        Diff::Added(Some(path), _) => Some((path, PathChange::Add)),
                                        Diff::Removed(Some(path), _) => Some((path, PathChange::Remove)),
                                        _ => None,
                                    };
                                    if let Some((path, change)) = change {
                                        // If the changeset has file/dir conflict the diff between
                                        // parent manifests and the current will have two entries
                                        // for the same path: one to remove the file/dir, another
                                        // to introduce new dir/file node.
                                        changes.entry(path).and_modify(|e| { *e = PathChange::FileDirConflict }).or_insert(change);
                                    }
                                }
                                changes.into_iter().collect()
                            })
                            .boxify()
                    }
                    _ => {
                        return err(ErrorKind::DeletedManifestNotImplemented(
                                "non-linear history".to_string()
                            )
                            .into()
                        )
                        .boxify();
                    }
                }
            }
        })
        .map(move |changes| PathTree::from_iter(changes.into_iter().map(|(path, change)| (path, Some(change)))))
        .boxify()
}

enum DeletedManifestChange {
    AddOrKeepDeleted,
    RemoveOrKeepLive,
    Reuse(Option<DeletedManifestId>),
}

struct DeletedManifestUnfoldNode {
    path_element: Option<MPathElement>,
    changes: PathTree<Option<PathChange>>,
    parents: Vec<DeletedManifestId>,
}

fn do_derive_unfold(
    ctx: CoreContext,
    repo: BlobRepo,
    changes: PathTree<Option<PathChange>>,
    parents: Vec<DeletedManifestId>,
) -> impl Future<Item = (DeletedManifestChange, Vec<DeletedManifestUnfoldNode>), Error = Error> {
    let PathTree {
        value: change,
        subentries,
    } = changes;

    if parents.len() > 1 {
        return err(
            ErrorKind::DeletedManifestNotImplemented("non-linear history".to_string()).into(),
        )
        .right_future();
    }

    let parent = parents.first().copied();
    let fold_node = match change {
        None => {
            if subentries.is_empty() {
                // nothing changed in the current node and in the subentries
                return ok((DeletedManifestChange::Reuse(parent), vec![])).right_future();
            } else {
                // some paths might be added/deleted
                DeletedManifestChange::RemoveOrKeepLive
            }
        }
        Some(PathChange::Add) => {
            // the path was added
            DeletedManifestChange::RemoveOrKeepLive
        }
        Some(PathChange::Remove) => {
            // the path was removed
            DeletedManifestChange::AddOrKeepDeleted
        }
        Some(PathChange::FileDirConflict) => {
            // This is a file/dir conflict: either a file was replaced by directory or other way
            // round. In both cases one of the paths is being deleted and recreated as other
            // type. To keep this in history, we need to mark the path as deleted in the deleted
            // files manifest.
            DeletedManifestChange::RemoveOrKeepLive
        }
    };

    // some files might be added/removed in subentries, need to traverse the subentries
    let mut recurse_entries = BTreeMap::new();
    for (path, change_tree) in subentries {
        recurse_entries.insert(
            path.clone(),
            DeletedManifestUnfoldNode {
                path_element: Some(path),
                changes: change_tree,
                parents: vec![],
            },
        );
    }

    let parent_entries = if let Some(parent) = parents.first() {
        parent
            .load(ctx.clone(), repo.blobstore())
            .from_err()
            .map(move |parent_mf| {
                parent_mf
                    .list()
                    .map(|(path, mf_id)| (path.clone(), mf_id.clone()))
                    .collect::<Vec<_>>()
            })
            .left_future()
    } else {
        ok(vec![]).right_future()
    };

    parent_entries
        .map(move |entries| {
            for (path, mf_id) in entries {
                let entry =
                    recurse_entries
                        .entry(path.clone())
                        .or_insert(DeletedManifestUnfoldNode {
                            path_element: Some(path.clone()),
                            changes: Default::default(),
                            parents: vec![],
                        });
                entry.parents.push(mf_id);
            }

            (
                fold_node,
                recurse_entries
                    .into_iter()
                    .map(|(_, node)| node)
                    .collect::<Vec<_>>(),
            )
        })
        .left_future()
}

fn create_manifest(
    ctx: CoreContext,
    blobstore: RepoBlobstore,
    linknode: Option<ChangesetId>,
    subentries: BTreeMap<MPathElement, DeletedManifestId>,
    sender: mpsc::UnboundedSender<BoxFuture<(), Error>>,
) -> BoxFuture<DeletedManifestId, Error> {
    let manifest = DeletedManifest::new(linknode, subentries);
    let mf_id = manifest.get_manifest_id();

    let key = mf_id.blobstore_key();
    let blob = manifest.into_blob();
    let f = lazy(move || blobstore.put(ctx, key, blob.into())).boxify();

    sender
        .unbounded_send(f)
        .into_future()
        .map(move |()| mf_id)
        .map_err(|err| format_err!("failed to send manifest future {}", err))
        .boxify()
}

fn do_derive_create(
    ctx: CoreContext,
    repo: BlobRepo,
    cs_id: ChangesetId,
    change: DeletedManifestChange,
    subentries: BTreeMap<MPathElement, DeletedManifestId>,
    sender: mpsc::UnboundedSender<BoxFuture<(), Error>>,
) -> impl Future<Item = Option<DeletedManifestId>, Error = Error> {
    let blobstore = repo.get_blobstore();
    match change {
        DeletedManifestChange::Reuse(mb_mf_id) => ok(mb_mf_id).boxify(),
        DeletedManifestChange::AddOrKeepDeleted => {
            create_manifest(ctx.clone(), blobstore, Some(cs_id), subentries, sender)
                .map(Some)
                .boxify()
        }
        DeletedManifestChange::RemoveOrKeepLive => {
            if subentries.is_empty() {
                // there are no subentries, no need to create a new node
                ok(None).left_future()
            } else {
                // some of the subentries were deleted, creating a new node but there is no need to
                // mark it as deleted
                create_manifest(ctx.clone(), blobstore, None, subentries, sender)
                    .map(Some)
                    .right_future()
            }
        }
        .boxify(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blobrepo::save_bonsai_changesets;
    use blobrepo_factory::new_memblob_empty;
    use fbinit::FacebookInit;
    use fixtures::{many_files_dirs, store_files};
    use futures::stream::iter_ok;
    use futures_ext::bounded_traversal::bounded_traversal_stream;
    use maplit::btreemap;
    use mononoke_types::{BonsaiChangeset, BonsaiChangesetMut, DateTime, FileChange, MPath};
    use test_utils::get_bonsai_changeset;
    use tokio_compat::runtime::Runtime;

    #[fbinit::test]
    fn linear_test(fb: FacebookInit) {
        // Test simple separate files and whole dir deletions
        let repo = new_memblob_empty(None).unwrap();
        let mut runtime = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock(fb);

        // create parent deleted files manifest
        let (bcs_id_1, mf_id_1) = {
            let file_changes = btreemap! {
                "file.txt" => Some("1\n"),
                "file-2.txt" => Some("2\n"),
                "dir/sub/f-1" => Some("3\n"),
                "dir/f-2" => Some("4\n"),
                "dir-2/sub/f-3" => Some("5\n"),
                "dir-2/f-4" => Some("6\n"),
            };
            let (bcs_id, mf_id, deleted_nodes) = create_cs_and_derive_manifest(
                ctx.clone(),
                repo.clone(),
                &mut runtime,
                file_changes,
                vec![],
            );

            // nothing was deleted yet
            let expected_nodes = vec![(None, Status::Live)];
            assert_eq!(deleted_nodes, expected_nodes);

            (bcs_id, mf_id)
        };

        // delete some files and dirs
        let (bcs_id_2, mf_id_2) = {
            let file_changes = btreemap! {
                "file.txt" => None,
                "file-2.txt" => Some("2\n2\n"),
                "file-3.txt" => Some("3\n3\n"),
                "dir/sub/f-1" => None,
                "dir/f-2" => None,
                "dir-2/sub/f-3" => None,
            };
            let (bcs_id, mf_id, deleted_nodes) = create_cs_and_derive_manifest(
                ctx.clone(),
                repo.clone(),
                &mut runtime,
                file_changes,
                vec![(bcs_id_1, mf_id_1)],
            );

            let expected_nodes = vec![
                (None, Status::Live),
                (Some(path("dir")), Status::Deleted(bcs_id)),
                (Some(path("dir/f-2")), Status::Deleted(bcs_id)),
                (Some(path("dir/sub")), Status::Deleted(bcs_id)),
                (Some(path("dir/sub/f-1")), Status::Deleted(bcs_id)),
                (Some(path("dir-2")), Status::Live),
                (Some(path("dir-2/sub")), Status::Deleted(bcs_id)),
                (Some(path("dir-2/sub/f-3")), Status::Deleted(bcs_id)),
                (Some(path("file.txt")), Status::Deleted(bcs_id)),
            ];
            assert_eq!(deleted_nodes, expected_nodes);

            (bcs_id, mf_id)
        };

        // reincarnate file and directory
        let (bcs_id_3, mf_id_3) = {
            let file_changes = btreemap! {
                "file.txt" => Some("1\n1\n1\n"),
                "file-2.txt" => None,
                "dir/sub/f-4" => Some("4\n4\n4\n"),
            };
            let (bcs_id, mf_id, deleted_nodes) = create_cs_and_derive_manifest(
                ctx.clone(),
                repo.clone(),
                &mut runtime,
                file_changes,
                vec![(bcs_id_2, mf_id_2)],
            );

            let expected_nodes = vec![
                (None, Status::Live),
                (Some(path("dir")), Status::Live),
                (Some(path("dir/f-2")), Status::Deleted(bcs_id_2)),
                (Some(path("dir/sub")), Status::Live),
                (Some(path("dir/sub/f-1")), Status::Deleted(bcs_id_2)),
                (Some(path("dir-2")), Status::Live),
                (Some(path("dir-2/sub")), Status::Deleted(bcs_id_2)),
                (Some(path("dir-2/sub/f-3")), Status::Deleted(bcs_id_2)),
                (Some(path("file-2.txt")), Status::Deleted(bcs_id)),
            ];
            assert_eq!(deleted_nodes, expected_nodes);

            (bcs_id, mf_id)
        };

        // reincarnate file as dir and dir as file
        let (bcs_id_4, mf_id_4) = {
            let file_changes = btreemap! {
                // file as dir
                "file-2.txt/subfile.txt" => Some("2\n2\n1\n"),
                // dir as file
                "dir-2/sub" => Some("file now!\n"),
            };
            let (bcs_id, mf_id, deleted_nodes) = create_cs_and_derive_manifest(
                ctx.clone(),
                repo.clone(),
                &mut runtime,
                file_changes,
                vec![(bcs_id_3, mf_id_3)],
            );

            let expected_nodes = vec![
                (None, Status::Live),
                (Some(path("dir")), Status::Live),
                (Some(path("dir/f-2")), Status::Deleted(bcs_id_2)),
                (Some(path("dir/sub")), Status::Live),
                (Some(path("dir/sub/f-1")), Status::Deleted(bcs_id_2)),
                (Some(path("dir-2")), Status::Live),
                (Some(path("dir-2/sub")), Status::Live),
                (Some(path("dir-2/sub/f-3")), Status::Deleted(bcs_id_2)),
            ];
            assert_eq!(deleted_nodes, expected_nodes);

            (bcs_id, mf_id)
        };

        // delete everything
        {
            let file_changes = btreemap! {
                "file.txt" => None,
                "file-2.txt/subfile.txt" => None,
                "file-3.txt" => None,
                "dir-2/f-4" => None,
                "dir-2/sub" => None,
                "dir/sub/f-4" => None,
            };
            let (bcs_id, mf_id, deleted_nodes) = create_cs_and_derive_manifest(
                ctx.clone(),
                repo.clone(),
                &mut runtime,
                file_changes,
                vec![(bcs_id_4, mf_id_4)],
            );

            let expected_nodes = vec![
                (None, Status::Live),
                (Some(path("dir")), Status::Deleted(bcs_id)),
                (Some(path("dir/f-2")), Status::Deleted(bcs_id_2)),
                (Some(path("dir/sub")), Status::Deleted(bcs_id)),
                (Some(path("dir/sub/f-1")), Status::Deleted(bcs_id_2)),
                (Some(path("dir/sub/f-4")), Status::Deleted(bcs_id)),
                (Some(path("dir-2")), Status::Deleted(bcs_id)),
                (Some(path("dir-2/f-4")), Status::Deleted(bcs_id)),
                (Some(path("dir-2/sub")), Status::Deleted(bcs_id)),
                (Some(path("dir-2/sub/f-3")), Status::Deleted(bcs_id_2)),
                (Some(path("file-2.txt")), Status::Deleted(bcs_id)),
                (
                    Some(path("file-2.txt/subfile.txt")),
                    Status::Deleted(bcs_id),
                ),
                (Some(path("file-3.txt")), Status::Deleted(bcs_id)),
                (Some(path("file.txt")), Status::Deleted(bcs_id)),
            ];
            assert_eq!(deleted_nodes, expected_nodes);

            (bcs_id, mf_id)
        };
    }

    #[fbinit::test]
    fn many_file_dirs_test(fb: FacebookInit) {
        let repo = many_files_dirs::getrepo(fb);
        let mut runtime = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock(fb);

        let mf_id_1 = {
            let hg_cs = "5a28e25f924a5d209b82ce0713d8d83e68982bc8";
            let (_, bcs) = get_bonsai_changeset(ctx.clone(), repo.clone(), &mut runtime, hg_cs);

            let (_, mf_id, deleted_nodes) =
                derive_manifest(ctx.clone(), repo.clone(), &mut runtime, bcs, vec![]);

            // nothing was deleted yet
            let expected_nodes = vec![(None, Status::Live)];
            assert_eq!(deleted_nodes, expected_nodes);
            mf_id
        };

        let mf_id_2 = {
            let hg_cs = "2f866e7e549760934e31bf0420a873f65100ad63";
            let (_, bcs) = get_bonsai_changeset(ctx.clone(), repo.clone(), &mut runtime, hg_cs);

            let (_, mf_id, deleted_nodes) =
                derive_manifest(ctx.clone(), repo.clone(), &mut runtime, bcs, vec![mf_id_1]);

            // nothing was deleted yet
            let expected_nodes = vec![(None, Status::Live)];
            assert_eq!(deleted_nodes, expected_nodes);
            mf_id
        };

        let mf_id_3 = {
            let hg_cs = "d261bc7900818dea7c86935b3fb17a33b2e3a6b4";
            let (_, bcs) = get_bonsai_changeset(ctx.clone(), repo.clone(), &mut runtime, hg_cs);

            let (_, mf_id, deleted_nodes) =
                derive_manifest(ctx.clone(), repo.clone(), &mut runtime, bcs, vec![mf_id_2]);

            // nothing was deleted yet
            let expected_nodes = vec![(None, Status::Live)];
            assert_eq!(deleted_nodes, expected_nodes);
            mf_id
        };

        {
            let hg_cs = "051946ed218061e925fb120dac02634f9ad40ae2";
            let (bcs_id, bcs) =
                get_bonsai_changeset(ctx.clone(), repo.clone(), &mut runtime, hg_cs);

            let (_, mf_id, deleted_nodes) =
                derive_manifest(ctx.clone(), repo.clone(), &mut runtime, bcs, vec![mf_id_3]);

            let expected_nodes = vec![
                (None, Status::Live),
                (Some(path("dir1")), Status::Live),
                (Some(path("dir1/file_1_in_dir1")), Status::Deleted(bcs_id)),
                (Some(path("dir1/file_2_in_dir1")), Status::Deleted(bcs_id)),
                (Some(path("dir1/subdir1")), Status::Deleted(bcs_id)),
                (Some(path("dir1/subdir1/file_1")), Status::Deleted(bcs_id)),
                (
                    Some(path("dir1/subdir1/subsubdir1")),
                    Status::Deleted(bcs_id),
                ),
                (
                    Some(path("dir1/subdir1/subsubdir1/file_1")),
                    Status::Deleted(bcs_id),
                ),
                (
                    Some(path("dir1/subdir1/subsubdir2")),
                    Status::Deleted(bcs_id),
                ),
                (
                    Some(path("dir1/subdir1/subsubdir2/file_1")),
                    Status::Deleted(bcs_id),
                ),
                (
                    Some(path("dir1/subdir1/subsubdir2/file_2")),
                    Status::Deleted(bcs_id),
                ),
            ];
            assert_eq!(deleted_nodes, expected_nodes);
            mf_id
        };
    }

    fn create_cs_and_derive_manifest(
        ctx: CoreContext,
        repo: BlobRepo,
        mut runtime: &mut Runtime,
        file_changes: BTreeMap<&str, Option<&str>>,
        parent_ids: Vec<(ChangesetId, DeletedManifestId)>,
    ) -> (ChangesetId, DeletedManifestId, Vec<(Option<MPath>, Status)>) {
        let parent_bcs_ids = parent_ids
            .iter()
            .map(|(bs, _)| bs.clone())
            .collect::<Vec<_>>();
        let parent_mf_ids = parent_ids.into_iter().map(|(_, mf)| mf).collect::<Vec<_>>();

        let bcs = create_bonsai_changeset(
            ctx.fb,
            repo.clone(),
            &mut runtime,
            store_files(ctx.clone(), file_changes, repo.clone()),
            parent_bcs_ids,
        );

        derive_manifest(ctx.clone(), repo.clone(), &mut runtime, bcs, parent_mf_ids)
    }

    fn derive_manifest(
        ctx: CoreContext,
        repo: BlobRepo,
        runtime: &mut Runtime,
        bcs: BonsaiChangeset,
        parent_mf_ids: Vec<DeletedManifestId>,
    ) -> (ChangesetId, DeletedManifestId, Vec<(Option<MPath>, Status)>) {
        let bcs_id = bcs.get_changeset_id();

        let changes = runtime
            .block_on(get_changes(ctx.clone(), repo.clone(), &bcs))
            .unwrap();
        let f = derive_deleted_files_manifest(
            ctx.clone(),
            repo.clone(),
            bcs_id,
            parent_mf_ids,
            changes,
        );

        let dfm_id = runtime.block_on(f).unwrap();
        // Make sure it's saved in the blobstore
        runtime
            .block_on(dfm_id.load(ctx.clone(), repo.blobstore()))
            .unwrap();

        let mut deleted_nodes = runtime
            .block_on(
                iterate_all_entries(ctx.clone(), repo.clone(), dfm_id.clone())
                    .map(|(path, st, ..)| (path, st))
                    .collect(),
            )
            .unwrap();
        deleted_nodes.sort_by_key(|(path, ..)| path.clone());

        (bcs_id, dfm_id, deleted_nodes)
    }

    fn create_bonsai_changeset(
        fb: FacebookInit,
        repo: BlobRepo,
        runtime: &mut Runtime,
        file_changes: BTreeMap<MPath, Option<FileChange>>,
        parents: Vec<ChangesetId>,
    ) -> BonsaiChangeset {
        let bcs = BonsaiChangesetMut {
            parents,
            author: "author".to_string(),
            author_date: DateTime::now(),
            committer: None,
            committer_date: None,
            message: "message".to_string(),
            extra: btreemap! {},
            file_changes,
        }
        .freeze()
        .unwrap();

        runtime
            .block_on(save_bonsai_changesets(
                vec![bcs.clone()],
                CoreContext::test_mock(fb),
                repo.clone(),
            ))
            .unwrap();
        bcs
    }

    #[derive(Debug, Clone, Eq, PartialEq)]
    enum Status {
        Deleted(ChangesetId),
        Live,
    }

    impl From<Option<ChangesetId>> for Status {
        fn from(linknode: Option<ChangesetId>) -> Self {
            linknode.map(Status::Deleted).unwrap_or(Status::Live)
        }
    }

    fn iterate_all_entries(
        ctx: CoreContext,
        repo: BlobRepo,
        manifest_id: DeletedManifestId,
    ) -> impl Stream<Item = (Option<MPath>, Status, DeletedManifestId), Error = Error> {
        let blobstore = repo.get_blobstore();
        bounded_traversal_stream(
            256,
            Some((None, manifest_id)),
            move |(path, manifest_id)| {
                manifest_id
                    .load(ctx.clone(), &blobstore)
                    .map(move |manifest| {
                        let entry = (
                            path.clone(),
                            Status::from(manifest.linknode().clone()),
                            manifest_id,
                        );
                        let recurse_subentries = manifest
                            .list()
                            .map(|(name, mf_id)| {
                                let full_path = MPath::join_opt_element(path.as_ref(), &name);
                                (Some(full_path), mf_id.clone())
                            })
                            .collect::<Vec<_>>();

                        (vec![entry], recurse_subentries)
                    })
            },
        )
        .map(|entries| iter_ok(entries))
        .flatten()
    }

    fn path(path_str: &str) -> MPath {
        MPath::new(path_str).unwrap()
    }
}
