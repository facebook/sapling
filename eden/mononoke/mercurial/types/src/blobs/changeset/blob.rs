/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::revlog::{serialize_extras, Extra, RevlogChangeset};
use crate::{
    nodehash::{HgChangesetId, HgManifestId},
    HgBlobNode, HgChangesetEnvelopeMut, HgNodeHash, HgParents, MPath,
};
use anyhow::{Error, Result};
use blobstore::{Blobstore, Loadable, LoadableError};
use bytes::Bytes;
use context::CoreContext;
use futures::{
    compat::Future01CompatExt,
    future::{BoxFuture, FutureExt, TryFutureExt},
};
use futures_old::future::{Future, IntoFuture};
use mononoke_types::DateTime;
use std::fmt::{self, Display};
use std::{collections::BTreeMap, io::Write};

const STEP_PARENTS_METADATA_KEY: &str = "stepparents";

pub struct ChangesetMetadata {
    pub user: String,
    pub time: DateTime,
    pub extra: BTreeMap<Vec<u8>, Vec<u8>>,
    pub message: String,
}

impl ChangesetMetadata {
    pub fn record_step_parents(&mut self, step_parents: impl Iterator<Item = HgChangesetId>) {
        let mut meta = Vec::new();

        for (idx, parent) in step_parents.enumerate() {
            if idx > 0 {
                write!(meta, ",").expect("writes to memory don't fail");
            }
            write!(meta, "{}", parent).expect("writes to memory don't fail");
        }

        if meta.len() == 0 {
            return;
        }

        self.extra.insert(STEP_PARENTS_METADATA_KEY.into(), meta);
    }
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
    message: Vec<u8>,
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
            message: cs_metadata.message.into_bytes(),
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
            message: revlogcs.message,
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
        out.write_all(&self.message)?;

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
        RevlogChangeset::load(ctx, blobstore, changesetid).map(move |got| {
            got.map(|revlogcs| {
                HgBlobChangeset::new_with_id(
                    changesetid,
                    HgChangesetContent::from_revlogcs(revlogcs),
                )
            })
        })
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
            .and_then(move |blob| blobstore.put(ctx, key, blob.into()).compat())
    }

    #[inline]
    pub fn p1(&self) -> Option<HgNodeHash> {
        self.content.p1()
    }

    #[inline]
    pub fn p2(&self) -> Option<HgNodeHash> {
        self.content.p2()
    }

    pub fn manifestid(&self) -> HgManifestId {
        self.content.manifestid
    }

    pub fn user(&self) -> &[u8] {
        &self.content.user
    }

    pub fn extra(&self) -> &BTreeMap<Vec<u8>, Vec<u8>> {
        self.content.extra.as_ref()
    }

    pub fn message(&self) -> &[u8] {
        &self.content.message
    }

    pub fn files(&self) -> &[MPath] {
        &self.content.files
    }

    pub fn time(&self) -> &DateTime {
        &self.content.time
    }

    pub fn parents(&self) -> HgParents {
        // XXX Change this to return p1 and p2 directly.
        HgParents::new(self.content.p1(), self.content.p2())
    }

    pub fn step_parents(&self) -> Result<Vec<HgNodeHash>> {
        let mut ret = vec![];

        if let Some(step_parents) = self.extra().get(STEP_PARENTS_METADATA_KEY.as_bytes()) {
            let step_parents = std::str::from_utf8(step_parents)?;
            for csid in step_parents.split(",") {
                let csid = csid.parse()?;
                ret.push(csid);
            }
        }

        Ok(ret)
    }
}

impl Loadable for HgChangesetId {
    type Value = HgBlobChangeset;

    fn load<B: Blobstore + Clone>(
        &self,
        ctx: CoreContext,
        blobstore: &B,
    ) -> BoxFuture<'static, Result<Self::Value, LoadableError>> {
        let csid = *self;
        let load = HgBlobChangeset::load(ctx, blobstore, csid).compat();
        async move {
            let value = load.await?;
            value.ok_or_else(|| LoadableError::Missing(csid.blobstore_key()))
        }
        .boxed()
    }
}

impl Display for HgBlobChangeset {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let message = self.message();
        let title_end = message
            .iter()
            .enumerate()
            .find(|(_, &c)| c == b'\n')
            .map(|(i, _)| i)
            .unwrap_or(message.len());

        write!(
            f,
            "changeset: {}\nauthor: {}\ndate: {}\nsummary: {}\n",
            self.changesetid,
            String::from_utf8_lossy(&self.user()),
            self.time().as_chrono().to_rfc2822(),
            String::from_utf8_lossy(&self.message()[0..title_end])
        )
    }
}
