// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! A Mononoke hash corresponding to a unode in the  (changeset, manifest or file).

use std::fmt::{self, Display};
use std::str::FromStr;

use ascii::{AsciiStr, AsciiString};
use quickcheck::{empty_shrinker, Arbitrary, Gen};

use errors::*;
use hash::{self, Blake2};
use thrift;

// There is no NULL_HASH for NodeHash. Any places that need a null hash should use an
// Option<UnodeHash>.

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
#[derive(HeapSizeOf)]
pub struct UnodeHash(Blake2);

impl UnodeHash {
    pub fn new(blake2: Blake2) -> Option<UnodeHash> {
        if blake2 == hash::NULL {
            None
        } else {
            Some(UnodeHash(blake2))
        }
    }

    /// This function is only meant to be used in mocks. Use `new` for most
    /// purposes.
    ///
    /// This function doesn't check the hash to see that it's null. It's defined separately because
    /// (at least as of Rust 1.24) const fns can't use `Eq`.
    pub const fn new_mock(blake2: Blake2) -> UnodeHash {
        UnodeHash(blake2)
    }

    pub(crate) fn from_thrift(h: thrift::UnodeHash) -> Result<Self> {
        // This assumes that a null hash is never serialized. This should always be the case.
        Ok(UnodeHash(Blake2::from_thrift(h.0)?))
    }

    #[inline]
    pub fn from_bytes(bytes: &[u8]) -> Result<Option<UnodeHash>> {
        Blake2::from_bytes(bytes).map(UnodeHash::new)
    }

    #[inline]
    pub fn from_str(s: &str) -> Result<Option<UnodeHash>> {
        Blake2::from_str(s).map(UnodeHash::new)
    }

    #[inline]
    pub fn from_ascii_str(s: &AsciiStr) -> Result<Option<UnodeHash>> {
        Blake2::from_ascii_str(s).map(UnodeHash::new)
    }

    pub fn blake2(&self) -> &Blake2 {
        &self.0
    }

    #[inline]
    pub fn to_hex(&self) -> AsciiString {
        self.0.to_hex()
    }

    pub(crate) fn into_thrift(self) -> thrift::UnodeHash {
        thrift::UnodeHash(self.0.into_thrift())
    }
}

impl From<Blake2> for Option<UnodeHash> {
    fn from(h: Blake2) -> Option<UnodeHash> {
        UnodeHash::new(h)
    }
}

impl<'a> From<&'a Blake2> for Option<UnodeHash> {
    fn from(h: &'a Blake2) -> Option<UnodeHash> {
        UnodeHash::new(*h)
    }
}

impl AsRef<[u8]> for UnodeHash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl Display for UnodeHash {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(fmt)
    }
}

impl Arbitrary for UnodeHash {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        // Blake2::arbitrary will sometimes generate a null hash, so don't use that
        // directly. (It's also theoretically possible that all the bytes are zeroes,
        // but the chance of that happening is vanishingly small.)
        let mut bytes = [0; 32];
        g.fill_bytes(&mut bytes);
        UnodeHash(Blake2::from_byte_array(bytes))
    }

    fn shrink(&self) -> Box<Iterator<Item = Self>> {
        empty_shrinker()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    quickcheck! {
        fn thrift_roundtrip(h: UnodeHash) -> bool {
            let v = h.into_thrift();
            let sh = UnodeHash::from_thrift(v).expect("converting a valid Thrift structure should always work");
            h == sh
        }
    }
}
