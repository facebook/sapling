// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Plain files, symlinks

use futures::future::{BoxFuture, Future};

use mercurial_types::{Blob, NodeHash, Parents, Path};
use mercurial_types::manifest::{Content, Entry, Manifest, Type};

use blobstore::Blobstore;

use errors::*;

use manifest::BlobManifest;

use utils::{get_node, RawNodeBlob};

pub struct BlobEntry<B> {
    blobstore: B,
    path: Path, // XXX full path? Parent reference?
    nodeid: NodeHash,
    ty: Type,
}

pub fn fetch_file_blob_from_blobstore<B>(
    blobstore: B,
    nodeid: NodeHash,
) -> BoxFuture<Vec<u8>, Error>
where
    B: Blobstore<Key = String> + Clone,
    B::ValueOut: AsRef<[u8]>,
{
    get_node(&blobstore, nodeid)
        .and_then({
            let blobstore = blobstore.clone();
            move |node| {
                let key = format!("sha1:{}", node.blob);

                blobstore
                    .get(&key)
                    .map_err(blobstore_err)
                    .and_then(move |blob| {
                        blob.ok_or(ErrorKind::ContentMissing(nodeid, node.blob).into())
                    })
            }
        })
        .and_then({
            |blob| {
                Ok(Vec::from(blob.as_ref()))
            }
        })
        .boxed()
}

impl<B> BlobEntry<B>
where
    B: Blobstore<Key = String> + Sync + Clone,
    B::ValueOut: AsRef<[u8]>,
{
    pub fn new(blobstore: B, path: Path, nodeid: NodeHash, ty: Type) -> Self {
        Self {
            blobstore,
            path,
            nodeid,
            ty,
        }
    }

    fn get_node(&self) -> BoxFuture<RawNodeBlob, Error> {
        get_node(&self.blobstore, self.nodeid)
    }

    fn get_raw_content_inner(&self) -> BoxFuture<Vec<u8>, Error> {
        let nodeid = self.nodeid;
        let blobstore = self.blobstore.clone();

        self.get_node()
            .and_then({
                let blobstore = blobstore.clone();
                move |node| {
                    let key = format!("sha1:{}", node.blob);

                    blobstore
                        .get(&key)
                        .map_err(blobstore_err)
                        .and_then(move |blob| {
                            blob.ok_or(ErrorKind::ContentMissing(nodeid, node.blob).into())
                        })
                        .map(|blob| Vec::from(blob.as_ref()))
                }
            })
            .boxed()
    }
}

impl<B> Entry for BlobEntry<B>
where
    B: Blobstore<Key = String> + Sync + Clone,
    B::ValueOut: AsRef<[u8]>,
{
    type Error = Error;

    fn get_type(&self) -> Type {
        self.ty
    }

    fn get_parents(&self) -> BoxFuture<Parents, Self::Error> {
        self.get_node().map(|node| node.parents).boxed()
    }

    fn get_raw_content(&self) -> BoxFuture<Blob<Vec<u8>>, Self::Error> {
        self.get_raw_content_inner()
            .map(|blob| Blob::from(blob.as_ref()))
            .boxed()
    }

    fn get_content(&self) -> BoxFuture<Content<Self::Error>, Self::Error> {
        let blobstore = self.blobstore.clone();
        self.get_raw_content_inner()
            .and_then({
                let ty = self.ty;
                move |blob| {
                    let blob = blob.as_ref();

                    let res = match ty {
                        Type::File => Content::File(Blob::from(blob)),
                        Type::Executable => Content::Executable(Blob::from(blob)),
                        Type::Symlink => Content::Symlink(Path::new(blob)?),
                        Type::Tree => Content::Tree(BlobManifest::parse(blobstore, blob)?.boxed()),
                    };

                    Ok(res)
                }
            })
            .boxed()
    }

    fn get_size(&self) -> BoxFuture<Option<usize>, Self::Error> {
        self.get_content()
            .and_then(|content| match content {
                Content::File(data) | Content::Executable(data) => Ok(data.size()),
                Content::Symlink(path) => Ok(Some(path.to_vec().len())),
                Content::Tree(_) => Ok(None),
            })
            .boxed()
    }

    fn get_hash(&self) -> &NodeHash {
        &self.nodeid
    }

    fn get_path(&self) -> &Path {
        &self.path
    }
}
