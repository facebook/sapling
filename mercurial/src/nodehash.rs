// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! A hash of a node (changeset, manifest or file).

use std::fmt::{self, Display};
use std::result;
use std::str::FromStr;

use ascii::{AsciiStr, AsciiString};
use quickcheck::{single_shrinker, Arbitrary, Gen};

use errors::*;
use mercurial_types::{DNodeHash, RepoPath};
use mercurial_types::hash::{self, Sha1};
use serde;
use sql_types::{HgChangesetIdSql, HgManifestIdSql};

pub const NULL_HASH: NodeHash = NodeHash(hash::NULL);

/// This structure represents Sha1 based hashes that are used in Mercurial, but the Sha1
/// structure is private outside this crate to keep it an implementation detail.
/// This is why the main constructors to create this structure are from_bytes and from_ascii_str
/// which parses raw bytes or hex string to create NodeHash.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
#[derive(HeapSizeOf)]
pub struct NodeHash(pub(crate) Sha1);

impl NodeHash {
    /// Constructor to be used to parse 20 raw bytes that represent a sha1 hash into NodeHash
    pub fn from_bytes(bytes: &[u8]) -> Result<NodeHash> {
        Sha1::from_bytes(bytes).map(NodeHash)
    }

    /// Returns the underlying 20 raw bytes that represent a sha1 hash
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_ref()
    }

    /// Constructor to be used to parse 40 hex digits that represent a sha1 hash into NodeHash
    #[inline]
    pub fn from_ascii_str(s: &AsciiStr) -> Result<NodeHash> {
        Sha1::from_ascii_str(s).map(NodeHash)
    }

    /// Returns a 40 hex digits representation of the sha1 hash
    #[inline]
    pub fn to_hex(&self) -> AsciiString {
        self.0.to_hex()
    }

    #[inline]
    pub fn into_option(self) -> Option<Self> {
        if self == NULL_HASH {
            None
        } else {
            Some(self)
        }
    }

    /// Method used to convert a Mercurial Sha1 based NodeHash into Mononoke Sha1 based NodeHash
    /// without performing lookups in a remapping tables. It should be used only on Filenodes and
    /// Manifests that are not Root Manifests.
    /// This method is temporary (as the mercurial_types hashes are) and will go away once
    /// transision to BonsaiChangesets is complete
    #[inline]
    pub fn into_mononoke(self) -> DNodeHash {
        #![allow(deprecated)]
        DNodeHash::new(self.0)
    }

    /// Returns true if self Mercurial hash is equal to Mononoke Sha1 based hash
    pub fn is_equal_to(&self, hash: DNodeHash) -> bool {
        self.as_bytes() == hash.as_bytes()
    }
}

/// Trait to convieniently track the places where Mononoke to Mercurial NodeHash coversion is
/// taking place without performing a lookup in remapping tables.
pub trait NodeHashConversion {
    fn into_mercurial(self) -> NodeHash;
}

impl NodeHashConversion for DNodeHash {
    /// Method used to convert a Mononoke Sha1 based NodeHash into Mercurial Sha1 based NodeHash
    /// without performing lookups in a remapping tables. It should be used only on Filenodes and
    /// Manifests that are not Root Manifests.
    /// This method is temporary (as the mercurial_types hashes are) and will go away once
    /// transision to BonsaiChangesets is complete
    fn into_mercurial(self) -> NodeHash {
        #![allow(deprecated)]
        NodeHash(self.into_sha1())
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
        serializer.serialize_str(self.to_hex().as_str())
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

impl AsRef<[u8]> for NodeHash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
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

    #[inline]
    pub fn to_hex(&self) -> AsciiString {
        self.0.to_hex()
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

/// A (path, hash) combination. This is the key used throughout Mercurial for manifest and file
/// nodes.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct HgNodeKey {
    pub path: RepoPath,
    pub hash: NodeHash,
}
