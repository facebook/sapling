/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

//! Envelopes used for Changeset nodes.

use std::fmt;

use bytes::Bytes;
use failure_ext::{chain::*, err_msg};
use fbthrift::compact_protocol;
use quickcheck::{empty_shrinker, Arbitrary, Gen};

use super::HgEnvelopeBlob;
use crate::errors::*;
use crate::nodehash::HgChangesetId;
use crate::thrift;

/// A mutable representation of a Mercurial changeset node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HgChangesetEnvelopeMut {
    pub node_id: HgChangesetId,
    pub p1: Option<HgChangesetId>,
    pub p2: Option<HgChangesetId>,
    pub contents: Bytes,
}

impl HgChangesetEnvelopeMut {
    pub fn freeze(self) -> HgChangesetEnvelope {
        HgChangesetEnvelope { inner: self }
    }
}

impl fmt::Display for HgChangesetEnvelopeMut {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "node id: {}", self.node_id)?;
        writeln!(f, "p1: {}", HgChangesetId::display_opt(self.p1.as_ref()))?;
        writeln!(f, "p2: {}", HgChangesetId::display_opt(self.p2.as_ref()))?;
        // TODO: (rain1) T30970792 parse contents and print out in a better fashion
        writeln!(f, "contents: {:?}", self.contents)
    }
}

/// A serialized representation of a Mercurial Changeset node in the blob store.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HgChangesetEnvelope {
    inner: HgChangesetEnvelopeMut,
}

impl HgChangesetEnvelope {
    pub(crate) fn from_thrift(fe: thrift::HgChangesetEnvelope) -> Result<Self> {
        let catch_block = || -> Result<_> {
            Ok(Self {
                inner: HgChangesetEnvelopeMut {
                    node_id: HgChangesetId::from_thrift(fe.node_id)?,
                    p1: HgChangesetId::from_thrift_opt(fe.p1)?,
                    p2: HgChangesetId::from_thrift_opt(fe.p2)?,
                    contents: Bytes::from(
                        fe.contents
                            .ok_or_else(|| err_msg("missing contents field"))?,
                    ),
                },
            })
        };

        Ok(catch_block().with_context(|| {
            ErrorKind::InvalidThrift(
                "HgChangesetEnvelope".into(),
                "Invalid Changeset envelope".into(),
            )
        })?)
    }

    pub fn from_blob(blob: HgEnvelopeBlob) -> Result<Self> {
        let thrift_tc = compact_protocol::deserialize(blob.0.as_ref()).chain_err(
            ErrorKind::BlobDeserializeError("HgChangesetEnvelope".into()),
        )?;
        Self::from_thrift(thrift_tc)
    }

    /// The ID for this changeset, as recorded by Mercurial. This is expected to match the
    /// actual hash computed from the contents.
    #[inline]
    pub fn node_id(&self) -> HgChangesetId {
        self.inner.node_id
    }

    /// The parent hashes for this node. The order matters.
    #[inline]
    pub fn parents(&self) -> (Option<HgChangesetId>, Option<HgChangesetId>) {
        (self.inner.p1, self.inner.p2)
    }

    /// The changeset contents as raw bytes.
    #[inline]
    pub fn contents(&self) -> &Bytes {
        &self.inner.contents
    }

    /// Convert into a mutable representation.
    #[inline]
    pub fn into_mut(self) -> HgChangesetEnvelopeMut {
        self.inner
    }

    pub(crate) fn into_thrift(self) -> thrift::HgChangesetEnvelope {
        let inner = self.inner;
        thrift::HgChangesetEnvelope {
            node_id: inner.node_id.into_thrift(),
            p1: inner.p1.map(HgChangesetId::into_thrift),
            p2: inner.p2.map(HgChangesetId::into_thrift),
            contents: Some(inner.contents.to_vec()),
        }
    }

    /// Serialize this structure into a blob.
    #[inline]
    pub fn into_blob(self) -> HgEnvelopeBlob {
        let thrift = self.into_thrift();
        HgEnvelopeBlob(compact_protocol::serialize(&thrift))
    }
}

impl fmt::Display for HgChangesetEnvelope {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.inner.fmt(f)
    }
}

impl Arbitrary for HgChangesetEnvelope {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        HgChangesetEnvelope {
            inner: HgChangesetEnvelopeMut {
                // XXX this doesn't ensure that the node ID actually matches the contents.
                // Might want to do that.
                node_id: Arbitrary::arbitrary(g),
                p1: Arbitrary::arbitrary(g),
                p2: Arbitrary::arbitrary(g),
                contents: Bytes::from(Vec::arbitrary(g)),
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
        fn thrift_roundtrip(ce: HgChangesetEnvelope) -> bool {
            let thrift_ce = ce.clone().into_thrift();
            let ce2 = HgChangesetEnvelope::from_thrift(thrift_ce)
                .expect("thrift roundtrips should always be valid");
            ce == ce2
        }

        fn blob_roundtrip(ce: HgChangesetEnvelope) -> bool {
            let blob = ce.clone().into_blob();
            let ce2 = HgChangesetEnvelope::from_blob(blob)
                .expect("blob roundtrips should always be valid");
            ce == ce2
        }
    }

    #[test]
    fn bad_thrift() {
        let mut thrift_ce = thrift::HgChangesetEnvelope {
            node_id: thrift::HgNodeHash(thrift::Sha1(vec![1; 20])),
            p1: Some(thrift::HgNodeHash(thrift::Sha1(vec![2; 20]))),
            p2: None,
            // contents must be present
            contents: None,
        };

        HgChangesetEnvelope::from_thrift(thrift_ce.clone())
            .expect_err("unexpected OK -- missing contents");

        thrift_ce.contents = Some(b"abc".to_vec());
        thrift_ce.node_id = thrift::HgNodeHash(thrift::Sha1(vec![1; 19]));

        HgChangesetEnvelope::from_thrift(thrift_ce)
            .expect_err("unexpected OK -- wrong hash length");
    }
}
