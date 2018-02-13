// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Plain files, symlinks
use std::sync::Arc;

use futures::future::Future;
use futures_ext::{BoxFuture, FutureExt};

use mercurial::file;
use mercurial_types::{Blob, BlobNode, MPath, MPathElement, ManifestId, NodeHash, Parents};
use mercurial_types::manifest::{Content, Entry, Manifest, Type};
use mercurial_types::nodehash::EntryId;

use blobstore::Blobstore;

use errors::*;

use manifest::BlobManifest;

use utils::{get_node, RawNodeBlob};

pub struct BlobEntry {
    blobstore: Arc<Blobstore>,
    name: Option<MPathElement>,
    id: EntryId,
    ty: Type,
}

pub fn fetch_file_content_and_renames_from_blobstore(
    blobstore: &Arc<Blobstore>,
    nodeid: NodeHash,
) -> BoxFuture<(Vec<u8>, Option<(MPath, NodeHash)>), Error> {
    get_node(blobstore, nodeid)
        .and_then({
            let blobstore = blobstore.clone();
            move |node| {
                let key = format!("sha1-{}", node.blob.sha1());
                let parents = node.parents;

                blobstore.get(key).and_then(move |blob| {
                    blob.ok_or(ErrorKind::ContentMissing(nodeid, node.blob).into())
                        .and_then(|blob| {
                            let blob = blob.as_ref();
                            let (p1, p2) = parents.get_nodes();
                            let blobnode = BlobNode::new(blob, p1, p2);
                            let file = file::File::new(blobnode);

                            file.copied_from().and_then(|from| {
                                file.content()
                                    .ok_or(ErrorKind::ContentMissing(nodeid, node.blob).into())
                                    .map(|content| (Vec::from(content), from))
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
        nodeid: NodeHash,
        ty: Type,
    ) -> Result<Self> {
        Ok(Self {
            blobstore,
            name,
            id: EntryId::new(nodeid),
            ty,
        })
    }

    pub fn new_root(blobstore: Arc<Blobstore>, manifestid: ManifestId) -> Self {
        Self {
            blobstore,
            name: None,
            id: EntryId::new(manifestid.into_nodehash()),
            ty: Type::Tree,
        }
    }

    fn get_node(&self) -> BoxFuture<RawNodeBlob, Error> {
        get_node(&self.blobstore, self.id.into_nodehash())
    }

    fn get_raw_content_inner(&self) -> BoxFuture<Vec<u8>, Error> {
        let nodeid = self.id.into_nodehash();
        let blobstore = self.blobstore.clone();

        self.get_node()
            .and_then({
                let blobstore = blobstore.clone();
                move |node| {
                    let key = format!("sha1-{}", node.blob.sha1());

                    blobstore
                        .get(key)
                        .and_then(move |blob| {
                            blob.ok_or(ErrorKind::ContentMissing(nodeid, node.blob).into())
                        })
                        .map(|blob| Vec::from(blob.as_ref()))
                }
            })
            .boxify()
    }
}

impl Entry for BlobEntry {
    fn get_type(&self) -> Type {
        self.ty
    }

    fn get_parents(&self) -> BoxFuture<Parents, Error> {
        self.get_node().map(|node| node.parents).boxify()
    }

    fn get_raw_content(&self) -> BoxFuture<Blob<Vec<u8>>, Error> {
        self.get_raw_content_inner()
            .map(|blob| Blob::from(blob.as_ref()))
            .boxify()
    }

    fn get_content(&self) -> BoxFuture<Content, Error> {
        let blobstore = self.blobstore.clone();
        self.get_raw_content_inner()
            .and_then({
                let ty = self.ty;
                move |blob| {
                    let blob = blob.as_ref();

                    // Mercurial file blob can have metadata, but tree manifest can't
                    let blob = if ty == Type::Tree {
                        blob
                    } else {
                        let (_, off) = file::File::extract_meta(blob);
                        &blob[off..]
                    };
                    let res = match ty {
                        Type::File => Content::File(Blob::from(blob)),
                        Type::Executable => Content::Executable(Blob::from(blob)),
                        Type::Symlink => Content::Symlink(MPath::new(blob)?),
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
                Content::File(data) | Content::Executable(data) => Ok(data.size()),
                Content::Symlink(path) => Ok(Some(path.to_vec().len())),
                Content::Tree(_) => Ok(None),
            })
            .boxify()
    }

    fn get_hash(&self) -> &EntryId {
        &self.id
    }

    fn get_name(&self) -> &Option<MPathElement> {
        &self.name
    }
}
