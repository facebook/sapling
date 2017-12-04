// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use futures::future::{Future, IntoFuture};
use futures_ext::{BoxFuture, FutureExt};

use bincode;

use blobstore::Blobstore;
use mercurial_types::{BlobHash, NodeHash, Parents};

use errors::*;

#[derive(Debug, Copy, Clone)]
#[derive(Serialize, Deserialize)]
pub struct RawNodeBlob {
    pub parents: Parents,
    pub blob: BlobHash,
}

pub fn get_node<B>(blobstore: &B, nodeid: NodeHash) -> BoxFuture<RawNodeBlob, Error>
where
    B: Blobstore,
{
    let key = format!("node-{}.bincode", nodeid);

    blobstore
        .get(key)
        .map_err(blobstore_err)
        .and_then(move |got| got.ok_or(ErrorKind::NodeMissing(nodeid).into()))
        .and_then(move |blob| {
            bincode::deserialize(blob.as_ref()).into_future().from_err()
        })
        .boxify()
}
