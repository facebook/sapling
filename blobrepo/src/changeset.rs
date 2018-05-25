// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::io::Write;
use std::sync::Arc;

use bytes::Bytes;
use failure;
use futures::future::{Either, Future, IntoFuture};

use blobstore::Blobstore;

use mercurial::{self, HgBlobNode, NodeHashConversion};
use mercurial::changeset::Extra;
use mercurial::revlogrepo::RevlogChangeset;
use mercurial_types::{Changeset, DBlobNode, DParents, HgBlob, MPath};
use mercurial_types::nodehash::{DChangesetId, DManifestId, D_NULL_HASH};
use mononoke_types::DateTime;

use errors::*;
use utils::{EnvelopeBlob, RawCSBlob};

#[derive(Debug)]
pub struct ChangesetContent {
    parents: DParents,
    manifestid: DManifestId,
    user: Vec<u8>,
    time: DateTime,
    extra: Extra,
    files: Vec<MPath>,
    comments: Vec<u8>,
}

impl ChangesetContent {
    pub fn new_from_parts(
        parents: DParents,
        manifestid: DManifestId,
        user: Vec<u8>,
        time: DateTime,
        extra: BTreeMap<Vec<u8>, Vec<u8>>,
        files: Vec<MPath>,
        comments: Vec<u8>,
    ) -> Self {
        Self {
            parents,
            manifestid,
            user,
            time,
            extra: Extra::new(extra),
            files,
            comments,
        }
    }

    pub fn from_revlogcs(revlogcs: RevlogChangeset) -> Self {
        // TODO(luk): T28348119 Following usages of into_mononoke are valid, because we use
        // RevlogChangeset to parse content of Blobstore and immediately create a BlobChangeset out
        // of it. In future this logic will go away, because we will either retrieve
        // BonsaiChangesets or we will fetch RevlogChangeset just to pass it to client untouched.
        let parents = {
            let (p1, p2) = revlogcs.parents.get_nodes();
            let p1 = p1.map(|p| p.into_mononoke());
            let p2 = p2.map(|p| p.into_mononoke());
            DParents::new(p1.as_ref(), p2.as_ref())
        };

        let manifestid = DManifestId::new(revlogcs.manifestid.into_nodehash().into_mononoke());

        Self {
            parents,
            manifestid,
            user: revlogcs.user,
            time: revlogcs.time,
            extra: revlogcs.extra,
            files: revlogcs.files,
            comments: revlogcs.comments,
        }
    }

    pub fn compute_hash(&self) -> Result<DChangesetId> {
        let mut v = Vec::new();

        self.generate(&mut v)?;
        let (p1, p2) = self.parents.get_nodes();
        let blobnode = DBlobNode::new(Bytes::from(v), p1, p2);

        let nodeid = blobnode
            .nodeid()
            .ok_or(Error::from(ErrorKind::NodeGenerationFailed))?;
        Ok(DChangesetId::new(nodeid))
    }

    /// Generate a serialized changeset. This is the counterpart to parse, and generates
    /// in the same format as Mercurial. It should be bit-for-bit identical in fact.
    fn generate<W: Write>(&self, out: &mut W) -> Result<()> {
        write!(out, "{}\n", self.manifestid.into_nodehash())?;
        out.write_all(&self.user)?;
        out.write_all(b"\n")?;
        write!(
            out,
            "{} {}",
            self.time.timestamp_secs(),
            self.time.tz_offset_secs()
        )?;

        if !self.extra.is_empty() {
            write!(out, " ")?;
            mercurial::changeset::serialize_extras(&self.extra, out)?;
        }

        write!(out, "\n")?;
        for f in &self.files {
            write!(out, "{}\n", f)?;
        }
        write!(out, "\n")?;
        out.write_all(&self.comments)?;

        Ok(())
    }
}

#[derive(Debug)]
pub struct BlobChangeset {
    changesetid: DChangesetId, // redundant - can be computed from revlogcs?
    content: ChangesetContent,
}

