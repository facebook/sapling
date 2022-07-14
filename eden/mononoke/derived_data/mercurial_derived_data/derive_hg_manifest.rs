/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::format_err;
use anyhow::Error;
use blobrepo_errors::ErrorKind;
use blobstore::Blobstore;
use blobstore::Loadable;
use cloned::cloned;
use context::CoreContext;
use futures::channel::mpsc;
use futures::compat::Future01CompatExt;
use futures::future;
use futures::future::try_join_all;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use futures::future::TryFutureExt;
use manifest::derive_manifest_with_io_sender;
use manifest::derive_manifests_for_simple_stack_of_commits;
use manifest::Entry;
use manifest::LeafInfo;
use manifest::ManifestChanges;
use manifest::Traced;
use manifest::TreeInfo;
use mercurial_types::blobs::ContentBlobMeta;
use mercurial_types::blobs::UploadHgFileContents;
use mercurial_types::blobs::UploadHgFileEntry;
use mercurial_types::blobs::UploadHgNodeHash;
use mercurial_types::blobs::UploadHgTreeEntry;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mononoke_types::ChangesetId;
use mononoke_types::FileType;
use mononoke_types::MPath;
use mononoke_types::RepoPath;
use mononoke_types::TrackedFileChange;
use sorted_vector_map::SortedVectorMap;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;

use crate::derive_hg_changeset::store_file_change;

#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
struct ParentIndex(usize);

pub async fn derive_simple_hg_manifest_stack_without_copy_info(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    manifest_changes: Vec<ManifestChanges<TrackedFileChange>>,
    parent: Option<HgManifestId>,
) -> Result<HashMap<ChangesetId, HgManifestId>, Error> {
    let res = derive_manifests_for_simple_stack_of_commits(
        ctx.clone(),
        blobstore.clone(),
        parent.map(|p| Traced::assign(ParentIndex(0), p)),
        manifest_changes,
        {
            cloned!(blobstore, ctx);
            move |mut tree_info, _cs_id| {
                cloned!(blobstore, ctx);
                async move {
                    tree_info.parents = tree_info
                        .parents
                        .into_iter()
                        .map(|p| Traced::assign(ParentIndex(0), p.into_untraced()))
                        .collect();
                    create_hg_manifest(ctx.clone(), blobstore.clone(), None, tree_info).await
                }
            }
        },
        {
            cloned!(blobstore, ctx);
            move |leaf_info, _cs_id| {
                cloned!(blobstore, ctx);
                async move {
                    let LeafInfo {
                        leaf,
                        path,
                        parents,
                    } = leaf_info;

                    let parents: Vec<_> = parents
                        .into_iter()
                        .map(|p| Traced::assign(ParentIndex(0), p.into_untraced()))
                        .collect();
                    match leaf {
                        Some(leaf) => {
                            if leaf.copy_from().is_some() {
                                return Err(
                                    format_err!(
                                        "unsupported generation of stack of hg manifests: leaf {} has copy info {:?}",
                                        path,
                                        leaf.copy_from(),
                                    )
                                );
                            }
                            store_file_change(
                                ctx,
                                blobstore,
                                parents.get(0).map(|p| p.untraced().1),
                                None,
                                &path,
                                &leaf,
                                None, // copy_from should be empty
                            )
                            .map_ok(|res| ((), Traced::generate(res)))
                            .await
                        }
                        None => {
                            let (file_type, filenode) =
                                resolve_conflict(ctx, blobstore, path, &parents).await?;
                            Ok(((), Traced::generate((file_type, filenode))))
                        }
                    }
                }
            }
        },
    )
    .await?;

    Ok(res
        .into_iter()
        .map(|(key, value)| (key, value.into_untraced()))
        .collect())
}

