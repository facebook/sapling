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
    channel::mpsc,
    future::{self, BoxFuture, FutureExt},
    stream::{StreamExt, TryStreamExt},
};
use manifest::{Diff, ManifestOps, PathTree};
use mononoke_types::{
    deleted_manifest_common::DeletedManifestCommon, BonsaiChangeset, ChangesetId, MPath,
    MPathElement, ManifestUnodeId, MononokeId,
};
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
pub(crate) struct DeletedManifestDeriver<Manifest: DeletedManifestCommon>(
    std::marker::PhantomData<Manifest>,
);

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum PathChange {
    Add,
    Remove,
    FileDirConflict,
}

enum DeletedManifestChangeType {
    /// Path was deleted, we create a node if not present.
    CreateDeleted,
    /// Path now exists, delete if it doesn't have any subentries that were
    /// previous deleted.
    RemoveIfNowEmpty,
    /// No changes to the path which has a single parent, reuse the parent.
    Reuse,
}

struct DeletedManifestChange<Manifest: DeletedManifestCommon> {
    /// Which change happened.
    change_type: DeletedManifestChangeType,
    /// Parent to base on. Result should be equivalent to copying the subentries
    /// of the parent and then applying the remanining modifications.
    copy_subentries_from: Option<Manifest>,
}

struct DeletedManifestUnfoldNode<Manifest: DeletedManifestCommon> {
    path_element: Option<MPathElement>,
    changes: PathTree<Option<PathChange>>,
    // set is used to automatically deduplicate parents that have equal ancestors
    parents: HashSet<Manifest::Id>,
}

