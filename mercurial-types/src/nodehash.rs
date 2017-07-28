// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! A hash of a node (changeset, manifest or file).

use std::fmt::{self, Display};
use std::str::FromStr;
use std::result;

use ascii::{AsciiStr, AsciiString};
use quickcheck::{Arbitrary, Gen, single_shrinker};

use errors::*;
use hash::{self, Sha1};
use serde;

pub const NULL_HASH: NodeHash = NodeHash(hash::NULL);

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
#[derive(HeapSizeOf)]
pub struct NodeHash(Sha1);

impl NodeHash {
    pub fn new(sha1: Sha1) -> NodeHash {
        NodeHash(sha1)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<NodeHash> {
        Sha1::from_bytes(bytes).map(NodeHash)
    }

    #[inline]
    pub fn from_ascii_str(s: &AsciiStr) -> Result<NodeHash> {
        Sha1::from_ascii_str(s).map(NodeHash)
    }

    pub fn sha1(&self) -> &Sha1 {
        &self.0
    }

    #[inline]
    pub fn to_hex(&self) -> AsciiString {
        self.0.to_hex()
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
            Ok(sha1) => Ok(NodeHash::new(sha1)),
            Err(error) => Err(serde::de::Error::custom(error)),
        }
    }
}

impl From<Sha1> for NodeHash {
    fn from(h: Sha1) -> NodeHash {
        NodeHash(h)
    }
}

impl<'a> From<&'a Sha1> for NodeHash {
    fn from(h: &'a Sha1) -> NodeHash {
        NodeHash(*h)
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
