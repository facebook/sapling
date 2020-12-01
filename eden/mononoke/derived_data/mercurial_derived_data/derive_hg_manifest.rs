/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Error};
use blobrepo_errors::ErrorKind;
use blobstore::{Blobstore, Loadable};
use cloned::cloned;
use context::CoreContext;
use futures::future::try_join_all;
use futures::{
    channel::mpsc,
    compat::Future01CompatExt,
    future::{BoxFuture, FutureExt, TryFutureExt},
};
use manifest::{derive_manifest_with_io_sender, Entry, LeafInfo, Traced, TreeInfo};
use mercurial_types::{
    blobs::{
        ContentBlobMeta, UploadHgFileContents, UploadHgFileEntry, UploadHgNodeHash,
        UploadHgTreeEntry,
    },
    HgFileNodeId, HgManifestId,
};
use mononoke_types::{FileType, MPath, RepoPath};
use std::{io::Write, sync::Arc};

#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
struct ParentIndex(usize);

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
    // case, let's produce a new filenode, and upload the entry. In doing so, exclude any parents
    // beyond p1 and p2.
    let (p1, p2) = hg_parents(&parents);
    let p1 = p1.map(|(_ft, id)| id);
    let p2 = p2.map(|(_ft, id)| id);

    let contents = ContentBlobMeta {
        id: content_id,
        size: content_size,
        copy_from: None,
    };

    let (filenode_id, _) = UploadHgFileEntry {
        upload_node_id: UploadHgNodeHash::Generate,
        contents: UploadHgFileContents::ContentUploaded(contents),
        p1,
        p2,
        path,
    }
    .upload(ctx, blobstore)?
    .1
    .compat()
    .await?;

    Ok((file_type, filenode_id))
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
