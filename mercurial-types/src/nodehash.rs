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
use quickcheck::{Arbitrary, Gen};
use serde;
use thrift;

use RepoPath;
use errors::*;
use hash::{self, Sha1};
use sql_types::{HgChangesetIdSql, HgFileNodeIdSql, HgManifestIdSql};

pub const NULL_HASH: HgNodeHash = HgNodeHash(hash::NULL);
pub const NULL_CSID: HgChangesetId = HgChangesetId(NULL_HASH);

/// This structure represents Sha1 based hashes that are used in Mercurial, but the Sha1
/// structure is private outside this crate to keep it an implementation detail.
/// This is why the main constructors to create this structure are from_bytes and from_ascii_str
/// which parses raw bytes or hex string to create HgNodeHash.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
#[derive(HeapSizeOf)]
pub struct HgNodeHash(pub(crate) Sha1);

impl HgNodeHash {
    #[deprecated(note = "This constructor is only used in creation of HgNodeHash mocks")]
    pub const fn new(sha1: Sha1) -> Self {
        HgNodeHash(sha1)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        Sha1::from_bytes(bytes).map(HgNodeHash)
    }

    pub(crate) fn from_thrift(thrift_hash: thrift::HgNodeHash) -> Result<Self> {
        Ok(HgNodeHash(Sha1::from_thrift(thrift_hash.0)?))
    }

    pub(crate) fn from_thrift_opt(
        thrift_hash_opt: Option<thrift::HgNodeHash>,
    ) -> Result<Option<Self>> {
        match thrift_hash_opt {
            Some(h) => Ok(Some(Self::from_thrift(h)?)),
            None => Ok(None),
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_ref()
    }

    pub fn from_static_str(hash: &'static str) -> Result<Self> {
        Sha1::from_str(hash).map(HgNodeHash)
    }

    pub fn sha1(&self) -> &Sha1 {
        &self.0
    }

    #[inline]
    pub fn from_ascii_str(s: &AsciiStr) -> Result<Self> {
        Sha1::from_ascii_str(s).map(HgNodeHash)
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

    pub(crate) fn into_thrift(self) -> thrift::HgNodeHash {
        thrift::HgNodeHash(self.0.into_thrift())
    }
}

impl From<Option<HgNodeHash>> for HgNodeHash {
    fn from(h: Option<HgNodeHash>) -> Self {
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

impl serde::ser::Serialize for HgNodeHash {
    fn serialize<S>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.to_hex().as_str())
    }
}

impl<'de> serde::de::Deserialize<'de> for HgNodeHash {
    fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        let hex = deserializer.deserialize_string(StringVisitor)?;
        match Sha1::from_str(hex.as_str()) {
            Ok(sha1) => Ok(HgNodeHash(sha1)),
            Err(error) => Err(serde::de::Error::custom(error)),
        }
    }
}

impl AsRef<[u8]> for HgNodeHash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl FromStr for HgNodeHash {
    type Err = <Sha1 as FromStr>::Err;

    fn from_str(s: &str) -> result::Result<Self, Self::Err> {
        Sha1::from_str(s).map(HgNodeHash)
    }
}

impl Display for HgNodeHash {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(fmt)
    }
}

impl Arbitrary for HgNodeHash {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        HgNodeHash(Sha1::arbitrary(g))
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
#[derive(HeapSizeOf, FromSqlRow, AsExpression)]
#[sql_type = "HgChangesetIdSql"]
pub struct HgChangesetId(HgNodeHash);

impl HgChangesetId {
    #[inline]
    pub fn from_ascii_str(s: &AsciiStr) -> Result<HgChangesetId> {
        HgNodeHash::from_ascii_str(s).map(HgChangesetId)
    }

    #[inline]
    pub fn as_nodehash(&self) -> &HgNodeHash {
        &self.0
    }

    pub fn into_nodehash(self) -> HgNodeHash {
        self.0
    }

    pub const fn new(hash: HgNodeHash) -> Self {
        HgChangesetId(hash)
    }

    #[inline]
    pub fn to_hex(&self) -> AsciiString {
        self.0.to_hex()
    }

    /// Produce a key suitable for using in a blobstore.
    #[inline]
    pub fn blobstore_key(&self) -> String {
        format!("hgchangeset.sha1.{}", self.0)
    }
}

impl FromStr for HgChangesetId {
    type Err = <HgNodeHash as FromStr>::Err;

    fn from_str(s: &str) -> result::Result<HgChangesetId, Self::Err> {
        HgNodeHash::from_str(s).map(HgChangesetId)
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
        match HgNodeHash::from_str(hex.as_str()) {
            Ok(hash) => Ok(HgChangesetId::new(hash)),
            Err(error) => Err(serde::de::Error::custom(error)),
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
#[derive(HeapSizeOf, FromSqlRow, AsExpression)]
#[sql_type = "HgManifestIdSql"]
pub struct HgManifestId(HgNodeHash);

impl HgManifestId {
    #[inline]
    pub(crate) fn as_nodehash(&self) -> &HgNodeHash {
        &self.0
    }

    pub fn into_nodehash(self) -> HgNodeHash {
        self.0
    }

    pub const fn new(hash: HgNodeHash) -> Self {
        HgManifestId(hash)
    }

    /// Produce a key suitable for using in a blobstore.
    #[inline]
    pub fn blobstore_key(&self) -> String {
        format!("hgmanifest.sha1.{}", self.0)
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
pub struct HgFileNodeId(HgNodeHash);

impl HgFileNodeId {
    #[inline]
    pub(crate) fn as_nodehash(&self) -> &HgNodeHash {
        &self.0
    }

    pub fn into_nodehash(self) -> HgNodeHash {
        self.0
    }

    pub const fn new(hash: HgNodeHash) -> Self {
        HgFileNodeId(hash)
    }

    /// Produce a key suitable for using in a blobstore.
    #[inline]
    pub fn blobstore_key(&self) -> String {
        format!("hgfilenode.sha1.{}", self.0)
    }
}

impl Display for HgFileNodeId {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(fmt)
    }
}

/// TODO: (jsgf) T25576292 HgEntryId should be a (Type, NodeId) tuple
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
#[derive(HeapSizeOf)]
pub struct HgEntryId(HgNodeHash);

impl HgEntryId {
    pub fn into_nodehash(self) -> HgNodeHash {
        self.0
    }

    pub const fn new(hash: HgNodeHash) -> Self {
        HgEntryId(hash)
    }
}

impl Display for HgEntryId {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(fmt)
    }
}

/// A (path, hash) combination. This is the key used throughout Mercurial for manifest and file
/// nodes.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct HgNodeKey {
    pub path: RepoPath,
    pub hash: HgNodeHash,
}

impl Display for HgNodeKey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "path: {}, hash: {}", self.path, self.hash)
    }
}