fn cskey(changesetid: &DChangesetId) -> String {
    format!("changeset-{}.bincode", changesetid)
}

impl BlobChangeset {
    pub fn new(content: ChangesetContent) -> Result<Self> {
        Ok(Self::new_with_id(&content.compute_hash()?, content))
    }

    pub fn new_with_id(changesetid: &DChangesetId, content: ChangesetContent) -> Self {
        Self {
            changesetid: *changesetid,
            content,
        }
    }

    pub fn get_changeset_id(&self) -> DChangesetId {
        self.changesetid
    }

    pub fn load(
        blobstore: &Arc<Blobstore>,
        changesetid: &DChangesetId,
    ) -> impl Future<Item = Option<Self>, Error = Error> + Send + 'static {
        let changesetid = *changesetid;
        if changesetid == DChangesetId::new(D_NULL_HASH) {
            let revlogcs = RevlogChangeset::new_null();
            let cs =
                BlobChangeset::new_with_id(&changesetid, ChangesetContent::from_revlogcs(revlogcs));
            Either::A(Ok(Some(cs)).into_future())
        } else {
            let key = cskey(&changesetid);

            let fut = blobstore.get(key).and_then(move |got| match got {
                None => Ok(None),
                Some(bytes) => {
                    // TODO(luk): T28348119 Following usages of into_mercurial are valid, because
                    // we use RevlogChangeset to decode content of Blobstore and immediately create
                    // a BlobChangeset out of it. In future this logic will go away, because we
                    // will either retrieve BonsaiChangesets or we will fetch RevlogChangeset just
                    // to pass it to client untouched.
                    let RawCSBlob { parents, blob } =
                        RawCSBlob::deserialize(&EnvelopeBlob::from(bytes))?;
                    let (p1, p2) = parents.get_nodes();
                    let p1 = p1.map(|p| p.into_mercurial());
                    let p2 = p2.map(|p| p.into_mercurial());

                    let blob = HgBlob::from(Bytes::from(blob.into_owned()));
                    let node = HgBlobNode::new(blob, p1.as_ref(), p2.as_ref());
                    let cs = BlobChangeset::new_with_id(
                        &changesetid,
                        ChangesetContent::from_revlogcs(RevlogChangeset::new(node)?),
                    );
                    Ok(Some(cs))
                }
            });
            Either::B(fut)
        }
    }

    pub fn save(
        &self,
        blobstore: Arc<Blobstore>,
    ) -> impl Future<Item = (), Error = Error> + Send + 'static {
        let key = cskey(&self.changesetid);

        let blob = {
            let mut v = Vec::new();

            self.content.generate(&mut v).map(|()| {
                let (p1, p2) = self.content.parents.get_nodes();
                DBlobNode::new(Bytes::from(v), p1, p2)
            })
        };

        blob.map_err(Error::from)
            .and_then(|node| {
                let data = node.as_blob()
                    .as_slice()
                    .ok_or(failure::err_msg("missing changeset blob"))?;
                let blob = RawCSBlob {
                    parents: self.content.parents,
                    blob: Cow::Borrowed(data),
                };
                blob.serialize().into()
            })
            .into_future()
            .and_then(move |blob| blobstore.put(key, blob.into()))
    }
}

impl Changeset for BlobChangeset {
    fn manifestid(&self) -> &DManifestId {
        &self.content.manifestid
    }

    fn user(&self) -> &[u8] {
        &self.content.user
    }

    fn extra(&self) -> &BTreeMap<Vec<u8>, Vec<u8>> {
        self.content.extra.as_ref()
    }

    fn comments(&self) -> &[u8] {
        &self.content.comments
    }

    fn files(&self) -> &[MPath] {
        &self.content.files
    }

    fn time(&self) -> &DateTime {
        &self.content.time
    }

    fn parents(&self) -> &DParents {
        &self.content.parents
    }
}
