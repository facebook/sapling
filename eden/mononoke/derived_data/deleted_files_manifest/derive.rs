/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, format_err, Error};
use blobstore::{Blobstore, Loadable};
use borrowed::borrowed;
use bounded_traversal::bounded_traversal;
use cloned::cloned;
use context::CoreContext;
use derived_data_manager::DerivationContext;
use futures::{
    channel::{mpsc, oneshot},
    future::{self, BoxFuture, FutureExt},
    stream::{StreamExt, TryStreamExt},
};
use manifest::{Diff, ManifestOps, PathTree};
use mononoke_types::{
    deleted_manifest_common::DeletedManifestCommon, BonsaiChangeset, ChangesetId, MPath,
    MPathElement, ManifestUnodeId, MononokeId,
};
use sorted_vector_map::SortedVectorMap;
use std::sync::Arc;
use std::{collections::BTreeMap, collections::HashSet};
use tokio::sync::Mutex;
use unodes::RootUnodeManifestId;

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
pub(crate) async fn derive_deleted_files_manifest<Manifest: DeletedManifestCommon>(
    ctx: &CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    cs_id: ChangesetId,
    parents: Vec<Manifest::Id>,
    changes: PathTree<Option<PathChange>>,
) -> Result<Manifest::Id, Error> {
    let (result_sender, result_receiver) = oneshot::channel();
    // Stream is used to batch writes to blobstore
    let (sender, receiver) = mpsc::unbounded();
    let created = Arc::new(Mutex::new(HashSet::new()));
    cloned!(blobstore, ctx);
    let f = async move {
        borrowed!(ctx, blobstore);
        let manifest_opt = bounded_traversal(
            256,
            DeletedManifestUnfoldNode {
                path_element: None,
                changes,
                parents,
            },
            // unfold
            {
                move |
                    DeletedManifestUnfoldNode {
                        path_element,
                        changes,
                        parents,
                    },
                | {
                    async move {
                        let (mf_change, next_states) =
                            do_derive_unfold::<Manifest>(ctx, blobstore, changes, parents).await?;
                        Ok(((path_element, mf_change), next_states))
                    }
                    .boxed()
                }
            },
            // fold
            {
                cloned!(sender, created);
                move |
                    (path, manifest_change),
                    // impl Iterator<Out>
                    subentries_iter,
                    // -> Out = Option<(Option<MPathElement>, Manifest::Id)>
                    // None means a leaf node was deleted because the file was recreated
                    // Option<MPathElement> is a possibly empty path (None if empty)
                |  {
                    cloned!(cs_id, sender, created);
                    async move {
                        let mut subentries = SortedVectorMap::new();
                        for entry in subentries_iter {
                            match entry {
                                Some((Some(path), mf_id)) => {
                                    subentries.insert(path, mf_id);
                                }
                                Some((None, _)) => {
                                    return Err(anyhow!(concat!(
                                        "Failed to create deleted files manifest: ",
                                        "subentry must have a path"
                                    )));
                                }
                                None => {}
                            }
                        }

                        Ok(do_derive_create::<Manifest>(
                            ctx,
                            blobstore,
                            cs_id.clone(),
                            manifest_change,
                            subentries,
                            sender.clone(),
                            created.clone(),
                        )
                        .await?
                        .map(|mf_id| (path, mf_id)))
                    }
                    .boxed()
                }
            },
        )
        .await?;

        let res = match manifest_opt {
            Some((_, mf_id)) => Ok(mf_id),
            None => {
                // there are no deleted files, need to create an empty root manifest
                create_manifest::<Manifest>(
                    ctx,
                    blobstore,
                    None,
                    Default::default(),
                    sender.clone(),
                    created.clone(),
                )
                .await
            }
        };

        let _ = result_sender.send(res);
        Result::<_, Error>::Ok(())
    };

    tokio::spawn(f);

    receiver
        .buffered(1024)
        .try_for_each(|_| async { Ok(()) })
        .await?;
    result_receiver.await?
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum PathChange {
    Add,
    Remove,
    FileDirConflict,
}

pub(crate) async fn get_changes(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsai: BonsaiChangeset,
) -> Result<PathTree<Option<PathChange>>, Error> {
    // Get file/directory changes between the current changeset and its parents
    //
    // get unode manifests first
    let bcs_id = bonsai.get_changeset_id();

    // get parent unodes
    let parent_cs_ids: Vec<_> = bonsai.parents().collect();
    let parent_unodes = parent_cs_ids.into_iter().map({
        move |cs_id| async move {
            let root_mf_id = derivation_ctx
                .derive_dependency::<RootUnodeManifestId>(ctx, cs_id)
                .await?;
            Ok(root_mf_id.manifest_unode_id().clone())
        }
    });

    let (root_unode_mf_id, parent_mf_ids) = future::try_join(
        derivation_ctx.derive_dependency::<RootUnodeManifestId>(ctx, bcs_id),
        future::try_join_all(parent_unodes),
    )
    .await?;

    // compute diff between changeset's and its parents' manifests
    let unode_mf_id = root_unode_mf_id.manifest_unode_id().clone();
    let changes = if parent_mf_ids.is_empty() {
        unode_mf_id
            .list_all_entries(ctx.clone(), derivation_ctx.blobstore().clone())
            .try_filter_map(move |(path, _)| async {
                match path {
                    Some(path) => Ok(Some((path, PathChange::Add))),
                    None => Ok(None),
                }
            })
            .try_collect::<Vec<_>>()
            .await
    } else {
        diff_against_parents(ctx, derivation_ctx, unode_mf_id, parent_mf_ids).await
    }?;

    Ok(PathTree::from_iter(
        changes
            .into_iter()
            .map(|(path, change)| (path, Some(change))),
    ))
}

async fn diff_against_parents(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    unode: ManifestUnodeId,
    parents: Vec<ManifestUnodeId>,
) -> Result<Vec<(MPath, PathChange)>, Error> {
    let blobstore = derivation_ctx.blobstore();
    let parent_diffs_fut = parents.into_iter().map({
        cloned!(ctx, blobstore, unode);
        move |parent| {
            parent
                .diff(ctx.clone(), blobstore.clone(), unode.clone())
                .try_collect::<Vec<_>>()
        }
    });
    let parent_diffs = future::try_join_all(parent_diffs_fut).await?;
    let diffs = parent_diffs
        .into_iter()
        .flatten()
        .filter_map(|diff| match diff {
            Diff::Added(Some(path), _) => Some((path, PathChange::Add)),
            Diff::Removed(Some(path), _) => Some((path, PathChange::Remove)),
            _ => None,
        });

    let mut changes = BTreeMap::new();
    for (path, change) in diffs {
        // If the changeset has file/dir conflict the diff between
        // parent manifests and the current will have two entries
        // for the same path: one to remove the file/dir, another
        // to introduce new dir/file node.
        changes
            .entry(path)
            .and_modify(|e| {
                if *e != change {
                    *e = PathChange::FileDirConflict
                }
            })
            .or_insert(change);
    }
    let res: Vec<_> = changes.into_iter().collect();
    Ok(res)
}

enum DeletedManifestChange<Manifest: DeletedManifestCommon> {
    CreateDeleted,
    RemoveOrKeepLive,
    Reuse(Option<Manifest::Id>),
}

struct DeletedManifestUnfoldNode<Manifest: DeletedManifestCommon> {
    path_element: Option<MPathElement>,
    changes: PathTree<Option<PathChange>>,
    parents: Vec<Manifest::Id>,
}

async fn do_derive_unfold<Manifest: DeletedManifestCommon>(
    ctx: &CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    changes: PathTree<Option<PathChange>>,
    parents: Vec<Manifest::Id>,
) -> Result<
    (
        DeletedManifestChange<Manifest>,
        Vec<DeletedManifestUnfoldNode<Manifest>>,
    ),
    Error,
> {
    let PathTree {
        value: change,
        subentries,
    } = changes;

    let parent_manifests =
        future::try_join_all(parents.iter().map(|mf_id| mf_id.load(ctx, blobstore))).await?;

    let check_consistency = |manifests: &Vec<Manifest>| {
        let mut it = manifests.iter().map(|mf| mf.is_deleted());
        if let Some(status) = it.next() {
            if it.all(|st| st == status) {
                return Ok(status);
            }
            return Err(format_err!(
                "parent deleted manifests have different node status, but no changes were provided"
            ));
        }
        Ok(false)
    };

    let fold_node = match change {
        None => {
            if subentries.is_empty() {
                // nothing changed in the current node and in the subentries
                // if parent manifests are equal, we can reuse them
                let mut it = parents.into_iter();
                if let Some(id) = it.next() {
                    if it.all(|mf| mf == id) {
                        return Ok((DeletedManifestChange::Reuse(Some(id)), vec![]));
                    }
                    // parent manifests are different, we need to merge them
                    // let's check that the node status is consistent across parents
                    let is_deleted = check_consistency(&parent_manifests)?;
                    if is_deleted {
                        DeletedManifestChange::CreateDeleted
                    } else {
                        DeletedManifestChange::RemoveOrKeepLive
                    }
                } else {
                    return Ok((DeletedManifestChange::Reuse(None), vec![]));
                }
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
            DeletedManifestChange::CreateDeleted
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

    for parent in parent_manifests {
        for (path, mf_id) in parent.into_subentries() {
            let entry = recurse_entries
                .entry(path.clone())
                .or_insert(DeletedManifestUnfoldNode {
                    path_element: Some(path),
                    changes: Default::default(),
                    parents: vec![],
                });
            entry.parents.push(mf_id);
        }
    }

    Ok((
        fold_node,
        recurse_entries
            .into_iter()
            .map(|(_, node)| node)
            .collect::<Vec<_>>(),
    ))
}

async fn create_manifest<Manifest: DeletedManifestCommon>(
    ctx: &CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    linknode: Option<ChangesetId>,
    subentries: SortedVectorMap<MPathElement, Manifest::Id>,
    sender: mpsc::UnboundedSender<BoxFuture<'static, Result<(), Error>>>,
    created: Arc<Mutex<HashSet<String>>>,
) -> Result<Manifest::Id, Error> {
    let manifest = Manifest::new(linknode, subentries);
    let mf_id = manifest.id();

    let key = mf_id.blobstore_key();
    let mut created = created.lock().await;
    if created.insert(key.clone()) {
        let blob = manifest.into_blob();
        cloned!(ctx, blobstore);
        let f = async move { blobstore.put(&ctx, key, blob.into()).await }.boxed();

        sender
            .unbounded_send(f)
            .map(move |()| mf_id)
            .map_err(|err| anyhow!("failed to send manifest future {}", err))
    } else {
        Ok(mf_id)
    }
}

async fn do_derive_create<Manifest: DeletedManifestCommon>(
    ctx: &CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    cs_id: ChangesetId,
    change: DeletedManifestChange<Manifest>,
    subentries: SortedVectorMap<MPathElement, Manifest::Id>,
    sender: mpsc::UnboundedSender<BoxFuture<'static, Result<(), Error>>>,
    created: Arc<Mutex<HashSet<String>>>,
) -> Result<Option<Manifest::Id>, Error> {
    match change {
        DeletedManifestChange::Reuse(mb_mf_id) => Ok(mb_mf_id),
        DeletedManifestChange::CreateDeleted => {
            create_manifest::<Manifest>(ctx, blobstore, Some(cs_id), subentries, sender, created)
                .await
                .map(Some)
        }
        DeletedManifestChange::RemoveOrKeepLive => {
            if subentries.is_empty() {
                // there are no subentries, no need to create a new node
                Ok(None)
            } else {
                // some of the subentries were deleted, creating a new node but there is no need to
                // mark it as deleted
                create_manifest::<Manifest>(ctx, blobstore, None, subentries, sender, created)
                    .await
                    .map(Some)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mapping::RootDeletedManifestId;
    use blobrepo::{save_bonsai_changesets, BlobRepo};
    use bounded_traversal::bounded_traversal_stream;
    use derived_data_test_utils::bonsai_changeset_from_hg;
    use fbinit::FacebookInit;
    use fixtures::{many_files_dirs, store_files};
    use futures::{pin_mut, stream::iter, Stream, TryStreamExt};
    use maplit::btreemap;
    use mononoke_types::{
        deleted_files_manifest::DeletedManifest, BonsaiChangeset, BonsaiChangesetMut, DateTime,
        DeletedManifestId, FileChange, MPath,
    };
    use repo_derived_data::RepoDerivedDataRef;
    use sorted_vector_map::SortedVectorMap;
    use tests_utils::CreateCommitContext;

    #[fbinit::test]
    async fn linear_test(fb: FacebookInit) {
        // Test simple separate files and whole dir deletions
        let repo: BlobRepo = test_repo_factory::build_empty().unwrap();
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
            let (bcs_id, mf_id, deleted_nodes) =
                create_cs_and_derive_manifest(ctx.clone(), repo.clone(), file_changes, vec![])
                    .await;

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
                file_changes,
                vec![(bcs_id_1, mf_id_1)],
            )
            .await;

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
                file_changes,
                vec![(bcs_id_2, mf_id_2)],
            )
            .await;

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
                file_changes,
                vec![(bcs_id_3, mf_id_3)],
            )
            .await;

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
                file_changes,
                vec![(bcs_id_4, mf_id_4)],
            )
            .await;

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
    async fn many_file_dirs_test(fb: FacebookInit) {
        let repo = many_files_dirs::getrepo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let mf_id_1 = {
            let hg_cs = "5a28e25f924a5d209b82ce0713d8d83e68982bc8";
            let (_, bcs) = bonsai_changeset_from_hg(&ctx, &repo, hg_cs).await.unwrap();

            let (_, mf_id, deleted_nodes) = derive_manifest(&ctx, &repo, bcs, vec![]).await;

            // nothing was deleted yet
            let expected_nodes = vec![(None, Status::Live)];
            assert_eq!(deleted_nodes, expected_nodes);
            mf_id
        };

        let mf_id_2 = {
            let hg_cs = "2f866e7e549760934e31bf0420a873f65100ad63";
            let (_, bcs) = bonsai_changeset_from_hg(&ctx, &repo, hg_cs).await.unwrap();

            let (_, mf_id, deleted_nodes) = derive_manifest(&ctx, &repo, bcs, vec![mf_id_1]).await;

            // nothing was deleted yet
            let expected_nodes = vec![(None, Status::Live)];
            assert_eq!(deleted_nodes, expected_nodes);
            mf_id
        };

        let mf_id_3 = {
            let hg_cs = "d261bc7900818dea7c86935b3fb17a33b2e3a6b4";
            let (_, bcs) = bonsai_changeset_from_hg(&ctx, &repo, hg_cs).await.unwrap();

            let (_, mf_id, deleted_nodes) = derive_manifest(&ctx, &repo, bcs, vec![mf_id_2]).await;

            // nothing was deleted yet
            let expected_nodes = vec![(None, Status::Live)];
            assert_eq!(deleted_nodes, expected_nodes);
            mf_id
        };

        {
            let hg_cs = "051946ed218061e925fb120dac02634f9ad40ae2";
            let (bcs_id, bcs) = bonsai_changeset_from_hg(&ctx, &repo, hg_cs).await.unwrap();

            let (_, mf_id, deleted_nodes) = derive_manifest(&ctx, &repo, bcs, vec![mf_id_3]).await;

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

    #[fbinit::test]
    async fn merged_history_test(fb: FacebookInit) -> Result<(), Error> {
        //
        //  N
        //  | \
        //  K  M
        //  |  |
        //  J  L
        //  | /
        //  I
        //  | \
        //  |  H
        //  |  |
        //  |  G
        //  |  | \
        //  |  D  F
        //  |  |  |
        //  B  C  E
        //  | /
        //  A
        //
        let repo: BlobRepo = test_repo_factory::build_empty().unwrap();
        let ctx = CoreContext::test_mock(fb);

        let a = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file", "1")
            .add_file("dir/file", "2")
            .add_file("dir_2/file", "3")
            .add_file("dir_3/file_1", "1")
            .add_file("dir_3/file_2", "2")
            .commit()
            .await?;

        let b = CreateCommitContext::new(&ctx, &repo, vec![a.clone()])
            .delete_file("file")
            .delete_file("dir/file")
            .delete_file("dir_3/file_1")
            .add_file("dir/file_2", "file->file_2")
            .commit()
            .await?;
        let deleted_nodes = gen_deleted_manifest_nodes(&ctx, &repo, b.clone()).await?;
        let expected_nodes = vec![
            (None, Status::Live),
            (Some(path("dir")), Status::Live),
            (Some(path("dir/file")), Status::Deleted(b)),
            (Some(path("dir_3")), Status::Live),
            (Some(path("dir_3/file_1")), Status::Deleted(b)),
            (Some(path("file")), Status::Deleted(b)),
        ];
        assert_eq!(deleted_nodes, expected_nodes);

        let c = CreateCommitContext::new(&ctx, &repo, vec![a.clone()])
            .add_file("file", "1->2")
            .commit()
            .await?;

        let d = CreateCommitContext::new(&ctx, &repo, vec![c.clone()])
            .delete_file("dir/file")
            .delete_file("dir_2/file")
            .commit()
            .await?;

        let deleted_nodes = gen_deleted_manifest_nodes(&ctx, &repo, d.clone()).await?;
        let expected_nodes = vec![
            (None, Status::Live),
            (Some(path("dir")), Status::Deleted(d)),
            (Some(path("dir/file")), Status::Deleted(d)),
            (Some(path("dir_2")), Status::Deleted(d)),
            (Some(path("dir_2/file")), Status::Deleted(d)),
        ];
        assert_eq!(deleted_nodes, expected_nodes);

        let e = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file", "3")
            .add_file("dir_2/file", "4")
            .commit()
            .await?;

        let f = CreateCommitContext::new(&ctx, &repo, vec![e.clone()])
            .delete_file("file")
            .add_file("dir_2/file", "4->5")
            .commit()
            .await?;

        // first merge commit:
        // * dir_2/file - was deleted in branch D and modified in F, merge commit
        //   accepts modification. It means the file must be restored.
        // * file - was changed in branch D and deleted in F, merge commit accepts
        //   deletion. It means new deleted manifet node must be created and must
        //   point to the merge commit.
        // * dir/file - existed and was deleted in the one branch and never
        //   existed in the other, but still must be discoverable.
        let g = CreateCommitContext::new(&ctx, &repo, vec![d.clone(), f.clone()])
            .delete_file("file")
            .add_file("dir_2/file", "4->5")
            .add_file("dir_2/file_2", "5")
            .commit()
            .await?;

        let deleted_nodes = gen_deleted_manifest_nodes(&ctx, &repo, g.clone()).await?;
        let expected_nodes = vec![
            (None, Status::Live),
            (Some(path("dir")), Status::Deleted(d)),
            (Some(path("dir/file")), Status::Deleted(d)),
            (Some(path("file")), Status::Deleted(g)),
        ];
        assert_eq!(deleted_nodes, expected_nodes);

        let h = CreateCommitContext::new(&ctx, &repo, vec![g.clone()])
            .delete_file("dir_3/file_2")
            .add_file("dir_2/file", "4->5")
            .add_file("dir_2/file_2", "5")
            .commit()
            .await?;

        let deleted_nodes = gen_deleted_manifest_nodes(&ctx, &repo, h.clone()).await?;
        let expected_nodes = vec![
            (None, Status::Live),
            (Some(path("dir")), Status::Deleted(d)),
            (Some(path("dir/file")), Status::Deleted(d)),
            (Some(path("dir_3")), Status::Live),
            (Some(path("dir_3/file_2")), Status::Deleted(h)),
            (Some(path("file")), Status::Deleted(g)),
        ];
        assert_eq!(deleted_nodes, expected_nodes);

        // second merge commit
        // * dir/file - is deleted in both branches, new manifest node must
        //   have linknode pointed to the merge commit
        // * file - same as for dir/file
        // * dir - still exists because of dir/file_2
        let i = CreateCommitContext::new(&ctx, &repo, vec![b.clone(), h.clone()])
            .delete_file("dir_3/file_1")
            .delete_file("dir_3/file_2")
            .add_file("dir_2/file", "4->5")
            .add_file("dir_5/file_1", "5.1")
            .add_file("dir_5/file_2", "5.2")
            .commit()
            .await?;
        let deleted_nodes = gen_deleted_manifest_nodes(&ctx, &repo, i.clone()).await?;
        let expected_nodes = vec![
            (None, Status::Live),
            (Some(path("dir")), Status::Live),
            (Some(path("dir/file")), Status::Deleted(i)),
            (Some(path("dir_3")), Status::Deleted(i)),
            (Some(path("dir_3/file_1")), Status::Deleted(i)),
            (Some(path("dir_3/file_2")), Status::Deleted(i)),
            (Some(path("file")), Status::Deleted(i)),
        ];
        assert_eq!(deleted_nodes, expected_nodes);

        // this commit creates a file in a new dir
        // and deletes one of the dir_5 files
        let j = CreateCommitContext::new(&ctx, &repo, vec![i.clone()])
            .delete_file("dir_5/file_1")
            .add_file("dir_4/file_1", "new")
            .commit()
            .await?;

        // this commit deletes the file created in its parent j
        // and adds a new file and dir
        let k = CreateCommitContext::new(&ctx, &repo, vec![j.clone()])
            .delete_file("dir_4/file_1")
            .add_file("dir_to_file/file", "will be replaced")
            .commit()
            .await?;

        // this commit creates a file in the same dir as the other branch
        // and deletes one of the dir_5 files
        let l = CreateCommitContext::new(&ctx, &repo, vec![i.clone()])
            .delete_file("dir_5/file_2")
            .add_file("dir_4/file_2", "new")
            .commit()
            .await?;

        // this commit deletes the file created in its parent l
        let m = CreateCommitContext::new(&ctx, &repo, vec![l.clone()])
            .delete_file("dir_4/file_2")
            .commit()
            .await?;

        // third merge commit
        // * dir_4/file_1 - is created and then deleted in the branch K,
        //   linknode for the merge commit N must point to the commit K
        // * dir_4/file_2 - is created and then deleted in the branch M,
        //   linknode for the merge commit N must point to the commit M
        // * dir_4 - existed in both branches, linknode should point to
        //   the merge commit itself
        // * dir_5/file_1 - existed in both branches, but deleted in J,
        //   linknode for the merge commit N must point to the N itself
        // * dir_5/file_2 - existed in both branches, but deleted in L,
        //   linknode for the merge commit N must point to the N itself
        // * dir_5 - existed in both branches, but as a result of merge
        //   must be deleted, linknode should point to N
        // * dir_to_file/file is replaced here with dir_to_file, this
        //   should result in dir_to_file node live and dir_to_file/file
        //   deleted
        let n = CreateCommitContext::new(&ctx, &repo, vec![k.clone(), m.clone()])
            .delete_file("dir_5/file_1")
            .delete_file("dir_5/file_2")
            .add_file("dir_to_file", "replaced!")
            .commit()
            .await?;

        let deleted_nodes = gen_deleted_manifest_nodes(&ctx, &repo, n.clone()).await?;
        let expected_nodes = vec![
            (None, Status::Live),
            (Some(path("dir")), Status::Live),
            (Some(path("dir/file")), Status::Deleted(i)),
            (Some(path("dir_3")), Status::Deleted(i)),
            (Some(path("dir_3/file_1")), Status::Deleted(i)),
            (Some(path("dir_3/file_2")), Status::Deleted(i)),
            (Some(path("dir_4")), Status::Deleted(n)),
            (Some(path("dir_4/file_1")), Status::Deleted(k)),
            (Some(path("dir_4/file_2")), Status::Deleted(m)),
            (Some(path("dir_5")), Status::Deleted(n)),
            (Some(path("dir_5/file_1")), Status::Deleted(n)),
            (Some(path("dir_5/file_2")), Status::Deleted(n)),
            (Some(path("dir_to_file")), Status::Live),
            (Some(path("dir_to_file/file")), Status::Deleted(n)),
            (Some(path("file")), Status::Deleted(i)),
        ];
        assert_eq!(deleted_nodes, expected_nodes);

        Ok(())
    }

    async fn gen_deleted_manifest_nodes(
        ctx: &CoreContext,
        repo: &BlobRepo,
        bonsai: ChangesetId,
    ) -> Result<Vec<(Option<MPath>, Status)>, Error> {
        let manifest = repo
            .repo_derived_data()
            .manager()
            .derive::<RootDeletedManifestId>(ctx, bonsai, None)
            .await?;
        let mut deleted_nodes =
            iterate_all_entries(ctx.clone(), repo.clone(), *manifest.deleted_manifest_id())
                .map_ok(|(path, st, ..)| (path, st))
                .try_collect::<Vec<_>>()
                .await?;
        deleted_nodes.sort_by_key(|(path, ..)| path.clone());
        Ok(deleted_nodes)
    }

    async fn create_cs_and_derive_manifest(
        ctx: CoreContext,
        repo: BlobRepo,
        file_changes: BTreeMap<&str, Option<&str>>,
        parent_ids: Vec<(ChangesetId, DeletedManifestId)>,
    ) -> (ChangesetId, DeletedManifestId, Vec<(Option<MPath>, Status)>) {
        let parent_bcs_ids = parent_ids
            .iter()
            .map(|(bs, _)| bs.clone())
            .collect::<Vec<_>>();
        let parent_mf_ids = parent_ids.into_iter().map(|(_, mf)| mf).collect::<Vec<_>>();

        let files = store_files(&ctx, file_changes, &repo).await;

        let bcs = create_bonsai_changeset(ctx.fb, repo.clone(), files, parent_bcs_ids).await;

        derive_manifest(&ctx, &repo, bcs, parent_mf_ids).await
    }

    async fn derive_manifest(
        ctx: &CoreContext,
        repo: &BlobRepo,
        bcs: BonsaiChangeset,
        parent_mf_ids: Vec<DeletedManifestId>,
    ) -> (ChangesetId, DeletedManifestId, Vec<(Option<MPath>, Status)>) {
        let blobstore = repo.blobstore().boxed();
        let bcs_id = bcs.get_changeset_id();

        let changes = get_changes(
            ctx,
            &repo.repo_derived_data().manager().derivation_context(None),
            bcs,
        )
        .await
        .unwrap();
        let f = derive_deleted_files_manifest::<DeletedManifest>(
            ctx,
            &blobstore,
            bcs_id,
            parent_mf_ids,
            changes,
        );

        let dfm_id = f.await.unwrap();
        // Make sure it's saved in the blobstore
        dfm_id.load(&ctx, &blobstore).await.unwrap();

        let mut deleted_nodes = iterate_all_entries(ctx.clone(), repo.clone(), dfm_id.clone())
            .map_ok(|(path, st, ..)| (path, st))
            .try_collect::<Vec<_>>()
            .await
            .unwrap();
        deleted_nodes.sort_by_key(|(path, ..)| path.clone());

        (bcs_id, dfm_id, deleted_nodes)
    }

    async fn create_bonsai_changeset(
        fb: FacebookInit,
        repo: BlobRepo,
        file_changes: SortedVectorMap<MPath, FileChange>,
        parents: Vec<ChangesetId>,
    ) -> BonsaiChangeset {
        let bcs = BonsaiChangesetMut {
            parents,
            author: "author".to_string(),
            author_date: DateTime::now(),
            committer: None,
            committer_date: None,
            message: "message".to_string(),
            extra: Default::default(),
            file_changes,
            is_snapshot: false,
        }
        .freeze()
        .unwrap();

        save_bonsai_changesets(vec![bcs.clone()], CoreContext::test_mock(fb), &repo)
            .await
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
    ) -> impl Stream<Item = Result<(Option<MPath>, Status, DeletedManifestId), Error>> {
        async_stream::stream! {
            let blobstore = repo.get_blobstore();
            let s = bounded_traversal_stream(256, Some((None, manifest_id)), move |(path, manifest_id)| {
                cloned!(ctx, blobstore);
                async move {
                    let manifest = manifest_id.load(&ctx, &blobstore).await?;
                    let entry = (
                        path.clone(),
                        Status::from(manifest.linknode().clone()),
                        manifest_id,
                    );
                    let recurse_subentries = manifest
                        .into_subentries()
                        .map(|(name, mf_id)| {
                            let full_path = MPath::join_opt_element(path.as_ref(), &name);
                            (Some(full_path), mf_id)
                        })
                        .collect::<Vec<_>>();

                    Result::<_, Error>::Ok((vec![entry], recurse_subentries))
                }.boxed()
            })
            .map_ok(|entries| iter(entries.into_iter().map(Ok)))
            .try_flatten();

            pin_mut!(s);
            while let Some(value) = s.next().await {
                yield value;
            }
        }
    }

    fn path(path_str: &str) -> MPath {
        MPath::new(path_str).unwrap()
    }
}
