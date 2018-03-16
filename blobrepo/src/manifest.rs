// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Root manifest, tree nodes

use std::sync::Arc;

use futures::future::{Future, IntoFuture};
use futures::stream::{self, Stream};
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};

use mercurial::manifest::revlog::ManifestContent;
use mercurial_types::{Entry, MPath, Manifest};
use mercurial_types::nodehash::{HgManifestId, NULL_HASH};

use blobstore::Blobstore;

use errors::*;
use file::BlobEntry;
use utils::get_node;

pub struct BlobManifest {
    blobstore: Arc<Blobstore>,
    content: ManifestContent,
}

impl BlobManifest {
    pub fn load(
        blobstore: &Arc<Blobstore>,
        manifestid: &HgManifestId,
    ) -> BoxFuture<Option<Self>, Error> {
        let nodehash = manifestid.clone().into_nodehash();
        if nodehash == NULL_HASH {
            Ok(Some(BlobManifest {
                blobstore: blobstore.clone(),
                content: ManifestContent::new_empty(),
            })).into_future()
                .boxify()
        } else {
            get_node(blobstore, nodehash)
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
    }

    pub fn parse<D: AsRef<[u8]>>(blobstore: Arc<Blobstore>, data: D) -> Result<Self> {
        Ok(BlobManifest {
            blobstore: blobstore,
            content: ManifestContent::parse(data.as_ref())?,
        })
    }
}

impl Manifest for BlobManifest {
    fn lookup(&self, path: &MPath) -> BoxFuture<Option<Box<Entry + Sync>>, Error> {
        // Path is a single MPathElement. In t25575327 we'll change the type.
        let name = path.clone().into_iter().next_back();

        let res = self.content.files.get(path).map({
            move |d| {
                BlobEntry::new(
                    self.blobstore.clone(),
                    name,
                    d.entryid().into_nodehash(),
                    d.flag(),
                )
            }
        });

        match res {
            Some(e_res) => e_res.map(|e| Some(e.boxed())).into_future().boxify(),
            None => Ok(None).into_future().boxify(),
        }
    }

    fn list(&self) -> BoxStream<Box<Entry + Sync>, Error> {
        let entries = self.content
            .files
            .clone()
            .into_iter()
            .map({
                let blobstore = self.blobstore.clone();
                move |(path, d)| {
                    let name = path.clone().into_iter().next_back();
                    BlobEntry::new(
                        blobstore.clone(),
                        name,
                        d.entryid().into_nodehash(),
                        d.flag(),
                    )
                }
            })
            .map(|e_res| e_res.map(|e| e.boxed()));
        // TODO: (sid0) T23193289 replace with stream::iter_result once that becomes available
        stream::iter_ok(entries).and_then(|x| x).boxify()
    }
}
