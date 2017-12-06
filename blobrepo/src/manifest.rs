// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Root manifest, tree nodes

use std::collections::BTreeMap;

use futures::future::{Future, IntoFuture};
use futures::stream::{self, Stream};
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};

use mercurial::manifest::revlog::{self, Details};
use mercurial_types::{Entry, MPath, Manifest, NodeHash};

use blobstore::Blobstore;

use errors::*;
use file::BlobEntry;
use utils::get_node;

pub struct BlobManifest<B> {
    blobstore: B,
    files: BTreeMap<MPath, Details>,
}

impl<B> BlobManifest<B>
where
    B: Blobstore + Clone,
{
    pub fn load(blobstore: &B, manifestid: &NodeHash) -> BoxFuture<Option<Self>, Error> {
        get_node(blobstore, manifestid.clone())
            .and_then({
                let blobstore = blobstore.clone();
                move |nodeblob| {
                    let blobkey = format!("sha1-{}", nodeblob.blob.sha1());
                    blobstore.get(blobkey)
                }
            })
            .and_then({
                let blobstore = blobstore.clone();
                move |got| match got {
                    None => Ok(None),
                    Some(blob) => Ok(Some(Self::parse(blobstore, blob)?)),
                }
            })
            .boxify()
    }

    pub fn parse<D: AsRef<[u8]>>(blobstore: B, data: D) -> Result<Self> {
        Ok(BlobManifest {
            blobstore: blobstore,
            files: revlog::parse(data.as_ref())?,
        })
    }
}

impl<B> Manifest for BlobManifest<B>
where
    B: Blobstore + Sync + Clone,
{
    fn lookup(&self, path: &MPath) -> BoxFuture<Option<Box<Entry + Sync>>, Error> {
        let res = self.files.get(path).map({
            let blobstore = self.blobstore.clone();
            move |d| BlobEntry::new(blobstore, path.clone(), *d.nodeid(), d.flag())
        });

        match res {
            Some(e_res) => e_res.map(|e| Some(e.boxed())).into_future().boxify(),
            None => Ok(None).into_future().boxify(),
        }
    }

    fn list(&self) -> BoxStream<Box<Entry + Sync>, Error> {
        let entries = self.files
            .clone()
            .into_iter()
            .map({
                let blobstore = self.blobstore.clone();
                move |(path, d)| BlobEntry::new(blobstore.clone(), path, *d.nodeid(), d.flag())
            })
            .map(|e_res| e_res.map(|e| e.boxed()));
        // TODO: (sid0) T23193289 replace with stream::iter_result once that becomes available
        stream::iter_ok(entries).and_then(|x| x).boxify()
    }
}