impl<Manifest: DeletedManifestCommon> DeletedManifestDeriver<Manifest> {
    pub(crate) async fn derive(
        ctx: &CoreContext,
        blobstore: &Arc<dyn Blobstore>,
        cs_id: ChangesetId,
        parents: Vec<Manifest::Id>,
        changes: PathTree<Option<PathChange>>,
    ) -> Result<Manifest::Id, Error> {
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
                    parents: parents.into_iter().collect(),
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
                                Self::do_unfold(ctx, blobstore, changes, parents).await?;
                            Ok(((path_element, mf_change), next_states))
                        }
                        .boxed()
                    }
                },
                // fold
                {
                    cloned!(sender, created);
                    move |
                        (path, manifest_change): (
                            Option<MPathElement>,
                            DeletedManifestChange<Manifest>,
                        ),
                        // impl Iterator<Out>
                        subentries_iter,
                        // -> Out = (Option<MPathElement>, Option<Manifest::Id>)
                        // (_, None) means a leaf node was deleted because the file was recreated.
                        // (None, _) means the path is empty and should only happen on the root.
                    | {
                        cloned!(cs_id, sender, created);
                        async move {
                            let mut subentries_to_update = BTreeMap::new();
                            for entry in subentries_iter {
                                match entry {
                                    (None, _) => {
                                        return Err(anyhow!(concat!(
                                            "Failed to create deleted files manifest: ",
                                            "subentry must have a path"
                                        )));
                                    }
                                    (Some(path), maybe_mf_id) => {
                                        subentries_to_update.insert(path, maybe_mf_id);
                                    }
                                }
                            }

                            let maybe_mf_id = Self::do_create(
                                ctx,
                                blobstore,
                                cs_id.clone(),
                                manifest_change,
                                subentries_to_update,
                                sender.clone(),
                                created.clone(),
                            )
                            .await?;

                            Ok((path, maybe_mf_id))
                        }
                        .boxed()
                    }
                },
            )
            .await?;

            debug_assert!(manifest_opt.0.is_none());
            match manifest_opt {
                (_, Some(mf_id)) => Ok(mf_id),
                (_, None) => {
                    // there are no deleted files, need to create an empty root manifest
                    match Manifest::copy_and_update_subentries(
                        ctx,
                        blobstore,
                        None,
                        None,
                        BTreeMap::new(),
                    )
                    .await
                    {
                        Ok(mf) => {
                            Self::save_manifest(mf, ctx, blobstore, sender.clone(), created.clone())
                                .await
                        }
                        Err(err) => Err(err),
                    }
                }
            }
        };

        let handle = tokio::spawn(f);

        receiver
            .buffered(1024)
            .try_for_each(|_| async { Ok(()) })
            .await?;
        handle.await?
    }


    async fn do_unfold(
        ctx: &CoreContext,
        blobstore: &Arc<dyn Blobstore>,
        changes: PathTree<Option<PathChange>>,
        parents: HashSet<Manifest::Id>,
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

        let check_consistency = |manifests: &[Manifest]| {
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


        let change_type = match change {
            None => {
                if subentries.is_empty() {
                    // nothing changed in the current node and in the subentries
                    // if parent manifests are equal, we can reuse them
                    match parent_manifests.as_slice() {
                        [] => {
                            return Ok((
                                DeletedManifestChange {
                                    change_type: DeletedManifestChangeType::Reuse,
                                    copy_subentries_from: None,
                                },
                                vec![],
                            ));
                        }
                        [parent] => {
                            return Ok((
                                DeletedManifestChange {
                                    change_type: DeletedManifestChangeType::Reuse,
                                    copy_subentries_from: Some(parent.clone()),
                                },
                                vec![],
                            ));
                        }
                        parents => {
                            // parent manifests are different, we need to merge them
                            // let's check that the node status is consistent across parents
                            let is_deleted = check_consistency(parents)?;
                            if is_deleted {
                                DeletedManifestChangeType::CreateDeleted
                            } else {
                                DeletedManifestChangeType::RemoveIfNowEmpty
                            }
                        }
                    }
                } else {
                    // some paths might be added/deleted
                    DeletedManifestChangeType::RemoveIfNowEmpty
                }
            }
            Some(PathChange::Add) => {
                // the path was added
                DeletedManifestChangeType::RemoveIfNowEmpty
            }
            Some(PathChange::Remove) => {
                // the path was removed
                DeletedManifestChangeType::CreateDeleted
            }
            Some(PathChange::FileDirConflict) => {
                // This is a file/dir conflict: either a file was replaced by directory or other way
                // round. In both cases one of the paths is being deleted and recreated as other
                // type. To keep this in history, we need to mark the path as deleted in the deleted
                // files manifest.
                DeletedManifestChangeType::RemoveIfNowEmpty
            }
        };

        // Base traversal for all modified subentries
        let mut recurse_entries = subentries
            .into_iter()
            .map(|(path, change_tree)| {
                (
                    path.clone(),
                    DeletedManifestUnfoldNode {
                        path_element: Some(path),
                        changes: change_tree,
                        parents: HashSet::new(),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();

        let fold_node = match parent_manifests.as_slice() {
            [] => DeletedManifestChange {
                change_type,
                copy_subentries_from: None,
            },
            [parent] => {
                // If there's one parent, we can "copy" its subentries
                // and modify only a few fields. Important if we're doing few
                // changes on a big node and need to optimise.
                for (path, node) in &mut recurse_entries {
                    if let Some(subentry_id) = parent.lookup(ctx, blobstore, path).await? {
                        node.parents.insert(subentry_id);
                    }
                }

                DeletedManifestChange {
                    change_type,
                    copy_subentries_from: Some(parent.clone()),
                }
            }
            _ => {
                // If there are multiple parents and they're different, we need to
                // merge all different subentries. So let's just look at all of them.
                for parent in parent_manifests {
                    parent
                        .into_subentries(ctx, blobstore)
                        .try_for_each(|(path, mf_id)| {
                            let entry = recurse_entries.entry(path.clone()).or_insert_with(|| {
                                DeletedManifestUnfoldNode {
                                    path_element: Some(path),
                                    changes: Default::default(),
                                    parents: HashSet::new(),
                                }
                            });
                            entry.parents.insert(mf_id);
                            async { Ok(()) }
                        })
                        .await?;
                }
                DeletedManifestChange {
                    change_type,
                    copy_subentries_from: None,
                }
            }
        };

        Ok((
            fold_node,
            recurse_entries
                .into_iter()
                .map(|(_, node)| node)
                .collect::<Vec<_>>(),
        ))
    }

    async fn save_manifest(
        manifest: Manifest,
        ctx: &CoreContext,
        blobstore: &Arc<dyn Blobstore>,
        sender: mpsc::UnboundedSender<BoxFuture<'static, Result<(), Error>>>,
        created: Arc<Mutex<HashSet<String>>>,
    ) -> Result<Manifest::Id, Error> {
        let mf_id = manifest.id();

        let key = mf_id.blobstore_key();
        let mut created = created.lock().await;
        if created.insert(key.clone()) {
            let blob = manifest.into_blob();
            cloned!(ctx, blobstore);
            let f = async move { blobstore.put(&ctx, key, blob.into()).await }.boxed();

            sender
                .unbounded_send(f)
                .map_err(|err| anyhow!("failed to send manifest future {}", err))?;
        }
        Ok(mf_id)
    }

    async fn do_create(
        ctx: &CoreContext,
        blobstore: &Arc<dyn Blobstore>,
        cs_id: ChangesetId,
        change: DeletedManifestChange<Manifest>,
        subentries_to_update: BTreeMap<MPathElement, Option<Manifest::Id>>,
        sender: mpsc::UnboundedSender<BoxFuture<'static, Result<(), Error>>>,
        created: Arc<Mutex<HashSet<String>>>,
    ) -> Result<Option<Manifest::Id>, Error> {
        match change.change_type {
            DeletedManifestChangeType::Reuse => Ok(change.copy_subentries_from.map(|mf| mf.id())),
            DeletedManifestChangeType::CreateDeleted => Self::save_manifest(
                Manifest::copy_and_update_subentries(
                    ctx,
                    blobstore,
                    change.copy_subentries_from,
                    Some(cs_id),
                    subentries_to_update,
                )
                .await?,
                ctx,
                blobstore,
                sender,
                created,
            )
            .await
            .map(Some),
            DeletedManifestChangeType::RemoveIfNowEmpty => {
                let manifest = Manifest::copy_and_update_subentries(
                    ctx,
                    blobstore,
                    change.copy_subentries_from,
                    None,
                    subentries_to_update,
                )
                .await?;
                // some of the subentries were deleted, creating a new node but there is no need to
                // mark it as deleted
                if !manifest.is_empty() {
                    Self::save_manifest(manifest, ctx, blobstore, sender, created)
                        .await
                        .map(Some)
                } else {
                    Ok(None)
                }
            }
        }
    }
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

#[cfg(test)]
crate::test_utils::impl_deleted_manifest_tests!(DeletedManifest);
