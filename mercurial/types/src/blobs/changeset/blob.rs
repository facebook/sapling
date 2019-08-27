// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use super::revlog::{serialize_extras, Extra, RevlogChangeset};
use crate::{
    nodehash::{HgChangesetId, HgManifestId, NULL_HASH},
    Changeset, HgBlobNode, HgChangesetEnvelope, HgChangesetEnvelopeMut, HgNodeHash, HgParents,
    MPath,
};
use blobstore::{Blobstore, Loadable, LoadableError};
use bytes::Bytes;
use context::CoreContext;
use failure_ext::{bail_msg, Error, FutureFailureErrorExt, Result};
use futures::future::{Either, Future, IntoFuture};
use futures_ext::{BoxFuture, FutureExt};
use mononoke_types::DateTime;
use std::{collections::BTreeMap, io::Write};

pub struct ChangesetMetadata {
    pub user: String,
    pub time: DateTime,
    pub extra: BTreeMap<Vec<u8>, Vec<u8>>,
    pub comments: String,
}

#[derive(Debug, Clone)]
pub struct HgChangesetContent {
    p1: Option<HgNodeHash>,
    p2: Option<HgNodeHash>,
    manifestid: HgManifestId,
    user: Vec<u8>,
    time: DateTime,
    extra: Extra,
    files: Vec<MPath>,
    comments: Vec<u8>,
}

impl HgChangesetContent {
    pub fn new_from_parts(
        // XXX replace parents with p1 and p2
        parents: HgParents,
        manifestid: HgManifestId,
        cs_metadata: ChangesetMetadata,
        files: Vec<MPath>,
    ) -> Self {
        let (p1, p2) = parents.get_nodes();
        Self {
            p1,
            p2,
            manifestid,
            user: cs_metadata.user.into_bytes(),
            time: cs_metadata.time,
            extra: Extra::new(cs_metadata.extra),
            files,
            comments: cs_metadata.comments.into_bytes(),
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

        let nodeid = blobnode.nodeid();
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
            serialize_extras(&self.extra, out)?;
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
    pub fn p1(&self) -> Option<HgNodeHash> {
        self.p1
    }

    #[inline]
    pub fn p2(&self) -> Option<HgNodeHash> {
        self.p2
    }
}

#[derive(Debug, Clone)]
pub struct HgBlobChangeset {
    changesetid: HgChangesetId, // redundant - can be computed from revlogcs?
    content: HgChangesetContent,
}

impl HgBlobChangeset {
    pub fn new(content: HgChangesetContent) -> Result<Self> {
        Ok(Self::new_with_id(content.compute_hash()?, content))
    }

    pub fn new_with_id(changesetid: HgChangesetId, content: HgChangesetContent) -> Self {
        Self {
            changesetid,
            content,
        }
    }

    pub fn get_changeset_id(&self) -> HgChangesetId {
        self.changesetid
    }

    pub fn load<B: Blobstore + Clone>(
        ctx: CoreContext,
        blobstore: &B,
        changesetid: HgChangesetId,
    ) -> impl Future<Item = Option<Self>, Error = Error> + Send + 'static {
        if changesetid == HgChangesetId::new(NULL_HASH) {
            let revlogcs = RevlogChangeset::new_null();
            let cs = HgBlobChangeset::new_with_id(
                changesetid,
                HgChangesetContent::from_revlogcs(revlogcs),
            );
            Either::A(Ok(Some(cs)).into_future())
        } else {
            let key = changesetid.blobstore_key();

            let fut = blobstore
                .get(ctx, key.clone())
                .and_then(move |got| match got {
                    None => Ok(None),
                    Some(bytes) => {
                        let envelope = HgChangesetEnvelope::from_blob(bytes.into())?;
                        if changesetid != envelope.node_id() {
                            bail_msg!(
                                "Changeset ID mismatch (requested: {}, got: {})",
                                changesetid,
                                envelope.node_id()
                            );
                        }
                        let revlogcs = RevlogChangeset::from_envelope(envelope)?;
                        let cs = HgBlobChangeset::new_with_id(
                            changesetid,
                            HgChangesetContent::from_revlogcs(revlogcs),
                        );
                        Ok(Some(cs))
                    }
                })
                .with_context(move |_| {
                    format!(
                        "Error while deserializing changeset retrieved from key '{}'",
                        key
                    )
                })
                .from_err();
            Either::B(fut)
        }
    }

    pub fn save(
        &self,
        ctx: CoreContext,
        blobstore: impl Blobstore + Clone,
    ) -> impl Future<Item = (), Error = Error> + Send + 'static {
        let key = self.changesetid.blobstore_key();

        let blob = {
            let mut v = Vec::new();

            self.content.generate(&mut v).map(|()| Bytes::from(v))
        };

        blob.map_err(Error::from)
            .and_then(|contents| {
                let envelope = HgChangesetEnvelopeMut {
                    node_id: self.changesetid,
                    p1: self.content.p1().map(HgChangesetId::new),
                    p2: self.content.p2().map(HgChangesetId::new),
                    contents,
                };
                let envelope = envelope.freeze();
                Ok(envelope.into_blob())
            })
            .into_future()
            .and_then(move |blob| blobstore.put(ctx, key, blob.into()))
    }

    #[inline]
    pub fn p1(&self) -> Option<HgNodeHash> {
        self.content.p1()
    }

    #[inline]
    pub fn p2(&self) -> Option<HgNodeHash> {
        self.content.p2()
    }
}

impl Loadable for HgChangesetId {
    type Value = HgBlobChangeset;

    fn load<B: Blobstore + Clone>(
        &self,
        ctx: CoreContext,
        blobstore: &B,
    ) -> BoxFuture<Self::Value, LoadableError> {
        HgBlobChangeset::load(ctx, blobstore, *self)
            .from_err()
            .and_then(|value| value.ok_or(LoadableError::Missing))
            .boxify()
    }
}

impl Changeset for HgBlobChangeset {
    fn manifestid(&self) -> HgManifestId {
        self.content.manifestid
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