/// Derive mercurial manifest from parent manifests and bonsai file changes.
pub async fn derive_hg_manifest(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    parents: impl IntoIterator<Item = HgManifestId>,
    changes: impl IntoIterator<Item = (MPath, Option<(FileType, HgFileNodeId)>)> + 'static,
) -> Result<HgManifestId, Error> {
    let parents: Vec<_> = parents
        .into_iter()
        .enumerate()
        .map(|(i, m)| Traced::assign(ParentIndex(i), m))
        .collect();

    let tree_id = derive_manifest_with_io_sender(
        ctx.clone(),
        blobstore.clone(),
        parents.clone(),
        changes,
        {
            cloned!(ctx, blobstore);
            move |tree_info, sender| {
                create_hg_manifest(ctx.clone(), blobstore.clone(), Some(sender), tree_info)
            }
        },
        {
            cloned!(ctx, blobstore);
            move |leaf_info, _sender| create_hg_file(ctx.clone(), blobstore.clone(), leaf_info)
        },
    )
    .await?;

    match tree_id {
        Some(traced_tree_id) => Ok(traced_tree_id.into_untraced()),
        None => {
            // All files have been deleted, generate empty **root** manifest
            let tree_info = TreeInfo {
                path: None,
                parents,
                subentries: Default::default(),
            };
            let (_, traced_tree_id) = create_hg_manifest(ctx, blobstore, None, tree_info).await?;
            Ok(traced_tree_id.into_untraced())
        }
    }
}

/// This function is used as callback from `derive_manifest` to generate and store manifest
/// object from `TreeInfo`.
async fn create_hg_manifest(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    sender: Option<mpsc::UnboundedSender<BoxFuture<'static, Result<(), Error>>>>,
    tree_info: TreeInfo<
        Traced<ParentIndex, HgManifestId>,
        Traced<ParentIndex, (FileType, HgFileNodeId)>,
        (),
    >,
) -> Result<((), Traced<ParentIndex, HgManifestId>), Error> {
    let TreeInfo {
        subentries,
        path,
        parents,
    } = tree_info;

    // See if any of the parents have the same hg manifest. If yes, then we can just reuse it
    // without recreating manifest again.
    // But note that we reuse only if manifest has more than on parent, and there are a few reasons for
    // it:
    // 1) If a commit have a single parent then create_hg_manifest function shouldn't normally be called -
    //    it can only happen if a file hasn't changed, but nevertheless there's an entry for this file
    //    in the bonsai. This should happen rarely, and recreating manifest in these cases shouldn't be
    //    a problem.
    // 2) It adds an additional read of parent manifests, and it can potentially be expensive if manifests
    //    are large.
    //    We'd rather not do it if we don't need to, and it seems that we don't really need to (see point 1)
    if parents.len() > 1 {
        let mut subentries_vec_map = BTreeMap::new();
        for (name, (_context, subentry)) in &subentries {
            let subentry = match subentry {
                Entry::Tree(manifest_id) => Entry::Tree(*manifest_id.untraced()),
                Entry::Leaf(leaf) => Entry::Leaf(*leaf.untraced()),
            };
            subentries_vec_map.insert(name.clone(), subentry);
        }

        let subentries_vec_map = SortedVectorMap::from(subentries_vec_map);

        let (p1_parent, p2_parent) = hg_parents(&parents);
        let loaded_parents = {
            let ctx = &ctx;
            let blobstore = &blobstore;

            future::try_join_all(p1_parent.into_iter().chain(p2_parent).map(|id| async move {
                let mf = id.load(ctx, blobstore).map_err(Error::from).await?;
                Result::<_, Error>::Ok((id, mf))
            }))
            .await?
        };

        if let Some((reuse_id, _)) = loaded_parents
            .into_iter()
            .find(|(_, p)| p.content().files == subentries_vec_map)
        {
            return Ok(((), Traced::generate(reuse_id)));
        }
    }

    let mut contents = Vec::new();
    for (name, (_context, subentry)) in subentries {
        contents.extend(name.as_ref());
        let subentry: Entry<_, _> = subentry.into();
        let (tag, hash) = match subentry {
            Entry::Tree(manifest_id) => ("t", manifest_id.into_nodehash()),
            Entry::Leaf((file_type, filenode_id)) => {
                let tag = match file_type {
                    FileType::Symlink => "l",
                    FileType::Executable => "x",
                    FileType::Regular => "",
                };
                (tag, filenode_id.into_nodehash())
            }
        };
        write!(&mut contents, "\0{}{}\n", hash, tag).expect("write to memory failed");
    }

    let path = match path {
        None => RepoPath::RootPath,
        Some(path) => RepoPath::DirectoryPath(path),
    };

    let (p1, p2) = hg_parents(&parents);

    let p1 = p1.map(|id| id.into_nodehash());
    let p2 = p2.map(|id| id.into_nodehash());

    let uploader = UploadHgTreeEntry {
        upload_node_id: UploadHgNodeHash::Generate,
        contents: contents.into(),
        p1,
        p2,
        path,
    }
    .upload(ctx, blobstore);

    let (mfid, upload_fut) = match uploader {
        Ok((mfid, fut)) => (mfid, fut.compat().map_ok(|_| ())),
        Err(e) => return Err(e),
    };

    match sender {
        Some(sender) => {
            sender
                .unbounded_send(upload_fut.boxed())
                .map_err(|err| format_err!("failed to send hg manifest future {}", err))?;
        }
        None => upload_fut.await?,
    }
    Ok(((), Traced::generate(mfid)))
}

