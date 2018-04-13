// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! A hash of a node (changeset, manifest or file).

use std::fmt::{self, Display};
use std::result;
use std::str::FromStr;

use ascii::AsciiStr;

use quickcheck::{single_shrinker, Arbitrary, Gen};

use errors::*;
use hash::{self, Sha1};
use serde;
use sql_types::{HgChangesetIdSql, HgFileNodeIdSql, HgManifestIdSql};

pub const NULL_HASH: NodeHash = NodeHash(hash::NULL);
pub const NULL_CSID: HgChangesetId = HgChangesetId(NULL_HASH);

/// This structure represents Sha1 based hashes that are used in Mononoke. It is a temporary
/// structure that will be entirely replaced by structures from mononoke-types::typed_hash.
/// It's current distinction from mercurial::NodeHash serves two purposes:
/// - make it relatively straightforward to replace it in future with typed_hash
/// - easily distinguish between the NodeHash values provided by Mercurial client that might
///   require remapping, f.e. hashes of Changeset and hashes of Root Manifests since the client
///   provides Flat Manifest hashes as aliases for Root Manifest hashes
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
#[derive(HeapSizeOf)]
pub struct NodeHash(pub(crate) Sha1);

impl NodeHash {
    #[deprecated(note = "This constructor is only used in two places: \
                         conversion from mercurial NodeHash and creation of NodeHash mocks")]
    pub const fn new(sha1: Sha1) -> NodeHash {
        NodeHash(sha1)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<NodeHash> {
        Sha1::from_bytes(bytes).map(NodeHash)
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_ref()
    }

    #[inline]
    pub fn from_ascii_str(s: &AsciiStr) -> Result<NodeHash> {
        Sha1::from_ascii_str(s).map(NodeHash)
    }

    #[inline]
    pub fn into_option(self) -> Option<Self> {
        if self == NULL_HASH {
            None
        } else {
            Some(self)
        }
    }

    #[deprecated(note = "This method is used only to have a \
                         zero-cost conversion to mercurial::NodeHash")]
    pub fn into_sha1(self) -> Sha1 {
        self.0
    }
}

impl From<Option<NodeHash>> for NodeHash {
    fn from(h: Option<NodeHash>) -> Self {
        match h {
            None => NULL_HASH,
            Some(h) => h,
        }
    }
}

struct StringVisitor;

impl<'de> serde::de::Visitor<'de> for StringVisitor {
    type Value = String;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("40 hex digits")
    }

    fn visit_str<E>(self, value: &str) -> ::std::result::Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(value.to_string())
    }

    fn visit_string<E>(self, value: String) -> ::std::result::Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(value)
    }
}

impl serde::ser::Serialize for NodeHash {
    fn serialize<S>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.0.to_hex().as_str())
    }
}

impl<'de> serde::de::Deserialize<'de> for NodeHash {
    fn deserialize<D>(deserializer: D) -> ::std::result::Result<NodeHash, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        let hex = deserializer.deserialize_string(StringVisitor)?;
        match Sha1::from_str(hex.as_str()) {
            Ok(sha1) => Ok(NodeHash(sha1)),
            Err(error) => Err(serde::de::Error::custom(error)),
        }
    }
}

impl FromStr for NodeHash {
    type Err = <Sha1 as FromStr>::Err;

    fn from_str(s: &str) -> result::Result<NodeHash, Self::Err> {
        Sha1::from_str(s).map(NodeHash)
    }
}

impl Display for NodeHash {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(fmt)
    }
}

impl Arbitrary for NodeHash {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        NodeHash(Sha1::arbitrary(g))
    }

    fn shrink(&self) -> Box<Iterator<Item = Self>> {
        single_shrinker(NULL_HASH)
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
#[derive(HeapSizeOf, FromSqlRow, AsExpression)]
#[sql_type = "HgChangesetIdSql"]
pub struct HgChangesetId(NodeHash);

impl HgChangesetId {
    #[inline]
    pub fn from_ascii_str(s: &AsciiStr) -> Result<HgChangesetId> {
        NodeHash::from_ascii_str(s).map(HgChangesetId)
    }

    #[inline]
    pub(crate) fn as_nodehash(&self) -> &NodeHash {
        &self.0
    }

    pub fn into_nodehash(self) -> NodeHash {
        self.0
    }

    pub const fn new(hash: NodeHash) -> Self {
        HgChangesetId(hash)
    }
}

impl FromStr for HgChangesetId {
    type Err = <NodeHash as FromStr>::Err;

    fn from_str(s: &str) -> result::Result<HgChangesetId, Self::Err> {
        NodeHash::from_str(s).map(HgChangesetId)
    }
}

impl Display for HgChangesetId {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(fmt)
    }
}

impl serde::ser::Serialize for HgChangesetId {
    fn serialize<S>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> serde::de::Deserialize<'de> for HgChangesetId {
    fn deserialize<D>(deserializer: D) -> ::std::result::Result<HgChangesetId, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        let hex = deserializer.deserialize_string(StringVisitor)?;
        match NodeHash::from_str(hex.as_str()) {
            Ok(hash) => Ok(HgChangesetId::new(hash)),
            Err(error) => Err(serde::de::Error::custom(error)),
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
#[derive(HeapSizeOf, FromSqlRow, AsExpression)]
#[sql_type = "HgManifestIdSql"]
pub struct HgManifestId(NodeHash);

impl HgManifestId {
    #[inline]
    pub(crate) fn as_nodehash(&self) -> &NodeHash {
        &self.0
    }

    pub fn into_nodehash(self) -> NodeHash {
        self.0
    }

    pub const fn new(hash: NodeHash) -> Self {
        HgManifestId(hash)
    }
}

impl Display for HgManifestId {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(fmt)
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
#[derive(HeapSizeOf, FromSqlRow, AsExpression)]
#[sql_type = "HgFileNodeIdSql"]
pub struct HgFileNodeId(NodeHash);

impl HgFileNodeId {
    #[inline]
    pub(crate) fn as_nodehash(&self) -> &NodeHash {
        &self.0
    }

    pub fn into_nodehash(self) -> NodeHash {
        self.0
    }

    pub const fn new(hash: NodeHash) -> Self {
        HgFileNodeId(hash)
    }
}

impl Display for HgFileNodeId {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(fmt)
    }
}

/// TODO: (jsgf) T25576292 EntryId should be a (Type, NodeId) tuple
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
#[derive(HeapSizeOf)]
pub struct EntryId(NodeHash);

impl EntryId {
    pub fn into_nodehash(self) -> NodeHash {
        self.0
    }

    pub fn new(hash: NodeHash) -> Self {
        EntryId(hash)
    }
}

impl Display for EntryId {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(fmt)
    }
}
