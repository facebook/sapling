// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Envelopes used for file nodes.

use bytes::Bytes;
use failure::{err_msg, SyncFailure};
use quickcheck::{empty_shrinker, Arbitrary, Gen};

use rust_thrift::compact_protocol;

use mononoke_types::ContentId;

use super::HgEnvelopeBlob;
use errors::*;
use nodehash::HgNodeHash;
use thrift;

/// A mutable representation of a Mercurial file node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HgFileEnvelopeMut {
    pub node_id: HgNodeHash,
    pub p1: Option<HgNodeHash>,
    pub p2: Option<HgNodeHash>,
    pub content_id: ContentId,
    pub content_size: u64,
    pub metadata: Bytes,
}

impl HgFileEnvelopeMut {
    pub fn freeze(self) -> HgFileEnvelope {
        HgFileEnvelope { inner: self }
    }
}

/// A serialized representation of a Mercurial file node in the blob store.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HgFileEnvelope {
    inner: HgFileEnvelopeMut,
}

impl HgFileEnvelope {
    pub(crate) fn from_thrift(fe: thrift::HgFileEnvelope) -> Result<Self> {
        let catch_block = || {
            Ok(Self {
                inner: HgFileEnvelopeMut {
                    node_id: HgNodeHash::from_thrift(fe.node_id)?,
                    p1: HgNodeHash::from_thrift_opt(fe.p1)?,
                    p2: HgNodeHash::from_thrift_opt(fe.p2)?,
                    content_id: ContentId::from_thrift(fe.content_id
                        .ok_or_else(|| err_msg("missing content id field"))?)?,
                    content_size: fe.content_size as u64,
                    // metadata will always be stored, even if it's length 0
                    metadata: Bytes::from(fe.metadata
                        .ok_or_else(|| err_msg("missing metadata field"))?),
                },
            })
        };

        Ok(catch_block().with_context(|_: &Error| {
            ErrorKind::InvalidThrift("HgFileEnvelope".into(), "Invalid file envelope".into())
        })?)
    }

    pub fn from_blob(blob: HgEnvelopeBlob) -> Result<Self> {
        // TODO (T27336549) stop using SyncFailure once thrift is converted to failure
        let thrift_tc = compact_protocol::deserialize(blob.0.as_ref())
            .map_err(SyncFailure::new)
            .context(ErrorKind::BlobDeserializeError("HgFileEnvelope".into()))?;
        Self::from_thrift(thrift_tc)
    }

    /// The ID for this file version.
    #[inline]
    pub fn node_id(&self) -> &HgNodeHash {
        &self.inner.node_id
    }

    /// The parent hashes for this node. The order matters.
    #[inline]
    pub fn parents(&self) -> (Option<&HgNodeHash>, Option<&HgNodeHash>) {
        (self.inner.p1.as_ref(), self.inner.p2.as_ref())
    }

    /// The content ID -- this can be used to retrieve the contents.
    #[inline]
    pub fn content_id(&self) -> &ContentId {
        &self.inner.content_id
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
            node_id: inner.node_id.into_thrift(),
            p1: inner.p1.map(HgNodeHash::into_thrift),
            p2: inner.p2.map(HgNodeHash::into_thrift),
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

    fn shrink(&self) -> Box<Iterator<Item = Self>> {
        empty_shrinker()
    }
}

#[cfg(test)]
mod test {
    use super::*;

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
