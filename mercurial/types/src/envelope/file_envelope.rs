/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

//! Envelopes used for file nodes.

use std::fmt;

use anyhow::{Context, Error, Result};
use bytes::Bytes;
use failure_ext::chain::ChainExt;
use fbthrift::compact_protocol;
use quickcheck::{empty_shrinker, Arbitrary, Gen};

use mononoke_types::ContentId;

use super::HgEnvelopeBlob;
use crate::errors::*;
use crate::nodehash::{HgFileNodeId, HgNodeHash};
use crate::thrift;

/// A mutable representation of a Mercurial file node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HgFileEnvelopeMut {
    pub node_id: HgFileNodeId,
    pub p1: Option<HgFileNodeId>,
    pub p2: Option<HgFileNodeId>,
    pub content_id: ContentId,
    pub content_size: u64,
    pub metadata: Bytes,
}

impl HgFileEnvelopeMut {
    pub fn freeze(self) -> HgFileEnvelope {
        HgFileEnvelope { inner: self }
    }
}

impl fmt::Display for HgFileEnvelopeMut {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "node id: {}", self.node_id)?;
        writeln!(
            f,
            "p1: {}",
            HgNodeHash::display_opt(self.p1.map(HgFileNodeId::into_nodehash).as_ref())
        )?;
        writeln!(
            f,
            "p2: {}",
            HgNodeHash::display_opt(self.p2.map(HgFileNodeId::into_nodehash).as_ref())
        )?;
        writeln!(f, "content id: {}", self.content_id)?;
        writeln!(f, "content size: {}", self.content_size)?;
        writeln!(f, "metadata: {:?}", self.metadata)
    }
}

/// A serialized representation of a Mercurial file node in the blob store.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HgFileEnvelope {
    inner: HgFileEnvelopeMut,
}

impl HgFileEnvelope {
    pub(crate) fn from_thrift(fe: thrift::HgFileEnvelope) -> Result<Self> {
        let catch_block = || -> Result<_> {
            Ok(Self {
                inner: HgFileEnvelopeMut {
                    node_id: HgFileNodeId::new(HgNodeHash::from_thrift(fe.node_id)?),
                    p1: HgNodeHash::from_thrift_opt(fe.p1)?.map(HgFileNodeId::new),
                    p2: HgNodeHash::from_thrift_opt(fe.p2)?.map(HgFileNodeId::new),
                    content_id: ContentId::from_thrift(
                        fe.content_id
                            .ok_or_else(|| Error::msg("missing content id field"))?,
                    )?,
                    content_size: fe.content_size as u64,
                    // metadata will always be stored, even if it's length 0
                    metadata: Bytes::from(
                        fe.metadata
                            .ok_or_else(|| Error::msg("missing metadata field"))?,
                    ),
                },
            })
        };

        Ok(catch_block().with_context(|| {
            ErrorKind::InvalidThrift("HgFileEnvelope".into(), "Invalid file envelope".into())
        })?)
    }

    pub fn from_blob(blob: HgEnvelopeBlob) -> Result<Self> {
        let thrift_tc = compact_protocol::deserialize(blob.0.as_ref())
            .chain_err(ErrorKind::BlobDeserializeError("HgFileEnvelope".into()))?;
        Self::from_thrift(thrift_tc)
    }

    /// The ID for this file version.
    #[inline]
    pub fn node_id(&self) -> HgFileNodeId {
        self.inner.node_id
    }

    /// The parent hashes for this node. The order matters.
    #[inline]
    pub fn parents(&self) -> (Option<HgFileNodeId>, Option<HgFileNodeId>) {
        (self.inner.p1, self.inner.p2)
    }

    /// The content ID -- this can be used to retrieve the contents.
    #[inline]
    pub fn content_id(&self) -> ContentId {
        self.inner.content_id
    }

    /// The size of the content ID, not counting the metadata.
    #[inline]
    pub fn content_size(&self) -> u64 {
        self.inner.content_size
    }

    /// The metadata for this node, exactly as provided to Mercurial. This is extracted from
    /// and prepended to the content for Mercurial.
    #[inline]
    pub fn metadata(&self) -> &Bytes {
        &self.inner.metadata
    }

    /// Convert into a mutable representation.
    #[inline]
    pub fn into_mut(self) -> HgFileEnvelopeMut {
        self.inner
    }

    pub(crate) fn into_thrift(self) -> thrift::HgFileEnvelope {
        let inner = self.inner;
        thrift::HgFileEnvelope {
            node_id: inner.node_id.into_nodehash().into_thrift(),
            p1: inner.p1.map(|p| p.into_nodehash().into_thrift()),
            p2: inner.p2.map(|p| p.into_nodehash().into_thrift()),
            content_id: Some(inner.content_id.into_thrift()),
            content_size: inner.content_size as i64,
            metadata: Some(inner.metadata.to_vec()),
        }
    }

    /// Serialize this structure into a blob.
    #[inline]
    pub fn into_blob(self) -> HgEnvelopeBlob {
        let thrift = self.into_thrift();
        HgEnvelopeBlob(compact_protocol::serialize(&thrift))
    }
}

impl fmt::Display for HgFileEnvelope {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.inner.fmt(f)
    }
}

impl Arbitrary for HgFileEnvelope {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        HgFileEnvelope {
            inner: HgFileEnvelopeMut {
                node_id: Arbitrary::arbitrary(g),
                p1: Arbitrary::arbitrary(g),
                p2: Arbitrary::arbitrary(g),
                content_id: Arbitrary::arbitrary(g),
                content_size: Arbitrary::arbitrary(g),
                metadata: Bytes::from(Vec::arbitrary(g)),
            },
        }
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        empty_shrinker()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use quickcheck::quickcheck;

    quickcheck! {
        fn thrift_roundtrip(fe: HgFileEnvelope) -> bool {
            let thrift_fe = fe.clone().into_thrift();
            let fe2 = HgFileEnvelope::from_thrift(thrift_fe)
                .expect("thrift roundtrips should always be valid");
            fe == fe2
        }

        fn blob_roundtrip(fe: HgFileEnvelope) -> bool {
            let blob = fe.clone().into_blob();
            let fe2 = HgFileEnvelope::from_blob(blob)
                .expect("blob roundtrips should always be valid");
            fe == fe2
        }
    }

    #[test]
    fn bad_thrift() {
        let mut thrift_fe = thrift::HgFileEnvelope {
            node_id: thrift::HgNodeHash(thrift::Sha1(vec![1; 20])),
            p1: Some(thrift::HgNodeHash(thrift::Sha1(vec![2; 20]))),
            p2: None,
            // a content ID must be present
            content_id: None,
            content_size: 42,
            metadata: Some(vec![].into()),
        };

        HgFileEnvelope::from_thrift(thrift_fe.clone())
            .expect_err("unexpected OK -- missing content ID");

        thrift_fe.content_id = Some(thrift::ContentId(thrift::IdType::Blake2(thrift::Blake2(
            vec![3; 32],
        ))));
        thrift_fe.metadata = None;

        HgFileEnvelope::from_thrift(thrift_fe).expect_err("unexpected OK -- missing metadata");
    }
}
