// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::errors::ErrorKind;
use crate::file::{fetch_file_envelope, HgBlobEntry};
use crate::manifest::ManifestContent;
use crate::repo::{
    BlobRepo, ContentBlobMeta, UploadHgFileContents, UploadHgFileEntry, UploadHgNodeHash,
    UploadHgTreeEntry,
};
use crate::utils::{IncompleteFilenodeInfo, IncompleteFilenodes};
use blobstore::{Blobstore, Loadable};
use cloned::cloned;
use context::CoreContext;
use failure::{err_msg, format_err, Error};
use futures::{future, Future, IntoFuture};
use futures_ext::{BoxFuture, FutureExt};
use manifest::{derive_manifest, Entry, LeafInfo, Manifest, TreeInfo};
use mercurial_types::{
    Entry as HgEntry, HgEntryId, HgFileNodeId, HgManifestEnvelope, HgManifestId,
};
use mononoke_types::{FileType, MPath, MPathElement, RepoPath};
use repo_blobstore::RepoBlobstore;
use std::io::Write;

#[derive(Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub struct Id<T>(T);

impl Loadable for Id<HgManifestId> {
    type Value = ManifestContent;

    fn load(
        &self,
        ctx: CoreContext,
        blobstore: impl Blobstore + Clone,
    ) -> BoxFuture<Self::Value, Error> {
        let manifest_id = self.0;
        blobstore
            .get(ctx, manifest_id.blobstore_key())
            .and_then(move |bytes| match bytes {
                None => Err(ErrorKind::ManifestMissing(manifest_id).into()),
                Some(bytes) => {
                    let envelope = HgManifestEnvelope::from_blob(bytes.into())?;
                    ManifestContent::parse(envelope.contents().as_ref())
                }
            })
            .boxify()
    }
}

impl Manifest for ManifestContent {
    type TreeId = Id<HgManifestId>;
    type LeafId = (FileType, HgFileNodeId);

    fn lookup(&self, name: &MPathElement) -> Option<Entry<Self::TreeId, Self::LeafId>> {
        self.files.get(name).map(hg_into_entry)
    }

    fn list(&self) -> Box<dyn Iterator<Item = (MPathElement, Entry<Self::TreeId, Self::LeafId>)>> {
        let iter = self
            .files
            .clone()
            .into_iter()
            .map(|(name, hg_entry_id)| (name, hg_into_entry(&hg_entry_id)));
        Box::new(iter)
    }
}

fn hg_into_entry(hg_entry_id: &HgEntryId) -> Entry<Id<HgManifestId>, (FileType, HgFileNodeId)> {
    match hg_entry_id {
        HgEntryId::File(file_type, filenode_id) => Entry::Leaf((*file_type, *filenode_id)),
        HgEntryId::Manifest(manifest_id) => Entry::Tree(Id(*manifest_id)),
    }
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

/// Derive mercurial manifest from parent manifests and bonsai file changes.
pub fn derive_hg_manifest(
    ctx: CoreContext,
    repo: BlobRepo, // TODO: replace with Blobstore, requires changing UploadHgFileEntry
    incomplete_filenodes: IncompleteFilenodes, // TODO: construct by diffing manifests
    parents: impl IntoIterator<Item = HgManifestId>,
    changes: impl IntoIterator<Item = (MPath, Option<HgBlobEntry>)>,
) -> impl Future<Item = HgManifestId, Error = Error> {
    let parents: Vec<_> = parents.into_iter().map(Id).collect();
    derive_manifest(
        ctx.clone(),
        repo.get_blobstore(),
        parents.clone(),
        changes,
        {
            cloned!(ctx, repo, incomplete_filenodes);
            move |tree_info| {
                create_hg_manifest(
                    ctx.clone(),
                    repo.get_blobstore(),
                    incomplete_filenodes.clone(),
                    tree_info,
                )
            }
        },
        {
            cloned!(ctx, repo, incomplete_filenodes);
            move |leaf_info| {
                create_hg_file(
                    ctx.clone(),
                    repo.clone(),
                    incomplete_filenodes.clone(),
                    leaf_info,
                )
            }
        },
    )
    .and_then(move |tree_id| match tree_id {
        Some(tree_id) => future::ok(tree_id.0).left_future(),
        None => {
            // All files have been deleted, generate empty **root** manifest
            let tree_info = TreeInfo {
                path: None,
                parents,
                subentries: Default::default(),
            };
            create_hg_manifest(ctx, repo.get_blobstore(), incomplete_filenodes, tree_info)
                .map(|id| id.0)
                .right_future()
        }
    })
}

/// This function is used as callback from `derive_manifest` to generate and store manifest
/// object from `TreeInfo`.
fn create_hg_manifest(
    ctx: CoreContext,
    blobstore: RepoBlobstore,
    incomplete_filenodes: IncompleteFilenodes,
    tree_info: TreeInfo<Id<HgManifestId>, (FileType, HgFileNodeId)>,
) -> impl Future<Item = Id<HgManifestId>, Error = Error> {
    let TreeInfo {
        subentries,
        path,
        parents,
    } = tree_info;

    let mut contents = Vec::new();
    for (name, subentry) in subentries {
        contents.extend(name.as_ref());
        let (tag, hash) = match subentry {
            Entry::Tree(manifest_id) => ("t", manifest_id.0.into_nodehash()),
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
            let p1 = p1.map(|id| id.0.into_nodehash());
            let p2 = p2.map(|id| id.0.into_nodehash());
            UploadHgTreeEntry {
                upload_node_id: UploadHgNodeHash::Generate,
                contents: contents.into(),
                p1,
                p2,
                path: path.clone(),
            }
            .upload_to_blobstore(ctx.clone(), &blobstore, ctx.logger())
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
                    Id(HgManifestId::new(hash))
                }
            })
        })
}

/// This function is used as callback from `derive_manifest` to generate and store file entry
/// object from `LeafInfo`.
fn create_hg_file(
    ctx: CoreContext,
    repo: BlobRepo,
    incomplete_filenodes: IncompleteFilenodes,
    leaf_info: LeafInfo<(FileType, HgFileNodeId), HgBlobEntry>,
) -> impl Future<Item = (FileType, HgFileNodeId), Error = Error> {
    let LeafInfo {
        leaf,
        path,
        parents,
    } = leaf_info;

    // TODO: move `Blobrepo::store_file_changes` logic in here
    if let Some(leaf) = leaf {
        return match leaf.get_hash() {
            HgEntryId::Manifest(_) => {
                future::err(err_msg("changes can not contain tree entry")).left_future()
            }
            HgEntryId::File(file_type, filenode_id) => {
                future::ok((file_type, filenode_id)).left_future()
            }
        };
    }

    // Leaf was not provided, try to resolve same-content different parents leaf
    let blobstore = repo.get_blobstore();
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
                        .upload(ctx, &repo)
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
        .right_future()
}
