// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Plain files, symlinks
use std::sync::Arc;

use bytes::Bytes;

use futures::future::Future;
use futures_ext::{BoxFuture, FutureExt};

use mercurial::file;
use mercurial_types::{BlobNode, DManifestId, DNodeHash, DParents, FileType, HgBlob, MPath,
                      MPathElement};
use mercurial_types::manifest::{Content, Entry, Manifest, Type};
use mercurial_types::nodehash::DEntryId;

use blobstore::Blobstore;

use errors::*;

use manifest::BlobManifest;

use utils::{get_node, RawNodeBlob};

#[derive(Clone)]
pub struct BlobEntry {
    blobstore: Arc<Blobstore>,
    name: Option<MPathElement>,
    id: DEntryId,
    ty: Type,
}

pub fn fetch_file_content_and_renames_from_blobstore(
    blobstore: &Arc<Blobstore>,
    nodeid: DNodeHash,
) -> BoxFuture<(Bytes, Option<(MPath, DNodeHash)>), Error> {
    get_node(blobstore, nodeid)
        .and_then({
            let blobstore = blobstore.clone();
            move |node| {
                let key = format!("sha1-{}", node.blob.sha1());
                let parents = node.parents;

                blobstore.get(key).and_then(move |blob| {
                    blob.ok_or(ErrorKind::ContentMissing(nodeid, node.blob).into())
                        .and_then(|blob| {
                            // XXX this is broken -- parents.get_nodes() will never return
                            // (None, Some(hash)), which is what BlobNode relies on to figure out
                            // whether a node is copied.
                            let (p1, p2) = parents.get_nodes();
                            let blobnode = BlobNode::new(blob, p1, p2);
                            let file = file::File::new(blobnode);

                            file.copied_from().and_then(|from| {
                                file.content()
                                    .ok_or(ErrorKind::ContentMissing(nodeid, node.blob).into())
                                    .map(|content| (Bytes::from(content), from))
                            })
                        })
                })
            }
        })
        .boxify()
}

impl BlobEntry {
    pub fn new(
        blobstore: Arc<Blobstore>,
        name: Option<MPathElement>,
        nodeid: DNodeHash,
        ty: Type,
    ) -> Result<Self> {
        Ok(Self {
            blobstore,
            name,
            id: DEntryId::new(nodeid),
            ty,
        })
    }

    pub fn new_root(blobstore: Arc<Blobstore>, manifestid: DManifestId) -> Self {
        Self {
            blobstore,
            name: None,
            id: DEntryId::new(manifestid.into_nodehash()),
            ty: Type::Tree,
        }
    }

    fn get_node(&self) -> BoxFuture<RawNodeBlob, Error> {
        get_node(&self.blobstore, self.id.into_nodehash())
    }

    fn get_raw_content_inner(&self) -> BoxFuture<Bytes, Error> {
        let nodeid = self.id.into_nodehash();
        let blobstore = self.blobstore.clone();

        self.get_node()
            .and_then({
                let blobstore = blobstore.clone();
                move |node| {
                    let key = format!("sha1-{}", node.blob.sha1());

                    blobstore.get(key).and_then(move |blob| {
                        blob.ok_or(ErrorKind::ContentMissing(nodeid, node.blob).into())
                    })
                }
            })
            .boxify()
    }
}

impl Entry for BlobEntry {
    fn get_type(&self) -> Type {
        self.ty
    }

    fn get_parents(&self) -> BoxFuture<DParents, Error> {
        self.get_node().map(|node| node.parents).boxify()
    }

    fn get_raw_content(&self) -> BoxFuture<HgBlob, Error> {
        self.get_raw_content_inner()
            .map(|bytes| HgBlob::from(bytes))
            .boxify()
    }

    fn get_content(&self) -> BoxFuture<Content, Error> {
        let blobstore = self.blobstore.clone();
        self.get_raw_content_inner()
            .and_then({
                let ty = self.ty;
                move |bytes| {
                    // Mercurial file blob can have metadata, but tree manifest can't
                    let blob = if ty == Type::Tree {
                        bytes
                    } else {
                        let (_, off) = file::File::extract_meta(&bytes);
                        bytes.slice_from(off)
                    };
                    let res = match ty {
                        Type::File(FileType::Regular) => Content::File(HgBlob::from(blob)),
                        Type::File(FileType::Executable) => Content::Executable(HgBlob::from(blob)),
                        Type::File(FileType::Symlink) => Content::Symlink(HgBlob::from(blob)),
                        Type::Tree => Content::Tree(BlobManifest::parse(blobstore, blob)?.boxed()),
                    };

                    Ok(res)
                }
            })
            .boxify()
    }

    fn get_size(&self) -> BoxFuture<Option<usize>, Error> {
        self.get_content()
            .and_then(|content| match content {
                Content::File(data) | Content::Executable(data) | Content::Symlink(data) => {
                    Ok(data.size())
                }
                Content::Tree(_) => Ok(None),
            })
            .boxify()
    }

    fn get_hash(&self) -> &DEntryId {
        &self.id
    }

    fn get_name(&self) -> Option<&MPathElement> {
        self.name.as_ref()
    }
}
