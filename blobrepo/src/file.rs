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
use mercurial_types::{Blob, MPath, NodeHash, Parents, RepoPath};
use mercurial_types::manifest::{Content, Entry, Manifest, Type};
use mercurial_types::nodehash::EntryId;

use blobstore::Blobstore;

use errors::*;

use manifest::BlobManifest;

use utils::{get_node, RawNodeBlob};

pub struct BlobEntry<B> {
    blobstore: B,
    path: RepoPath,
    id: EntryId,
    ty: Type,
}

pub fn fetch_blob_from_blobstore(
    blobstore: &Arc<Blobstore>,
    nodeid: NodeHash,
) -> BoxFuture<Vec<u8>, Error> {
    get_node(blobstore, nodeid)
        .and_then({
            let blobstore = blobstore.clone();
            move |node| {
                let key = format!("sha1-{}", node.blob.sha1());

                blobstore.get(key).and_then(move |blob| {
                    blob.ok_or(ErrorKind::ContentMissing(nodeid, node.blob).into())
                        .map(|blob| {
                            let (_, off) = file::File::extract_meta(blob.as_ref());
                            Vec::from(&blob.as_ref()[off..])
                        })
                })
            }
        })
        .boxify()
}

impl<B> BlobEntry<B>
where
    B: Blobstore + Sync + Clone,
{
    pub fn new(blobstore: B, path: MPath, nodeid: NodeHash, ty: Type) -> Result<Self> {
        let path = match ty {
            Type::Tree => RepoPath::dir(path)?,
            _ => RepoPath::file(path)?,
        };
        Ok(Self {
            blobstore,
            path,
            id: EntryId::new(nodeid),
            ty,
        })
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

impl<B> Entry for BlobEntry<B>
where
    B: Blobstore + Sync + Clone,
{
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

    fn get_path(&self) -> &RepoPath {
        &self.path
    }

    fn get_mpath(&self) -> &MPath {
        self.path
            .mpath()
            .expect("entries should always have an associated path")
    }
}
