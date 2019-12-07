/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::errors::ErrorKind;
use crate::utils::{IncompleteFilenodeInfo, IncompleteFilenodes};
use anyhow::{format_err, Error};
use blobstore::Blobstore;
use cloned::cloned;
use context::CoreContext;
use futures::{future, Future, IntoFuture};
use futures_ext::FutureExt;
use manifest::{derive_manifest, Entry, LeafInfo, TreeInfo};
use mercurial_types::{
    blobs::{
        fetch_file_envelope, ContentBlobMeta, HgBlobEntry, UploadHgFileContents, UploadHgFileEntry,
        UploadHgNodeHash, UploadHgTreeEntry,
    },
    HgEntry, HgEntryId, HgFileNodeId, HgManifestId,
};
use mononoke_types::{FileType, MPath, RepoPath};
use std::{io::Write, sync::Arc};

/// Derive mercurial manifest from parent manifests and bonsai file changes.
pub fn derive_hg_manifest(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    incomplete_filenodes: IncompleteFilenodes, // TODO: construct by diffing manifests
    parents: impl IntoIterator<Item = HgManifestId>,
    changes: impl IntoIterator<Item = (MPath, Option<HgBlobEntry>)>,
) -> impl Future<Item = HgManifestId, Error = Error> {
    let parents: Vec<_> = parents.into_iter().collect();
    derive_manifest(
        ctx.clone(),
        blobstore.clone(),
        parents.clone(),
        changes,
        {
            cloned!(ctx, blobstore, incomplete_filenodes);
            move |tree_info| {
                create_hg_manifest(
                    ctx.clone(),
                    blobstore.clone(),
                    incomplete_filenodes.clone(),
                    tree_info,
                )
            }
        },
        {
            cloned!(ctx, blobstore, incomplete_filenodes);
            move |leaf_info| {
                create_hg_file(
                    ctx.clone(),
                    blobstore.clone(),
                    incomplete_filenodes.clone(),
                    leaf_info,
                )
            }
        },
    )
    .and_then(move |tree_id| match tree_id {
        Some(tree_id) => future::ok(tree_id).left_future(),
        None => {
            // All files have been deleted, generate empty **root** manifest
            let tree_info = TreeInfo {
                path: None,
                parents,
                subentries: Default::default(),
            };
            create_hg_manifest(ctx, blobstore, incomplete_filenodes, tree_info)
                .map(|(_, tree_id)| tree_id)
                .right_future()
        }
    })
}

fn hg_parents<T: Copy>(parents: &Vec<T>) -> Result<(Option<T>, Option<T>), Error> {
    let mut parents = parents.iter();
    let p1 = parents.next().copied();
    let p2 = parents.next().copied();
    if parents.next().is_some() {
        Err(ErrorKind::TooManyParents.into())
    } else {
        Ok((p1, p2))
    }
}

/// This function is used as callback from `derive_manifest` to generate and store manifest
/// object from `TreeInfo`.
fn create_hg_manifest(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    incomplete_filenodes: IncompleteFilenodes,
    tree_info: TreeInfo<HgManifestId, (FileType, HgFileNodeId), ()>,
) -> impl Future<Item = ((), HgManifestId), Error = Error> {
    let TreeInfo {
        subentries,
        path,
        parents,
    } = tree_info;

    let mut contents = Vec::new();
    for (name, (_context, subentry)) in subentries {
        contents.extend(name.as_ref());
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

    hg_parents(&parents)
        .into_future()
        .and_then(move |(p1, p2)| {
            let p1 = p1.map(|id| id.into_nodehash());
            let p2 = p2.map(|id| id.into_nodehash());
            UploadHgTreeEntry {
                upload_node_id: UploadHgNodeHash::Generate,
                contents: contents.into(),
                p1,
                p2,
                path: path.clone(),
            }
            .upload(ctx.clone(), blobstore)
            .map(|(hash, future)| future.map(move |_| hash))
            .into_future()
            .flatten()
            .map({
                cloned!(incomplete_filenodes);
                move |hash| {
                    incomplete_filenodes.add(IncompleteFilenodeInfo {
                        path,
                        filenode: HgFileNodeId::new(hash),
                        p1: p1.map(HgFileNodeId::new),
                        p2: p2.map(HgFileNodeId::new),
                        copyfrom: None,
                    });
                    ((), HgManifestId::new(hash))
                }
            })
        })
}

/// This function is used as callback from `derive_manifest` to generate and store file entry
/// object from `LeafInfo`.
fn create_hg_file(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    incomplete_filenodes: IncompleteFilenodes,
    leaf_info: LeafInfo<(FileType, HgFileNodeId), HgBlobEntry>,
) -> impl Future<Item = ((), (FileType, HgFileNodeId)), Error = Error> {
    let LeafInfo {
        leaf,
        path,
        parents,
    } = leaf_info;

    // TODO: move `Blobrepo::store_file_changes` logic in here
    if let Some(leaf) = leaf {
        return match leaf.get_hash() {
            HgEntryId::Manifest(_) => {
                future::err(Error::msg("changes can not contain tree entry")).left_future()
            }
            HgEntryId::File(file_type, filenode_id) => {
                future::ok(((), (file_type, filenode_id))).left_future()
            }
        };
    }

    // Leaf was not provided, try to resolve same-content different parents leaf
    hg_parents(&parents)
        .into_future()
        .and_then(move |(p1, p2)| match (p1, p2) {
            (Some((ft1, p1)), Some((ft2, p2))) if ft1 == ft2 => (
                fetch_file_envelope(ctx.clone(), &blobstore, p1),
                fetch_file_envelope(ctx.clone(), &blobstore, p2),
            )
                .into_future()
                .and_then(move |(e1, e2)| {
                    if e1.content_id() == e2.content_id() {
                        let contents = ContentBlobMeta {
                            id: e1.content_id(),
                            size: e1.content_size(),
                            copy_from: None,
                        };
                        UploadHgFileEntry {
                            upload_node_id: UploadHgNodeHash::Generate,
                            contents: UploadHgFileContents::ContentUploaded(contents),
                            file_type: ft1,
                            p1: Some(p1),
                            p2: Some(p2),
                            path,
                        }
                        .upload(ctx, blobstore)
                        .map(|(_, future)| future)
                        .into_future()
                        .flatten()
                        .map(move |(hg_entry, path)| {
                            let (file_type, filenode) = hg_entry
                                .get_hash()
                                .to_filenode()
                                .expect("UploadHgFileEntry returned manifest entry");
                            incomplete_filenodes.add(IncompleteFilenodeInfo {
                                path,
                                filenode,
                                p1: Some(p1),
                                p2: Some(p2),
                                copyfrom: None,
                            });
                            (file_type, filenode)
                        })
                        .right_future()
                    } else {
                        let error = format_err!(
                            "Unresloved conflict at:\npath: {:?}\nparents: {:?}",
                            path,
                            parents,
                        );
                        future::err(error).left_future()
                    }
                })
                .right_future(),
            _ => {
                let error = format_err!(
                    "Unresloved conflict at:\npath: {:?}\nparents: {:?}",
                    path,
                    parents,
                );
                future::err(error).left_future()
            }
        })
        .map(|tree_id| ((), tree_id))
        .right_future()
}
