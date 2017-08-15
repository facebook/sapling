// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Root manifest, tree nodes

use std::collections::BTreeMap;

use futures::future::{BoxFuture, Future, IntoFuture};
use futures::stream::{self, BoxStream, Stream};

use mercurial_types::{Entry, Manifest, NodeHash, Path};
use mercurial::manifest::revlog::{self, Details};

use blobstore::Blobstore;

use errors::*;
use file::BlobEntry;
use utils::get_node;

pub struct BlobManifest<B> {
    blobstore: B,
    files: BTreeMap<Path, Details>,
}

impl<B> BlobManifest<B>
where
    B: Blobstore<Key = String> + Clone,
    B::ValueOut: AsRef<[u8]>,
{
    pub fn load(blobstore: &B, manifestid: &NodeHash) -> BoxFuture<Option<Self>, Error> {
        get_node(blobstore, manifestid.clone())
            .and_then({
                let blobstore = blobstore.clone();
                move |nodeblob| {
                    let blobkey = format!("sha1:{}", nodeblob.blob);
                    blobstore.get(&blobkey).map_err(blobstore_err)
                }
            })
            .and_then({
                let blobstore = blobstore.clone();
                move |got| match got {
                    None => Ok(None),
                    Some(blob) => Ok(Some(Self::parse(blobstore, blob)?)),
                }
            })
            .boxed()
    }

    pub fn parse<D: AsRef<[u8]>>(blobstore: B, data: D) -> Result<Self> {
        Ok(BlobManifest {
            blobstore: blobstore,
            files: revlog::parse(data.as_ref())?,
        })
    }
}

impl<B> Manifest for BlobManifest<B>
    where B: Blobstore<Key=String> + Sync + Clone,
          B::ValueOut: AsRef<[u8]> + Send, {
    type Error = Error;

    fn lookup(
        &self,
        path: &Path,
    ) -> BoxFuture<Option<Box<Entry<Error = Self::Error>>>, Self::Error> {
        let res = self.files
            .get(path)
            .map({
                let blobstore = self.blobstore.clone();
                move |d| BlobEntry::new(blobstore, path.clone(), *d.nodeid(), d.flag())
            })
            .map(|e| e.boxed());

        Ok(res).into_future().boxed()
    }

    fn list(&self) -> BoxStream<Box<Entry<Error = Self::Error>>, Self::Error> {
        let entries = self.files
            .clone()
            .into_iter()
            .map({
                let blobstore = self.blobstore.clone();
                move |(path, d)| BlobEntry::new(blobstore.clone(), path, *d.nodeid(), d.flag())
            })
            .map(|e| Ok(e.boxed()));
        stream::iter(entries).boxed()
    }
}