/// This function is used as callback from `derive_manifest` to generate and store file entry
/// object from `LeafInfo`.
async fn create_hg_file(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    leaf_info: LeafInfo<Traced<ParentIndex, (FileType, HgFileNodeId)>, (FileType, HgFileNodeId)>,
) -> Result<((), Traced<ParentIndex, (FileType, HgFileNodeId)>), Error> {
    let LeafInfo {
        leaf,
        path,
        parents,
    } = leaf_info;

    // TODO: move `Blobrepo::store_file_changes` logic in here
    match leaf {
        Some(leaf) => Ok(((), Traced::generate(leaf))),
        None => {
            // Leaf was not provided, try to resolve same-content different parents leaf. Since filenode
            // hashes include ancestry, this can be necessary if two identical files were created through
            // different paths in history.
            let (file_type, filenode) = resolve_conflict(ctx, blobstore, path, &parents).await?;
            Ok(((), Traced::generate((file_type, filenode))))
        }
    }
}

async fn resolve_conflict(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    path: MPath,
    parents: &[Traced<ParentIndex, (FileType, HgFileNodeId)>],
) -> Result<(FileType, HgFileNodeId), Error> {
    let make_err = || {
        ErrorKind::UnresolvedConflicts(
            path.clone(),
            parents.iter().map(|p| *p.untraced()).collect::<Vec<_>>(),
        )
    };

    // First, if the file type is different across entries, we need to bail. This is a conflict.
    let file_type =
        unique_or_nothing(parents.iter().map(|p| p.untraced().0)).ok_or_else(make_err)?;

    // Assuming the file type is the same, then let's check that the contents are identical. To do
    // so, we'll load the envelopes.
    let envelopes = parents
        .iter()
        .map(|p| p.untraced().1.load(&ctx, &blobstore));

    let envelopes = try_join_all(envelopes).await?;

    let (content_id, content_size) =
        unique_or_nothing(envelopes.iter().map(|e| (e.content_id(), e.content_size())))
            .ok_or_else(make_err)?;

    // If we got here, then that means the file type and content is the same everywhere. In this
    // case, let's reuse a filenode.
    let (maybe_reuse_filenode, _) = hg_parents(parents);
    match maybe_reuse_filenode {
        Some((_ft, id)) => Ok((file_type, id)),
        None => {
            // This can only happen in the case of an octopus merge where neither p1 nor p2
            // contained this content. It would be nice if we could reuse p3 or later,
            // but Mercurial could be confused by a filenode whose linknode is not a Mercurial
            // ancestor of the commit. So don't risk it.
            let contents = ContentBlobMeta {
                id: content_id,
                size: content_size,
                copy_from: None,
            };
            let (filenode_id, _) = UploadHgFileEntry {
                upload_node_id: UploadHgNodeHash::Generate,
                contents: UploadHgFileContents::ContentUploaded(contents),
                p1: None,
                p2: None,
            }
            .upload_with_path(ctx, blobstore, path)
            .await?;
            Ok((file_type, filenode_id))
        }
    }
}

/// Extract hg-relevant parents from a set of Traced entries. This means we ignore any parents
/// except for p1 and p2.
fn hg_parents<T: Copy>(parents: &[Traced<ParentIndex, T>]) -> (Option<T>, Option<T>) {
    let mut parents = parents.iter().filter_map(|t| match t.id() {
        Some(ParentIndex(0)) | Some(ParentIndex(1)) => Some(t.untraced()),
        Some(_) | None => None,
    });

    (parents.next().copied(), parents.next().copied())
}

/// Take an iterator, if it has just one value, return it. Otherwise, return None.
fn unique_or_nothing<T: PartialEq>(iter: impl Iterator<Item = T>) -> Option<T> {
    let mut ret = None;

    for e in iter {
        if ret.is_none() {
            ret = Some(e);
            continue;
        }

        if ret.as_ref().expect("We just checked") == &e {
            continue;
        }

        return None;
    }

    ret
}
