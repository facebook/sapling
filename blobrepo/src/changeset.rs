// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::io::Write;

use bytes::Bytes;
use failure;
use futures::future::{Either, Future, IntoFuture};

use blobstore::Blobstore;

use mercurial;
use mercurial::changeset::Extra;
use mercurial::revlogrepo::RevlogChangeset;
use mercurial_types::{Changeset, HgBlob, HgBlobNode, HgNodeHash, HgParents, MPath};
use mercurial_types::nodehash::{HgChangesetId, HgManifestId, NULL_HASH};
use mononoke_types::DateTime;

use errors::*;
use repo::RepoBlobstore;
use utils::{EnvelopeBlob, RawCSBlob};

#[derive(Debug)]
pub struct ChangesetContent {
    p1: Option<HgNodeHash>,
    p2: Option<HgNodeHash>,
    manifestid: HgManifestId,
    user: Vec<u8>,
    time: DateTime,
    extra: Extra,
    files: Vec<MPath>,
    comments: Vec<u8>,
}

impl ChangesetContent {
    pub fn new_from_parts(
        // XXX replace parents with p1 and p2
        parents: HgParents,
        manifestid: HgManifestId,
        user: Vec<u8>,
        time: DateTime,
        extra: BTreeMap<Vec<u8>, Vec<u8>>,
        files: Vec<MPath>,
        comments: Vec<u8>,
    ) -> Self {
        let (p1, p2) = parents.get_nodes();
        Self {
            p1: p1.cloned(),
            p2: p2.cloned(),
            manifestid,
            user,
            time,
            extra: Extra::new(extra),
            files,
            comments,
        }
    }

    pub fn from_revlogcs(revlogcs: RevlogChangeset) -> Self {
        Self {
            p1: revlogcs.p1,
            p2: revlogcs.p2,
            manifestid: revlogcs.manifestid,
            user: revlogcs.user,
            time: revlogcs.time,
            extra: revlogcs.extra,
            files: revlogcs.files,
            comments: revlogcs.comments,
        }
    }

    pub fn compute_hash(&self) -> Result<HgChangesetId> {
        let mut v = Vec::new();

        self.generate(&mut v)?;
        let blobnode = HgBlobNode::new(Bytes::from(v), self.p1(), self.p2());

        let nodeid = blobnode
            .nodeid()
            .ok_or(Error::from(ErrorKind::NodeGenerationFailed))?;
        Ok(HgChangesetId::new(nodeid))
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

    #[inline]
    pub fn p1(&self) -> Option<&HgNodeHash> {
        self.p1.as_ref()
    }

    #[inline]
    pub fn p2(&self) -> Option<&HgNodeHash> {
        self.p2.as_ref()
    }
}

#[derive(Debug)]
pub struct BlobChangeset {
    changesetid: HgChangesetId, // redundant - can be computed from revlogcs?
    content: ChangesetContent,
}

fn cskey(changesetid: &HgChangesetId) -> String {
    format!("changeset-{}.bincode", changesetid)
}

impl BlobChangeset {
    pub fn new(content: ChangesetContent) -> Result<Self> {
        Ok(Self::new_with_id(&content.compute_hash()?, content))
    }

    pub fn new_with_id(changesetid: &HgChangesetId, content: ChangesetContent) -> Self {
        Self {
            changesetid: *changesetid,
            content,
        }
    }

    pub fn get_changeset_id(&self) -> HgChangesetId {
        self.changesetid
    }

    pub fn load(
        blobstore: &RepoBlobstore,
        changesetid: &HgChangesetId,
    ) -> impl Future<Item = Option<Self>, Error = Error> + Send + 'static {
        let changesetid = *changesetid;
        if changesetid == HgChangesetId::new(NULL_HASH) {
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

                    let blob = HgBlob::from(Bytes::from(blob.into_owned()));
                    let node = HgBlobNode::new(blob, p1, p2);
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
        blobstore: RepoBlobstore,
    ) -> impl Future<Item = (), Error = Error> + Send + 'static {
        let key = cskey(&self.changesetid);

        let blob = {
            let mut v = Vec::new();

            self.content
                .generate(&mut v)
                .map(|()| HgBlobNode::new(Bytes::from(v), self.content.p1(), self.content.p2()))
        };

        blob.map_err(Error::from)
            .and_then(|node| {
                let data = node.as_blob()
                    .as_slice()
                    .ok_or(failure::err_msg("missing changeset blob"))?;
                let blob = RawCSBlob {
                    parents: HgParents::new(self.content.p1(), self.content.p2()),
                    blob: Cow::Borrowed(data),
                };
                blob.serialize().into()
            })
            .into_future()
            .and_then(move |blob| blobstore.put(key, blob.into()))
    }

    #[inline]
    pub fn p1(&self) -> Option<&HgNodeHash> {
        self.content.p1()
    }

    #[inline]
    pub fn p2(&self) -> Option<&HgNodeHash> {
        self.content.p2()
    }
}

impl Changeset for BlobChangeset {
    fn manifestid(&self) -> &HgManifestId {
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

    fn parents(&self) -> HgParents {
        // XXX Change this to return p1 and p2 directly.
        HgParents::new(self.content.p1(), self.content.p2())
    }
}
