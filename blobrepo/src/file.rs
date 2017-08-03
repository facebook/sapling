// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Plain files, symlinks

use futures::future::{BoxFuture, Future, IntoFuture};

use bincode;

use mercurial_types::{Blob, NodeHash, Parents, Path, hash};
use mercurial_types::manifest::{Content, Entry, Manifest, Type};

use blobstore::Blobstore;

use errors::*;

use manifest::BlobManifest;

pub struct BlobEntry<B> {
    blobstore: B,
    path: Path, // XXX full path? Parent reference?
    nodeid: NodeHash,
    ty: Type,
}

#[derive(Debug, Copy, Clone)]
#[derive(Serialize, Deserialize)]
pub struct RawNodeBlob {
    parents: Parents,
    blob: hash::Sha1,
}

impl<B> BlobEntry<B>
where
    B: Blobstore<Key = String>,
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
        let nodeid = self.nodeid;
        let key = format!("node:{}.bincode", self.nodeid);

        self.blobstore
            .get(&key)
            .map_err(blobstore_err)
            .and_then(move |got| got.ok_or(ErrorKind::NodeMissing(nodeid).into()))
            .and_then(move |blob| {
                bincode::deserialize(blob.as_ref()).into_future().from_err()
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

    fn get_content(&self) -> BoxFuture<Content<Self::Error>, Self::Error> {
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
                }
            })
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

    fn get_hash(&self) -> &NodeHash {
        &self.nodeid
    }

    fn get_path(&self) -> &Path {
        &self.path
    }
}
