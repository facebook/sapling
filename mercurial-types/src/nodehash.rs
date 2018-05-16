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
use sql_types::{DChangesetIdSql, DFileNodeIdSql, DManifestIdSql};

pub const D_NULL_HASH: DNodeHash = DNodeHash(hash::NULL);
pub const NULL_CSID: DChangesetId = DChangesetId(D_NULL_HASH);

/// This structure represents Sha1 based hashes that are used in Mononoke. It is a temporary
/// structure that will be entirely replaced by structures from mononoke-types::typed_hash.
/// It's current distinction from HgNodeHash serves two purposes:
/// - make it relatively straightforward to replace it in future with typed_hash
/// - easily distinguish between the HgNodeHash values provided by Mercurial client that might
///   require remapping, f.e. hashes of Changeset and hashes of Root Manifests since the client
///   provides Flat Manifest hashes as aliases for Root Manifest hashes
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
#[derive(HeapSizeOf)]
pub struct DNodeHash(pub(crate) Sha1);

impl DNodeHash {
    #[deprecated(note = "This constructor is only used in two places: \
                         conversion from mercurial HgNodeHash and creation of HgNodeHash mocks")]
    pub const fn new(sha1: Sha1) -> Self {
        DNodeHash(sha1)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        Sha1::from_bytes(bytes).map(DNodeHash)
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_ref()
    }

    pub fn from_static_str(hash: &'static str) -> Result<Self> {
        Sha1::from_str(hash).map(DNodeHash)
    }

    pub fn sha1(&self) -> &Sha1 {
        &self.0
    }

    #[inline]
    pub fn from_ascii_str(s: &AsciiStr) -> Result<Self> {
        Sha1::from_ascii_str(s).map(DNodeHash)
    }

    #[inline]
    pub fn into_option(self) -> Option<Self> {
        if self == D_NULL_HASH {
            None
        } else {
            Some(self)
        }
    }

    #[deprecated(note = "This method is used only to have a \
                         zero-cost conversion to HgNodeHash")]
    pub fn into_sha1(self) -> Sha1 {
        self.0
    }
}

impl From<Option<DNodeHash>> for DNodeHash {
    fn from(h: Option<DNodeHash>) -> Self {
        match h {
            None => D_NULL_HASH,
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

impl serde::ser::Serialize for DNodeHash {
    fn serialize<S>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.0.to_hex().as_str())
    }
}

impl<'de> serde::de::Deserialize<'de> for DNodeHash {
    fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        let hex = deserializer.deserialize_string(StringVisitor)?;
        match Sha1::from_str(hex.as_str()) {
            Ok(sha1) => Ok(DNodeHash(sha1)),
            Err(error) => Err(serde::de::Error::custom(error)),
        }
    }
}

impl FromStr for DNodeHash {
    type Err = <Sha1 as FromStr>::Err;

    fn from_str(s: &str) -> result::Result<Self, Self::Err> {
        Sha1::from_str(s).map(DNodeHash)
    }
}

impl Display for DNodeHash {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(fmt)
    }
}

impl Arbitrary for DNodeHash {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        DNodeHash(Sha1::arbitrary(g))
    }

    fn shrink(&self) -> Box<Iterator<Item = Self>> {
        single_shrinker(D_NULL_HASH)
    }
}

/// This structure represents Sha1 based hashes of Changesets used in Mononoke. It is a temporary
/// structure that will be entirely replaced by structures from mononoke-types::typed_hash.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
#[derive(HeapSizeOf, FromSqlRow, AsExpression)]
#[sql_type = "DChangesetIdSql"]
pub struct DChangesetId(DNodeHash);

impl DChangesetId {
    #[inline]
    pub fn from_ascii_str(s: &AsciiStr) -> Result<DChangesetId> {
        DNodeHash::from_ascii_str(s).map(DChangesetId)
    }

    #[inline]
    pub(crate) fn as_nodehash(&self) -> &DNodeHash {
        &self.0
    }

    pub fn into_nodehash(self) -> DNodeHash {
        self.0
    }

    pub const fn new(hash: DNodeHash) -> Self {
        DChangesetId(hash)
    }
}

impl FromStr for DChangesetId {
    type Err = <DNodeHash as FromStr>::Err;

    fn from_str(s: &str) -> result::Result<DChangesetId, Self::Err> {
        DNodeHash::from_str(s).map(DChangesetId)
    }
}

impl Display for DChangesetId {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(fmt)
    }
}

impl serde::ser::Serialize for DChangesetId {
    fn serialize<S>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> serde::de::Deserialize<'de> for DChangesetId {
    fn deserialize<D>(deserializer: D) -> ::std::result::Result<DChangesetId, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        let hex = deserializer.deserialize_string(StringVisitor)?;
        match DNodeHash::from_str(hex.as_str()) {
            Ok(hash) => Ok(DChangesetId::new(hash)),
            Err(error) => Err(serde::de::Error::custom(error)),
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
#[derive(HeapSizeOf, FromSqlRow, AsExpression)]
#[sql_type = "DManifestIdSql"]
pub struct DManifestId(DNodeHash);

impl DManifestId {
    #[inline]
    pub(crate) fn as_nodehash(&self) -> &DNodeHash {
        &self.0
    }

    pub fn into_nodehash(self) -> DNodeHash {
        self.0
    }

    pub const fn new(hash: DNodeHash) -> Self {
        DManifestId(hash)
    }
}

impl Display for DManifestId {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(fmt)
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
#[derive(HeapSizeOf, FromSqlRow, AsExpression)]
#[sql_type = "DFileNodeIdSql"]
pub struct DFileNodeId(DNodeHash);

impl DFileNodeId {
    #[inline]
    pub(crate) fn as_nodehash(&self) -> &DNodeHash {
        &self.0
    }

    pub fn into_nodehash(self) -> DNodeHash {
        self.0
    }

    pub const fn new(hash: DNodeHash) -> Self {
        DFileNodeId(hash)
    }
}

impl Display for DFileNodeId {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(fmt)
    }
}

/// TODO: (jsgf) T25576292 DEntryId should be a (Type, NodeId) tuple
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
#[derive(HeapSizeOf)]
pub struct DEntryId(DNodeHash);

impl DEntryId {
    pub fn into_nodehash(self) -> DNodeHash {
        self.0
    }

    pub fn new(hash: DNodeHash) -> Self {
        DEntryId(hash)
    }
}

impl Display for DEntryId {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(fmt)
    }
}
