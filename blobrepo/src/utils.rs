// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::borrow::Cow;

use bytes::Bytes;
use futures::future::{Future, IntoFuture};
use futures_ext::{BoxFuture, FutureExt};

use bincode;

use blobstore::Blobstore;
use mercurial::HgNodeHash;
use mercurial_types::{DNodeHash, DParents, HgBlobHash};

use errors::*;

#[derive(Debug, Copy, Clone)]
#[derive(Serialize, Deserialize)]
pub struct RawNodeBlob {
    pub parents: DParents,
    pub blob: HgBlobHash,
}

impl RawNodeBlob {
    pub fn serialize(&self, nodeid: &HgNodeHash) -> Result<Bytes> {
        let serialized = bincode::serialize(self)
            .map_err(|err| Error::from(ErrorKind::SerializationFailed(*nodeid, err)))?;
        Ok(serialized.into())
    }

    pub fn deserialize(blob: &Bytes) -> Result<Self> {
        Ok(bincode::deserialize(blob.as_ref())?)
    }
}

// In stock mercurial, the revlog acts as an envelope which holds (primarily) the parents
// for each entry. The changelog itself is encoded as a blob within the entry. This structure
// replicates this for use within the blob store. In principle the cs blob and the envelope
// could be stored separately, but I think the disadvantages (more objects, more latency,
// more brittle) outweigh the advantages (potential for sharing changesets, consistency
// with file storage).
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
#[derive(Serialize, Deserialize, HeapSizeOf)]
pub struct RawCSBlob<'a> {
    pub parents: DParents,
    pub blob: Cow<'a, [u8]>,
}

impl<'a> RawCSBlob<'a> {
    pub(crate) fn serialize(&self) -> Result<Bytes> {
        let serialized = bincode::serialize(self)?;
        // XXX better error message?
        Ok(serialized.into())
    }

    pub(crate) fn deserialize(blob: &Bytes) -> Result<Self> {
        Ok(bincode::deserialize(blob.as_ref())?)
    }
}

pub fn get_node_key(nodeid: DNodeHash) -> String {
    format!("node-{}.bincode", nodeid)
}

pub fn get_node(blobstore: &Blobstore, nodeid: DNodeHash) -> BoxFuture<RawNodeBlob, Error> {
    let key = get_node_key(nodeid);

    blobstore
        .get(key)
        .and_then(move |got| got.ok_or(ErrorKind::NodeMissing(nodeid).into()))
        .and_then(move |blob| RawNodeBlob::deserialize(&blob).into_future())
        .boxify()
}
