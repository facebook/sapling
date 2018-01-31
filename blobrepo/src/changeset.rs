// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::Arc;

use bincode;
use failure;
use futures::future::{Future, IntoFuture};

use blobstore::Blobstore;

use mercurial::revlogrepo::RevlogChangeset;
use mercurial_types::{Blob, BlobNode, Changeset, MPath, Parents, Time};
use mercurial_types::nodehash::{ChangesetId, ManifestId};

use errors::*;

// In stock mercurial, the revlog acts as an envelope which holds (primarily) the parents
// for each entry. The changelog itself is encoded as a blob within the entry. This structure
// replicates this for use within the blob store. In principle the cs blob and the envelope
// could be stored separately, but I think the disadvantages (more objects, more latency,
// more brittle) outweigh the advantages (potential for sharing changesets, consistency
// with file storage).
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
#[derive(Serialize, Deserialize, HeapSizeOf)]
struct RawCSBlob<'a> {
    parents: Parents,
    blob: Cow<'a, [u8]>,
}

pub struct BlobChangeset {
    changesetid: ChangesetId, // redundant - can be computed from revlogcs?
    revlogcs: RevlogChangeset,
}

fn cskey(changesetid: &ChangesetId) -> String {
    format!("changeset-{}.bincode", changesetid)
}

impl BlobChangeset {
    pub fn new(changesetid: &ChangesetId, revlogcs: RevlogChangeset) -> Self {
        Self {
            changesetid: *changesetid,
            revlogcs,
        }
    }

    pub fn load(
        blobstore: &Arc<Blobstore>,
        changesetid: &ChangesetId,
    ) -> impl Future<Item = Option<Self>, Error = Error> + Send + 'static {
        let changesetid = changesetid.clone();
        let key = cskey(&changesetid);

        blobstore.get(key).and_then(move |got| match got {
            None => Ok(None),
            Some(blob) => {
                let RawCSBlob { parents, blob } = bincode::deserialize(blob.as_ref())?;
                let (p1, p2) = parents.get_nodes();
                let blob = Blob::from(blob.into_owned());
                let node = BlobNode::new(blob, p1, p2);
                let cs = BlobChangeset {
                    changesetid: changesetid,
                    revlogcs: RevlogChangeset::new(node)?,
                };
                Ok(Some(cs))
            }
        })
    }

    pub fn save(
        &self,
        blobstore: Arc<Blobstore>,
    ) -> impl Future<Item = (), Error = Error> + Send + 'static {
        let key = cskey(&self.changesetid);

        self.revlogcs.get_node() // FIXME: generate from scratch
            .map_err(Error::from)
            .and_then(|node| {
                let data = node
                    .as_blob()
                    .as_slice()
                    .ok_or(failure::err_msg("missing changeset blob"))?;
                let blob = RawCSBlob {
                    parents: *self.revlogcs.parents(),
                    blob: Cow::Borrowed(data),
                };
                bincode::serialize(&blob, bincode::Infinite).map_err(Error::from)
            })
            .into_future()
            .and_then(move |blob| blobstore.put(key, blob.into()))
    }
}

impl Changeset for BlobChangeset {
    fn manifestid(&self) -> &ManifestId {
        self.revlogcs.manifestid()
    }

    fn user(&self) -> &[u8] {
        self.revlogcs.user()
    }

    fn extra(&self) -> &BTreeMap<Vec<u8>, Vec<u8>> {
        self.revlogcs.extra()
    }

    fn comments(&self) -> &[u8] {
        self.revlogcs.comments()
    }

    fn files(&self) -> &[MPath] {
        self.revlogcs.files()
    }

    fn time(&self) -> &Time {
        self.revlogcs.time()
    }

    fn parents(&self) -> &Parents {
        self.revlogcs.parents()
    }
}
